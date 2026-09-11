\set ON_ERROR_STOP on

-- Run as the migration owner. All fixture writes are rolled back.
BEGIN;

DO $$
BEGIN
    IF has_table_privilege('agrisense_farm', 'farm.activities', 'SELECT')
       OR has_table_privilege('agrisense_farm', 'farm.activities', 'INSERT')
       OR has_table_privilege('agrisense_farm', 'identity.users', 'SELECT')
       OR has_table_privilege('agrisense_farm', 'outbox.events', 'SELECT') THEN
        RAISE EXCEPTION 'agrisense_farm unexpectedly has direct table access';
    END IF;
    IF NOT has_function_privilege(
        'agrisense_farm',
        'farm.record_activity_for_current_phone(uuid,text,text,text,text,timestamptz)',
        'EXECUTE'
    ) THEN
        RAISE EXCEPTION 'agrisense_farm cannot execute the scoped activity command';
    END IF;
    IF has_table_privilege(
        'agrisense_brain', 'ai.activity_command_sessions', 'SELECT'
    ) OR has_table_privilege(
        'agrisense_brain', 'ai.activity_command_responses', 'INSERT'
    ) THEN
        RAISE EXCEPTION 'agrisense_brain unexpectedly has direct activity-state access';
    END IF;
    IF NOT has_function_privilege(
        'agrisense_brain',
        'ai.save_activity_command_session(text,jsonb,uuid,text)',
        'EXECUTE'
    ) THEN
        RAISE EXCEPTION 'agrisense_brain cannot execute scoped activity state command';
    END IF;
END
$$;

-- Brain state and response replay are tenant scoped even if a UUID collides.
SET LOCAL ROLE agrisense_brain;
SELECT set_config('app.current_phone', '+999000000000001', TRUE);
SELECT ai.save_activity_command_session(
    'details',
    '{}',
    '96000000-0000-0000-0000-000000000001',
    'Tenant A response'
);
SELECT set_config('app.current_phone', '+999000000000002', TRUE);
DO $$
BEGIN
    IF ai.get_activity_command_response(
        '96000000-0000-0000-0000-000000000001'
    ) IS NOT NULL THEN
        RAISE EXCEPTION 'tenant B can read tenant A activity response';
    END IF;

    BEGIN
        PERFORM ai.save_activity_command_session(
            'details',
            '{}',
            '96000000-0000-0000-0000-000000000001',
            'Tenant B collision'
        );
        RAISE EXCEPTION 'cross-tenant activity response UUID was accepted';
    EXCEPTION WHEN insufficient_privilege THEN
        NULL;
    END;
END
$$;
RESET ROLE;

INSERT INTO identity.users (id, phone, name, role, status)
VALUES
    ('91000000-0000-0000-0000-000000000001', '+999000000000001', 'Tenant A', 'farmer', 'active'),
    ('91000000-0000-0000-0000-000000000002', '+999000000000002', 'Tenant B', 'farmer', 'active');
INSERT INTO farm.farmers (id, user_id)
VALUES
    ('92000000-0000-0000-0000-000000000001', '91000000-0000-0000-0000-000000000001'),
    ('92000000-0000-0000-0000-000000000002', '91000000-0000-0000-0000-000000000002');
INSERT INTO farm.farms (id, farmer_id, name, area_hectares)
VALUES
    ('93000000-0000-0000-0000-000000000001', '92000000-0000-0000-0000-000000000001', 'Farm A', 1),
    ('93000000-0000-0000-0000-000000000002', '92000000-0000-0000-0000-000000000002', 'Farm B', 1);
INSERT INTO farm.crops (id, farm_id, crop_type, planted_at, status)
VALUES
    ('94000000-0000-0000-0000-000000000001', '93000000-0000-0000-0000-000000000001', 'melon', CURRENT_DATE, 'growing'),
    ('94000000-0000-0000-0000-000000000002', '93000000-0000-0000-0000-000000000002', 'melon', CURRENT_DATE, 'growing');

SET LOCAL ROLE agrisense_farm;
SELECT set_config('app.current_phone', '+999000000000001', TRUE);
SELECT activity_id, activity_type, already_existed
FROM farm.record_activity_for_current_phone(
    '95000000-0000-0000-0000-000000000001',
    'fertilizing',
    'Pemupukan tenant isolation test',
    '25',
    'kg',
    NOW()
);

-- Same owner + same inbound UUID is an idempotent replay.
SELECT activity_id, activity_type, already_existed
FROM farm.record_activity_for_current_phone(
    '95000000-0000-0000-0000-000000000001',
    'fertilizing',
    'Pemupukan tenant isolation test',
    '25',
    'kg',
    NOW()
);

SELECT set_config('app.current_phone', '+999000000000002', TRUE);
DO $$
DECLARE
    visible_count INTEGER;
BEGIN
    SELECT count(*) INTO visible_count
    FROM farm.list_recent_activities_for_current_phone(NULL, 20);
    IF visible_count <> 0 THEN
        RAISE EXCEPTION 'tenant B can see tenant A activity';
    END IF;

    BEGIN
        PERFORM * FROM farm.record_activity_for_current_phone(
            '95000000-0000-0000-0000-000000000001',
            'watering',
            'Cross-tenant collision attempt',
            NULL,
            NULL,
            NOW()
        );
        RAISE EXCEPTION 'cross-tenant idempotency collision was accepted';
    EXCEPTION WHEN insufficient_privilege THEN
        NULL;
    END;
END
$$;

RESET ROLE;

DO $$
BEGIN
    IF (SELECT count(*) FROM farm.activities
        WHERE source_request_id = '95000000-0000-0000-0000-000000000001') <> 1 THEN
        RAISE EXCEPTION 'idempotent replay created duplicate activities';
    END IF;
    IF (SELECT count(*) FROM outbox.events
        WHERE idempotency_key = 'farm_activity:95000000-0000-0000-0000-000000000001') <> 1 THEN
        RAISE EXCEPTION 'domain write and outbox event are not one-to-one';
    END IF;
END
$$;

ROLLBACK;
