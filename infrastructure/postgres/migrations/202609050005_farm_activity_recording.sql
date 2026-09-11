-- Durable, tenant-safe farm activity recording.
-- Domain writes are owned by farm-service; Brain only owns confirmation state.

BEGIN;

ALTER TABLE farm.activities
    ADD COLUMN IF NOT EXISTS source_request_id UUID,
    ADD COLUMN IF NOT EXISTS recorded_via VARCHAR(30) NOT NULL DEFAULT 'manual';

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'activities_activity_type_check'
          AND conrelid = 'farm.activities'::regclass
    ) THEN
        ALTER TABLE farm.activities ADD CONSTRAINT activities_activity_type_check
            CHECK (activity_type IN (
                'watering', 'fertilizing', 'spraying', 'pruning', 'inspection'
            )) NOT VALID;
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'activities_recorded_via_check'
          AND conrelid = 'farm.activities'::regclass
    ) THEN
        ALTER TABLE farm.activities ADD CONSTRAINT activities_recorded_via_check
            CHECK (recorded_via IN ('manual', 'whatsapp', 'api')) NOT VALID;
    END IF;
END
$$;

CREATE UNIQUE INDEX IF NOT EXISTS uq_farm_activities_source_request
    ON farm.activities (source_request_id)
    WHERE source_request_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_farm_activities_owner_time
    ON farm.activities (farm_id, performed_at DESC);

CREATE TABLE IF NOT EXISTS ai.activity_command_sessions (
    phone_digest    BYTEA PRIMARY KEY,
    phone           VARCHAR(20) NOT NULL UNIQUE,
    current_step    VARCHAR(20) NOT NULL
                    CHECK (current_step IN ('details', 'confirm')),
    draft           JSONB NOT NULL DEFAULT '{}'
                    CHECK (jsonb_typeof(draft) = 'object'),
    status          VARCHAR(20) NOT NULL DEFAULT 'active'
                    CHECK (status IN ('active', 'completed', 'cancelled')),
    version         INTEGER NOT NULL DEFAULT 1,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at    TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_ai_activity_command_active
    ON ai.activity_command_sessions (updated_at)
    WHERE status = 'active';

CREATE TABLE IF NOT EXISTS ai.activity_command_responses (
    request_id      UUID PRIMARY KEY,
    phone_digest    BYTEA NOT NULL,
    response        TEXT NOT NULL CHECK (length(response) BETWEEN 1 AND 4000),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_ai_activity_command_responses_owner
    ON ai.activity_command_responses (phone_digest, created_at DESC);

ALTER TABLE ai.activity_command_sessions ENABLE ROW LEVEL SECURITY;
ALTER TABLE ai.activity_command_sessions FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_activity_command_sessions ON ai.activity_command_sessions;
CREATE POLICY tenant_activity_command_sessions ON ai.activity_command_sessions TO agrisense_tenant
    USING (phone_digest = identity.current_phone_digest())
    WITH CHECK (phone_digest = identity.current_phone_digest());

ALTER TABLE ai.activity_command_responses ENABLE ROW LEVEL SECURITY;
ALTER TABLE ai.activity_command_responses FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_activity_command_responses ON ai.activity_command_responses;
CREATE POLICY tenant_activity_command_responses ON ai.activity_command_responses TO agrisense_tenant
    USING (phone_digest = identity.current_phone_digest())
    WITH CHECK (phone_digest = identity.current_phone_digest());

-- Brain confirmation-state RPCs. Every function derives its owner from the
-- transaction-local phone scope and accepts no caller-supplied owner ID.
CREATE OR REPLACE FUNCTION ai.get_activity_command_session()
RETURNS TABLE (current_step TEXT, draft JSONB)
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog
AS $$
    SELECT s.current_step::TEXT, s.draft
    FROM ai.activity_command_sessions s
    WHERE s.phone_digest = identity.current_phone_digest()
      AND s.status = 'active'
$$;

CREATE OR REPLACE FUNCTION ai.get_activity_command_response(p_request_id UUID)
RETURNS TEXT
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog
AS $$
    SELECT r.response
    FROM ai.activity_command_responses r
    WHERE r.request_id = p_request_id
      AND r.phone_digest = identity.current_phone_digest()
$$;

CREATE OR REPLACE FUNCTION ai.save_activity_command_session(
    p_step TEXT,
    p_draft JSONB,
    p_request_id UUID,
    p_response TEXT
)
RETURNS VOID
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $$
DECLARE
    v_phone TEXT := identity.normalize_whatsapp_phone(
        current_setting('app.current_phone', FALSE)
    );
    v_digest BYTEA := identity.current_phone_digest();
BEGIN
    IF p_step NOT IN ('details', 'confirm') THEN
        RAISE EXCEPTION 'invalid activity command step' USING ERRCODE = '22023';
    END IF;
    IF p_draft IS NULL OR jsonb_typeof(p_draft) <> 'object'
       OR pg_column_size(p_draft) > 8192 THEN
        RAISE EXCEPTION 'invalid activity command draft' USING ERRCODE = '22023';
    END IF;
    IF length(p_response) NOT BETWEEN 1 AND 4000 THEN
        RAISE EXCEPTION 'invalid activity command response' USING ERRCODE = '22023';
    END IF;

    PERFORM pg_advisory_xact_lock(hashtextextended(v_phone, 0));
    INSERT INTO ai.activity_command_sessions
        (phone_digest, phone, current_step, draft, status)
    VALUES (v_digest, v_phone, p_step, p_draft, 'active')
    ON CONFLICT (phone_digest) DO UPDATE SET
        phone = EXCLUDED.phone,
        current_step = EXCLUDED.current_step,
        draft = EXCLUDED.draft,
        status = 'active',
        version = ai.activity_command_sessions.version + 1,
        updated_at = NOW(),
        completed_at = NULL;

    INSERT INTO ai.activity_command_responses (request_id, phone_digest, response)
    VALUES (p_request_id, v_digest, p_response)
    ON CONFLICT (request_id) DO NOTHING;
    IF NOT EXISTS (
        SELECT 1 FROM ai.activity_command_responses
        WHERE request_id = p_request_id AND phone_digest = v_digest
    ) THEN
        RAISE EXCEPTION 'activity response idempotency key belongs to another owner'
            USING ERRCODE = '42501';
    END IF;
END
$$;

CREATE OR REPLACE FUNCTION ai.cancel_activity_command_session(
    p_request_id UUID,
    p_response TEXT
)
RETURNS VOID
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $$
DECLARE
    v_digest BYTEA := identity.current_phone_digest();
BEGIN
    IF length(p_response) NOT BETWEEN 1 AND 4000 THEN
        RAISE EXCEPTION 'invalid activity command response' USING ERRCODE = '22023';
    END IF;

    UPDATE ai.activity_command_sessions
    SET status = 'cancelled', updated_at = NOW(), completed_at = NOW(),
        version = version + 1
    WHERE phone_digest = v_digest AND status = 'active';

    INSERT INTO ai.activity_command_responses (request_id, phone_digest, response)
    VALUES (p_request_id, v_digest, p_response)
    ON CONFLICT (request_id) DO NOTHING;
    IF NOT EXISTS (
        SELECT 1 FROM ai.activity_command_responses
        WHERE request_id = p_request_id AND phone_digest = v_digest
    ) THEN
        RAISE EXCEPTION 'activity response idempotency key belongs to another owner'
            USING ERRCODE = '42501';
    END IF;
END
$$;

CREATE OR REPLACE FUNCTION ai.complete_activity_command_session(
    p_request_id UUID,
    p_response TEXT
)
RETURNS VOID
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $$
DECLARE
    v_digest BYTEA := identity.current_phone_digest();
BEGIN
    IF length(p_response) NOT BETWEEN 1 AND 4000 THEN
        RAISE EXCEPTION 'invalid activity command response' USING ERRCODE = '22023';
    END IF;

    UPDATE ai.activity_command_sessions
    SET status = 'completed', updated_at = NOW(), completed_at = NOW(),
        version = version + 1
    WHERE phone_digest = v_digest AND status = 'active';

    IF NOT FOUND THEN
        RAISE EXCEPTION 'active activity command session not found' USING ERRCODE = 'P0002';
    END IF;

    INSERT INTO ai.activity_command_responses (request_id, phone_digest, response)
    VALUES (p_request_id, v_digest, p_response)
    ON CONFLICT (request_id) DO NOTHING;
    IF NOT EXISTS (
        SELECT 1 FROM ai.activity_command_responses
        WHERE request_id = p_request_id AND phone_digest = v_digest
    ) THEN
        RAISE EXCEPTION 'activity response idempotency key belongs to another owner'
            USING ERRCODE = '42501';
    END IF;
END
$$;

-- Farm domain command. Ownership, farm and active crop are all resolved from
-- app.current_phone; callers cannot provide or substitute any of those IDs.
CREATE OR REPLACE FUNCTION farm.record_activity_for_current_phone(
    p_request_id UUID,
    p_activity_type TEXT,
    p_description TEXT,
    p_quantity_text TEXT,
    p_unit TEXT,
    p_performed_at TIMESTAMPTZ
)
RETURNS TABLE (
    activity_id UUID,
    farm_id UUID,
    crop_id UUID,
    activity_type TEXT,
    description TEXT,
    quantity TEXT,
    unit TEXT,
    performed_at TIMESTAMPTZ,
    already_existed BOOLEAN
)
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $$
DECLARE
    v_phone TEXT := identity.normalize_whatsapp_phone(
        current_setting('app.current_phone', FALSE)
    );
    v_user_id UUID;
    v_farmer_id UUID;
    v_farm_id UUID;
    v_crop_id UUID;
    v_activity_id UUID;
    v_quantity NUMERIC(10,3);
    v_inserted BOOLEAN := FALSE;
BEGIN
    IF p_activity_type NOT IN (
        'watering', 'fertilizing', 'spraying', 'pruning', 'inspection'
    ) THEN
        RAISE EXCEPTION 'invalid farm activity type' USING ERRCODE = '22023';
    END IF;
    IF p_description IS NULL OR length(btrim(p_description)) NOT BETWEEN 2 AND 500
       OR p_description ~ '[[:cntrl:]]' THEN
        RAISE EXCEPTION 'invalid farm activity description' USING ERRCODE = '22023';
    END IF;
    IF p_performed_at IS NULL
       OR p_performed_at > NOW() + INTERVAL '5 minutes'
       OR p_performed_at < NOW() - INTERVAL '10 years' THEN
        RAISE EXCEPTION 'invalid farm activity time' USING ERRCODE = '22023';
    END IF;
    IF (p_quantity_text IS NULL) <> (p_unit IS NULL) THEN
        RAISE EXCEPTION 'quantity and unit must be supplied together' USING ERRCODE = '22023';
    END IF;
    IF p_quantity_text IS NOT NULL THEN
        IF p_quantity_text !~ '^[0-9]+(\.[0-9]{1,3})?$' THEN
            RAISE EXCEPTION 'invalid farm activity quantity precision' USING ERRCODE = '22023';
        END IF;
        BEGIN
            v_quantity := p_quantity_text::NUMERIC(10,3);
        EXCEPTION WHEN invalid_text_representation OR numeric_value_out_of_range THEN
            RAISE EXCEPTION 'invalid farm activity quantity' USING ERRCODE = '22023';
        END;
        IF v_quantity <= 0 OR length(p_unit) NOT BETWEEN 1 AND 30
           OR p_unit !~ '^[a-zA-Z0-9 ./%-]+$' THEN
            RAISE EXCEPTION 'invalid farm activity quantity or unit' USING ERRCODE = '22023';
        END IF;
    END IF;

    PERFORM pg_advisory_xact_lock(hashtextextended(v_phone, 0));

    SELECT u.id, fr.id, f.id, c.id
    INTO v_user_id, v_farmer_id, v_farm_id, v_crop_id
    FROM identity.users u
    JOIN farm.farmers fr ON fr.user_id = u.id
    JOIN farm.farms f ON f.farmer_id = fr.id
    JOIN farm.crops c ON c.farm_id = f.id AND c.status = 'growing'
    WHERE u.phone = v_phone
    ORDER BY c.planted_at DESC, c.created_at DESC, c.id
    LIMIT 1;

    IF v_crop_id IS NULL THEN
        RAISE EXCEPTION 'active crop not found for authenticated owner' USING ERRCODE = 'P0002';
    END IF;

    SELECT a.id INTO v_activity_id
    FROM farm.activities a
    JOIN farm.farms f ON f.id = a.farm_id
    JOIN farm.farmers fr ON fr.id = f.farmer_id
    WHERE a.source_request_id = p_request_id
      AND fr.user_id = v_user_id;

    IF v_activity_id IS NULL THEN
        IF EXISTS (
            SELECT 1 FROM farm.activities a
            WHERE a.source_request_id = p_request_id
        ) THEN
            RAISE EXCEPTION 'activity idempotency key belongs to another owner'
                USING ERRCODE = '42501';
        END IF;

        v_activity_id := public.uuid_generate_v4();
        INSERT INTO farm.activities (
            id, farm_id, crop_id, activity_type, description, quantity, unit,
            performed_at, source_request_id, recorded_via
        ) VALUES (
            v_activity_id, v_farm_id, v_crop_id, p_activity_type,
            btrim(p_description), v_quantity, lower(p_unit), p_performed_at,
            p_request_id, 'whatsapp'
        );
        v_inserted := TRUE;

        INSERT INTO outbox.events (
            id, aggregate_type, aggregate_id, event_type, nats_subject,
            payload, idempotency_key
        ) VALUES (
            public.uuid_generate_v4(), 'farm_activity', v_activity_id,
            'farm_activity_recorded', 'agrisense.farm.activity_recorded',
            jsonb_build_object(
                'event_id', v_activity_id,
                'occurred_at', NOW(),
                'source', 'farm-service',
                'event_type', 'farm_activity_recorded',
                'correlation_id', p_request_id,
                'payload', jsonb_build_object(
                    'activity_id', v_activity_id,
                    'farm_id', v_farm_id,
                    'crop_id', v_crop_id,
                    'farmer_id', v_farmer_id,
                    'activity_type', p_activity_type,
                    'description', btrim(p_description),
                    'quantity', v_quantity,
                    'unit', lower(p_unit),
                    'performed_at', p_performed_at
                )
            ),
            'farm_activity:' || p_request_id::TEXT
        )
        ON CONFLICT (idempotency_key) DO NOTHING;
    END IF;

    RETURN QUERY
    SELECT a.id, a.farm_id, a.crop_id, a.activity_type::TEXT,
           a.description, a.quantity::TEXT, a.unit::TEXT, a.performed_at,
           NOT v_inserted
    FROM farm.activities a
    WHERE a.id = v_activity_id;
END
$$;

CREATE OR REPLACE FUNCTION farm.list_recent_activities_for_current_phone(
    p_activity_type TEXT DEFAULT NULL,
    p_limit INTEGER DEFAULT 5
)
RETURNS TABLE (
    activity_id UUID,
    activity_type TEXT,
    description TEXT,
    quantity TEXT,
    unit TEXT,
    performed_at TIMESTAMPTZ
)
LANGUAGE plpgsql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog
AS $$
DECLARE
    v_phone TEXT := identity.normalize_whatsapp_phone(
        current_setting('app.current_phone', FALSE)
    );
BEGIN
    IF p_limit NOT BETWEEN 1 AND 20 THEN
        RAISE EXCEPTION 'activity query limit must be between 1 and 20' USING ERRCODE = '22023';
    END IF;
    IF p_activity_type IS NOT NULL AND p_activity_type NOT IN (
        'watering', 'fertilizing', 'spraying', 'pruning', 'inspection'
    ) THEN
        RAISE EXCEPTION 'invalid farm activity type filter' USING ERRCODE = '22023';
    END IF;

    RETURN QUERY
    WITH selected_crop AS (
        SELECT c.id
        FROM identity.users u
        JOIN farm.farmers fr ON fr.user_id = u.id
        JOIN farm.farms f ON f.farmer_id = fr.id
        JOIN farm.crops c ON c.farm_id = f.id AND c.status = 'growing'
        WHERE u.phone = v_phone
        ORDER BY c.planted_at DESC, c.created_at DESC, c.id
        LIMIT 1
    )
    SELECT a.id, a.activity_type::TEXT,
           COALESCE(a.description, 'Tanpa rincian'), a.quantity::TEXT,
           a.unit::TEXT, a.performed_at
    FROM selected_crop c
    JOIN farm.activities a ON a.crop_id = c.id
    WHERE p_activity_type IS NULL OR a.activity_type = p_activity_type
    ORDER BY a.performed_at DESC, a.created_at DESC, a.id
    LIMIT p_limit;
END
$$;

-- Outbox leasing/acknowledgement is restricted to this event type. The
-- service never receives general access to the shared outbox table.
CREATE OR REPLACE FUNCTION farm.claim_activity_outbox(p_limit INTEGER DEFAULT 50)
RETURNS TABLE (event_id UUID, nats_subject TEXT, payload JSONB)
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $$
BEGIN
    IF p_limit NOT BETWEEN 1 AND 100 THEN
        RAISE EXCEPTION 'outbox claim limit must be between 1 and 100' USING ERRCODE = '22023';
    END IF;

    RETURN QUERY
    WITH candidates AS (
        SELECT e.id
        FROM outbox.events e
        WHERE e.event_type = 'farm_activity_recorded'
          AND e.nats_subject = 'agrisense.farm.activity_recorded'
          AND e.status IN ('pending', 'failed')
          AND e.next_retry_at <= NOW()
        ORDER BY e.created_at, e.id
        FOR UPDATE SKIP LOCKED
        LIMIT p_limit
    )
    UPDATE outbox.events e
    SET next_retry_at = NOW() + INTERVAL '30 seconds'
    FROM candidates c
    WHERE e.id = c.id
    RETURNING e.id, e.nats_subject::TEXT, e.payload;
END
$$;

CREATE OR REPLACE FUNCTION farm.mark_activity_outbox_published(p_event_id UUID)
RETURNS VOID
LANGUAGE sql
SECURITY DEFINER
SET search_path = pg_catalog
AS $$
    UPDATE outbox.events
    SET status = 'published', published_at = NOW(), last_error = NULL
    WHERE id = p_event_id
      AND event_type = 'farm_activity_recorded'
      AND nats_subject = 'agrisense.farm.activity_recorded'
$$;

CREATE OR REPLACE FUNCTION farm.mark_activity_outbox_failed(
    p_event_id UUID,
    p_error TEXT
)
RETURNS VOID
LANGUAGE sql
SECURITY DEFINER
SET search_path = pg_catalog
AS $$
    UPDATE outbox.events
    SET retry_count = retry_count + 1,
        status = CASE WHEN retry_count + 1 >= max_retries
                      THEN 'dead_letter'::outbox.publish_status
                      ELSE 'failed'::outbox.publish_status END,
        last_error = left(p_error, 1000),
        next_retry_at = NOW() + make_interval(
            secs => LEAST(300, (2 ^ LEAST(retry_count + 1, 8))::INTEGER)
        )
    WHERE id = p_event_id
      AND event_type = 'farm_activity_recorded'
      AND nats_subject = 'agrisense.farm.activity_recorded'
$$;

-- Revoke broad access first, then grant only the narrow service boundaries.
REVOKE ALL ON TABLE ai.activity_command_sessions FROM PUBLIC, agrisense_brain;
REVOKE ALL ON TABLE ai.activity_command_responses FROM PUBLIC, agrisense_brain;
REVOKE ALL ON ALL TABLES IN SCHEMA farm FROM agrisense_farm;
REVOKE ALL ON ALL TABLES IN SCHEMA identity FROM agrisense_farm;
REVOKE ALL ON ALL TABLES IN SCHEMA outbox FROM agrisense_farm;
GRANT USAGE ON SCHEMA farm TO agrisense_farm;

REVOKE ALL ON FUNCTION ai.get_activity_command_session() FROM PUBLIC;
REVOKE ALL ON FUNCTION ai.get_activity_command_response(UUID) FROM PUBLIC;
REVOKE ALL ON FUNCTION ai.save_activity_command_session(TEXT, JSONB, UUID, TEXT) FROM PUBLIC;
REVOKE ALL ON FUNCTION ai.cancel_activity_command_session(UUID, TEXT) FROM PUBLIC;
REVOKE ALL ON FUNCTION ai.complete_activity_command_session(UUID, TEXT) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION ai.get_activity_command_session() TO agrisense_brain;
GRANT EXECUTE ON FUNCTION ai.get_activity_command_response(UUID) TO agrisense_brain;
GRANT EXECUTE ON FUNCTION ai.save_activity_command_session(TEXT, JSONB, UUID, TEXT) TO agrisense_brain;
GRANT EXECUTE ON FUNCTION ai.cancel_activity_command_session(UUID, TEXT) TO agrisense_brain;
GRANT EXECUTE ON FUNCTION ai.complete_activity_command_session(UUID, TEXT) TO agrisense_brain;

REVOKE ALL ON FUNCTION farm.record_activity_for_current_phone(
    UUID, TEXT, TEXT, TEXT, TEXT, TIMESTAMPTZ
) FROM PUBLIC;
REVOKE ALL ON FUNCTION farm.list_recent_activities_for_current_phone(TEXT, INTEGER) FROM PUBLIC;
REVOKE ALL ON FUNCTION farm.claim_activity_outbox(INTEGER) FROM PUBLIC;
REVOKE ALL ON FUNCTION farm.mark_activity_outbox_published(UUID) FROM PUBLIC;
REVOKE ALL ON FUNCTION farm.mark_activity_outbox_failed(UUID, TEXT) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION farm.record_activity_for_current_phone(
    UUID, TEXT, TEXT, TEXT, TEXT, TIMESTAMPTZ
) TO agrisense_farm;
GRANT EXECUTE ON FUNCTION farm.list_recent_activities_for_current_phone(TEXT, INTEGER) TO agrisense_farm;
GRANT EXECUTE ON FUNCTION farm.claim_activity_outbox(INTEGER) TO agrisense_farm;
GRANT EXECUTE ON FUNCTION farm.mark_activity_outbox_published(UUID) TO agrisense_farm;
GRANT EXECUTE ON FUNCTION farm.mark_activity_outbox_failed(UUID, TEXT) TO agrisense_farm;

COMMIT;
