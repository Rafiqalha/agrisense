-- WhatsApp onboarding plus tenant isolation.
-- Safe to run repeatedly by the local db-migrate container.

BEGIN;

-- Runtime services never connect as the schema owner. Passwords and LOGIN are
-- configured separately by configure-runtime-roles.psql.
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'agrisense_brain') THEN
        CREATE ROLE agrisense_brain NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'agrisense_gateway') THEN
        CREATE ROLE agrisense_gateway NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'agrisense_tenant') THEN
        CREATE ROLE agrisense_tenant NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'agrisense_farm') THEN
        CREATE ROLE agrisense_farm NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'agrisense_agronomy') THEN
        CREATE ROLE agrisense_agronomy NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'agrisense_finance') THEN
        CREATE ROLE agrisense_finance NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'agrisense_marketplace') THEN
        CREATE ROLE agrisense_marketplace NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'agrisense_analytics') THEN
        CREATE ROLE agrisense_analytics NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'agrisense_platform') THEN
        CREATE ROLE agrisense_platform NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'agrisense_ai') THEN
        CREATE ROLE agrisense_ai NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS;
    END IF;
END
$$;

-- Reassert security attributes on every run. Existing roles must not retain a
-- privileged flag or inherited membership from manual/local experiments.
ALTER ROLE agrisense_brain NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS;
ALTER ROLE agrisense_gateway NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS;
ALTER ROLE agrisense_tenant NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS;
ALTER ROLE agrisense_farm NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS;
ALTER ROLE agrisense_agronomy NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS;
ALTER ROLE agrisense_finance NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS;
ALTER ROLE agrisense_marketplace NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS;
ALTER ROLE agrisense_analytics NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS;
ALTER ROLE agrisense_platform NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS;
-- ai-service currently has no database repository and therefore receives no
-- credential at all. Keep the reserved role disabled until one is needed.
ALTER ROLE agrisense_ai NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS;

DO $$
DECLARE
    runtime_role TEXT;
    inherited_role TEXT;
BEGIN
    FOREACH runtime_role IN ARRAY ARRAY[
        'agrisense_brain', 'agrisense_gateway', 'agrisense_tenant',
        'agrisense_farm', 'agrisense_agronomy', 'agrisense_finance',
        'agrisense_marketplace', 'agrisense_analytics',
        'agrisense_platform', 'agrisense_ai'
    ]
    LOOP
        FOR inherited_role IN
            SELECT parent.rolname
            FROM pg_catalog.pg_auth_members membership
            JOIN pg_catalog.pg_roles parent ON parent.oid = membership.roleid
            JOIN pg_catalog.pg_roles member ON member.oid = membership.member
            WHERE member.rolname = runtime_role
        LOOP
            EXECUTE format('REVOKE %I FROM %I', inherited_role, runtime_role);
        END LOOP;
    END LOOP;
END
$$;

DO $$
BEGIN
    EXECUTE format(
        'GRANT CONNECT ON DATABASE %I TO agrisense_brain, agrisense_gateway',
        current_database()
    );
END
$$;
DO $$
BEGIN
    EXECUTE format(
        'GRANT CONNECT ON DATABASE %I TO agrisense_farm, agrisense_agronomy, agrisense_finance, agrisense_marketplace, agrisense_analytics, agrisense_platform',
        current_database()
    );
END
$$;
DO $$
BEGIN
    EXECUTE format(
        'REVOKE CONNECT ON DATABASE %I FROM agrisense_ai',
        current_database()
    );
END
$$;
GRANT USAGE ON SCHEMA identity, farm, ai TO agrisense_brain;
GRANT USAGE ON SCHEMA ai TO agrisense_gateway;
GRANT USAGE ON SCHEMA identity, farm, ai TO agrisense_tenant;

-- Placeholder domain services currently execute readiness checks only. They
-- intentionally receive CONNECT and no schema/table/function privileges until
-- a concrete repository/API operation defines the exact required grant.
REVOKE ALL ON SCHEMA identity, farm, agronomy, finance, ai, analytics FROM
    agrisense_farm, agrisense_agronomy, agrisense_finance,
    agrisense_marketplace, agrisense_analytics, agrisense_platform, agrisense_ai;

-- A farmer profile must belong to exactly one identity. This closes the old
-- schema gap that allowed duplicate/cross-linked profiles.
ALTER TABLE farm.farmers
    DROP CONSTRAINT IF EXISTS farmers_user_id_fkey;
ALTER TABLE farm.farmers
    ADD CONSTRAINT farmers_user_id_fkey
    FOREIGN KEY (user_id) REFERENCES identity.users(id) ON DELETE CASCADE;

CREATE UNIQUE INDEX IF NOT EXISTS uq_farm_farmers_user_id
    ON farm.farmers (user_id);

CREATE UNIQUE INDEX IF NOT EXISTS uq_ai_conversation_farmer_channel
    ON ai.conversations (farmer_id, channel);

CREATE TABLE IF NOT EXISTS ai.onboarding_sessions (
    phone_digest    BYTEA PRIMARY KEY,
    phone           VARCHAR(20) NOT NULL,
    current_step    VARCHAR(30) NOT NULL
                    CHECK (current_step IN (
                        'consent', 'name', 'farm_name', 'crop_name',
                        'variety', 'planted_at', 'area', 'confirm'
                    )),
    draft           JSONB NOT NULL DEFAULT '{}',
    status          VARCHAR(20) NOT NULL DEFAULT 'active'
                    CHECK (status IN ('active', 'completed', 'cancelled')),
    version         INTEGER NOT NULL DEFAULT 1,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at    TIMESTAMPTZ,
    CHECK (jsonb_typeof(draft) = 'object')
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_ai_onboarding_phone
    ON ai.onboarding_sessions (phone);
CREATE INDEX IF NOT EXISTS idx_ai_onboarding_active
    ON ai.onboarding_sessions (updated_at)
    WHERE status = 'active';

CREATE TABLE IF NOT EXISTS ai.onboarding_responses (
    request_id      UUID PRIMARY KEY,
    phone_digest    BYTEA NOT NULL,
    response        TEXT NOT NULL CHECK (length(response) BETWEEN 1 AND 4000),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_ai_onboarding_responses_owner
    ON ai.onboarding_responses (phone_digest, created_at DESC);

-- Added here as well as in the dedicated crop migration so upgrades from a
-- pre-melon database can rebuild the context RPCs safely in this transaction.
ALTER TABLE farm.crops
    ADD COLUMN IF NOT EXISTS cultivation_system VARCHAR(50),
    ADD COLUMN IF NOT EXISTS cultivation_unit_count INTEGER,
    ADD COLUMN IF NOT EXISTS area_per_unit_hectares DECIMAL(10,4);

CREATE OR REPLACE FUNCTION identity.normalize_whatsapp_phone(p_phone TEXT)
RETURNS TEXT
LANGUAGE plpgsql
IMMUTABLE
STRICT
SET search_path = pg_catalog
AS $$
DECLARE
    digits TEXT;
BEGIN
    digits := regexp_replace(p_phone, '[^0-9]', '', 'g');
    IF length(digits) < 8 OR length(digits) > 15 THEN
        RAISE EXCEPTION 'invalid WhatsApp phone number' USING ERRCODE = '22023';
    END IF;
    RETURN '+' || digits;
END
$$;

CREATE OR REPLACE FUNCTION identity.current_user_id()
RETURNS UUID
LANGUAGE plpgsql
STABLE
SET search_path = pg_catalog
AS $$
DECLARE
    raw_scope TEXT := NULLIF(btrim(current_setting('app.current_user_id', TRUE)), '');
BEGIN
    IF raw_scope IS NULL THEN
        RAISE EXCEPTION 'tenant user scope is required' USING ERRCODE = '42501';
    END IF;

    BEGIN
        RETURN raw_scope::UUID;
    EXCEPTION WHEN invalid_text_representation THEN
        RAISE EXCEPTION 'tenant user scope is invalid' USING ERRCODE = '22023';
    END;
END
$$;

CREATE OR REPLACE FUNCTION identity.current_phone_digest()
RETURNS BYTEA
LANGUAGE plpgsql
STABLE
SET search_path = pg_catalog
AS $$
DECLARE
    raw_scope TEXT := NULLIF(btrim(current_setting('app.current_phone', TRUE)), '');
BEGIN
    IF raw_scope IS NULL THEN
        RAISE EXCEPTION 'tenant phone scope is required' USING ERRCODE = '42501';
    END IF;

    RETURN public.digest(identity.normalize_whatsapp_phone(raw_scope), 'sha256');
END
$$;

-- Defense in depth for any future tenant-facing SQL path. FORCE makes policies
-- apply to table owners too; runtime roles also have BYPASSRLS disabled.
ALTER TABLE identity.users ENABLE ROW LEVEL SECURITY;
ALTER TABLE identity.users FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_identity_users ON identity.users;
CREATE POLICY tenant_identity_users ON identity.users TO agrisense_tenant
    USING (id = identity.current_user_id())
    WITH CHECK (id = identity.current_user_id());

ALTER TABLE farm.farmers ENABLE ROW LEVEL SECURITY;
ALTER TABLE farm.farmers FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_farmers ON farm.farmers;
CREATE POLICY tenant_farmers ON farm.farmers TO agrisense_tenant
    USING (user_id = identity.current_user_id())
    WITH CHECK (user_id = identity.current_user_id());

ALTER TABLE farm.farms ENABLE ROW LEVEL SECURITY;
ALTER TABLE farm.farms FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_farms ON farm.farms;
CREATE POLICY tenant_farms ON farm.farms TO agrisense_tenant
    USING (EXISTS (
        SELECT 1 FROM farm.farmers fr
        WHERE fr.id = farmer_id AND fr.user_id = identity.current_user_id()
    ))
    WITH CHECK (EXISTS (
        SELECT 1 FROM farm.farmers fr
        WHERE fr.id = farmer_id AND fr.user_id = identity.current_user_id()
    ));

ALTER TABLE farm.crops ENABLE ROW LEVEL SECURITY;
ALTER TABLE farm.crops FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_crops ON farm.crops;
CREATE POLICY tenant_crops ON farm.crops TO agrisense_tenant
    USING (EXISTS (
        SELECT 1 FROM farm.farms f JOIN farm.farmers fr ON fr.id = f.farmer_id
        WHERE f.id = farm_id AND fr.user_id = identity.current_user_id()
    ))
    WITH CHECK (EXISTS (
        SELECT 1 FROM farm.farms f JOIN farm.farmers fr ON fr.id = f.farmer_id
        WHERE f.id = farm_id AND fr.user_id = identity.current_user_id()
    ));

ALTER TABLE farm.activities ENABLE ROW LEVEL SECURITY;
ALTER TABLE farm.activities FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_activities ON farm.activities;
CREATE POLICY tenant_activities ON farm.activities TO agrisense_tenant
    USING (EXISTS (
        SELECT 1 FROM farm.farms f JOIN farm.farmers fr ON fr.id = f.farmer_id
        WHERE f.id = farm_id AND fr.user_id = identity.current_user_id()
    ))
    WITH CHECK (EXISTS (
        SELECT 1 FROM farm.farms f JOIN farm.farmers fr ON fr.id = f.farmer_id
        WHERE f.id = farm_id AND fr.user_id = identity.current_user_id()
    ));

ALTER TABLE farm.harvests ENABLE ROW LEVEL SECURITY;
ALTER TABLE farm.harvests FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_harvests ON farm.harvests;
CREATE POLICY tenant_harvests ON farm.harvests TO agrisense_tenant
    USING (EXISTS (
        SELECT 1 FROM farm.farms f JOIN farm.farmers fr ON fr.id = f.farmer_id
        WHERE f.id = farm_id AND fr.user_id = identity.current_user_id()
    ))
    WITH CHECK (EXISTS (
        SELECT 1 FROM farm.farms f JOIN farm.farmers fr ON fr.id = f.farmer_id
        WHERE f.id = farm_id AND fr.user_id = identity.current_user_id()
    ));

ALTER TABLE ai.conversations ENABLE ROW LEVEL SECURITY;
ALTER TABLE ai.conversations FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_conversations ON ai.conversations;
CREATE POLICY tenant_conversations ON ai.conversations TO agrisense_brain, agrisense_tenant
    USING (farmer_id = identity.current_user_id())
    WITH CHECK (farmer_id = identity.current_user_id());

ALTER TABLE ai.messages ENABLE ROW LEVEL SECURITY;
ALTER TABLE ai.messages FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_messages ON ai.messages;
CREATE POLICY tenant_messages ON ai.messages TO agrisense_brain, agrisense_tenant
    USING (farmer_id = identity.current_user_id())
    WITH CHECK (
        farmer_id = identity.current_user_id()
        AND EXISTS (
            SELECT 1 FROM ai.conversations c
            WHERE c.id = conversation_id AND c.farmer_id = identity.current_user_id()
        )
    );

ALTER TABLE ai.agent_runs ENABLE ROW LEVEL SECURITY;
ALTER TABLE ai.agent_runs FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_agent_runs ON ai.agent_runs;
CREATE POLICY tenant_agent_runs ON ai.agent_runs TO agrisense_brain, agrisense_tenant
    USING (farmer_id = identity.current_user_id())
    WITH CHECK (farmer_id = identity.current_user_id());

ALTER TABLE ai.workflow_runs ENABLE ROW LEVEL SECURITY;
ALTER TABLE ai.workflow_runs FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_workflow_runs ON ai.workflow_runs;
CREATE POLICY tenant_workflow_runs ON ai.workflow_runs TO agrisense_brain, agrisense_tenant
    USING (farmer_id = identity.current_user_id())
    WITH CHECK (farmer_id = identity.current_user_id());

ALTER TABLE ai.onboarding_sessions ENABLE ROW LEVEL SECURITY;
ALTER TABLE ai.onboarding_sessions FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_onboarding_sessions ON ai.onboarding_sessions;
CREATE POLICY tenant_onboarding_sessions ON ai.onboarding_sessions TO agrisense_tenant
    USING (phone_digest = identity.current_phone_digest())
    WITH CHECK (phone_digest = identity.current_phone_digest());

ALTER TABLE ai.onboarding_responses ENABLE ROW LEVEL SECURITY;
ALTER TABLE ai.onboarding_responses FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_onboarding_responses ON ai.onboarding_responses;
CREATE POLICY tenant_onboarding_responses ON ai.onboarding_responses TO agrisense_tenant
    USING (phone_digest = identity.current_phone_digest())
    WITH CHECK (phone_digest = identity.current_phone_digest());

-- Narrow context function: callers can retrieve only the row belonging to the
-- supplied, normalized WhatsApp identity. No general table SELECT is granted.
DROP FUNCTION IF EXISTS identity.user_context(UUID);
DROP FUNCTION IF EXISTS identity.whatsapp_farmer_context(TEXT);
DROP FUNCTION IF EXISTS identity.user_context();
DROP FUNCTION IF EXISTS identity.whatsapp_farmer_context();
CREATE OR REPLACE FUNCTION identity.whatsapp_farmer_context()
RETURNS TABLE (
    user_id UUID,
    phone TEXT,
    farmer_name TEXT,
    farm_name TEXT,
    crop_type TEXT,
    seed_variety TEXT,
    planted_at DATE,
    crop_status TEXT,
    area_hectares TEXT,
    cultivation_system TEXT,
    cultivation_unit_count INTEGER,
    area_per_unit_hectares TEXT,
    expected_harvest_at DATE
)
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog
AS $$
    SELECT u.id,
           u.phone::TEXT,
           u.name::TEXT,
           COALESCE(cr.farm_name, fm.name)::TEXT,
           cr.crop_type::TEXT,
           cr.seed_variety::TEXT,
           cr.planted_at,
           cr.status::TEXT,
           cr.area_hectares::TEXT,
           cr.cultivation_system::TEXT,
           cr.cultivation_unit_count,
           cr.area_per_unit_hectares::TEXT,
           cr.expected_harvest_at
    FROM identity.users u
    LEFT JOIN LATERAL (
        SELECT f.id, f.name
        FROM farm.farmers fr
        JOIN farm.farms f ON f.farmer_id = fr.id
        WHERE fr.user_id = u.id
        ORDER BY f.created_at DESC, f.id
        LIMIT 1
    ) fm ON TRUE
    LEFT JOIN LATERAL (
        SELECT f.name AS farm_name, c.crop_type, c.seed_variety, c.planted_at, c.status,
               c.area_hectares, c.cultivation_system,
               c.cultivation_unit_count, c.area_per_unit_hectares,
               c.expected_harvest_at
        FROM farm.farmers fr
        JOIN farm.farms f ON f.farmer_id = fr.id
        JOIN farm.crops c ON c.farm_id = f.id
        WHERE fr.user_id = u.id AND c.status = 'growing'
        ORDER BY c.planted_at DESC, c.created_at DESC, c.id
        LIMIT 1
    ) cr ON TRUE
    WHERE u.phone = identity.normalize_whatsapp_phone(
        current_setting('app.current_phone', FALSE)
    )
    LIMIT 1
$$;

CREATE OR REPLACE FUNCTION identity.user_context()
RETURNS TABLE (
    user_id UUID,
    phone TEXT,
    farmer_name TEXT,
    farm_name TEXT,
    crop_type TEXT,
    seed_variety TEXT,
    planted_at DATE,
    crop_status TEXT,
    area_hectares TEXT,
    cultivation_system TEXT,
    cultivation_unit_count INTEGER,
    area_per_unit_hectares TEXT,
    expected_harvest_at DATE
)
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog
AS $$
    SELECT c.*
    FROM identity.users u
    CROSS JOIN LATERAL (
        SELECT u.id AS user_id,
               u.phone::TEXT AS phone,
               u.name::TEXT AS farmer_name,
               COALESCE(cr.farm_name, fm.name)::TEXT AS farm_name,
               cr.crop_type::TEXT AS crop_type,
               cr.seed_variety::TEXT AS seed_variety,
               cr.planted_at,
               cr.status::TEXT AS crop_status,
               cr.area_hectares::TEXT AS area_hectares,
               cr.cultivation_system::TEXT AS cultivation_system,
               cr.cultivation_unit_count,
               cr.area_per_unit_hectares::TEXT AS area_per_unit_hectares,
               cr.expected_harvest_at
        FROM (SELECT 1) anchor
        LEFT JOIN LATERAL (
            SELECT f.id, f.name
            FROM farm.farmers fr JOIN farm.farms f ON f.farmer_id = fr.id
            WHERE fr.user_id = u.id
            ORDER BY f.created_at DESC, f.id LIMIT 1
        ) fm ON TRUE
        LEFT JOIN LATERAL (
            SELECT f.name AS farm_name, c.crop_type, c.seed_variety, c.planted_at,
                   c.status, c.area_hectares, c.cultivation_system,
                   c.cultivation_unit_count, c.area_per_unit_hectares,
                   c.expected_harvest_at
            FROM farm.farmers fr
            JOIN farm.farms f ON f.farmer_id = fr.id
            JOIN farm.crops c ON c.farm_id = f.id
            WHERE fr.user_id = u.id AND c.status = 'growing'
            ORDER BY c.planted_at DESC, c.created_at DESC, c.id LIMIT 1
        ) cr ON TRUE
    ) c
    WHERE u.id = identity.current_user_id()
$$;

DROP FUNCTION IF EXISTS ai.get_onboarding_session(TEXT);
CREATE OR REPLACE FUNCTION ai.get_onboarding_session()
RETURNS TABLE (current_step TEXT, draft JSONB, status TEXT, version INTEGER)
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog
AS $$
    SELECT s.current_step::TEXT, s.draft, s.status::TEXT, s.version
    FROM ai.onboarding_sessions s
    WHERE s.phone_digest = identity.current_phone_digest()
      AND s.status = 'active'
$$;

DROP FUNCTION IF EXISTS ai.get_onboarding_response(TEXT, UUID);
CREATE OR REPLACE FUNCTION ai.get_onboarding_response(p_request_id UUID)
RETURNS TEXT
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog
AS $$
    SELECT r.response
    FROM ai.onboarding_responses r
    WHERE r.request_id = p_request_id
      AND r.phone_digest = identity.current_phone_digest()
$$;

DROP FUNCTION IF EXISTS ai.save_onboarding_session(TEXT, TEXT, JSONB, UUID, TEXT);
CREATE OR REPLACE FUNCTION ai.save_onboarding_session(
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
    normalized TEXT := identity.normalize_whatsapp_phone(
        current_setting('app.current_phone', FALSE)
    );
BEGIN
    IF p_step NOT IN ('consent', 'name', 'farm_name', 'crop_name', 'variety', 'planted_at', 'area', 'confirm') THEN
        RAISE EXCEPTION 'invalid onboarding step' USING ERRCODE = '22023';
    END IF;
    IF jsonb_typeof(p_draft) <> 'object' OR pg_column_size(p_draft) > 8192 THEN
        RAISE EXCEPTION 'invalid onboarding draft' USING ERRCODE = '22023';
    END IF;
    IF length(p_response) NOT BETWEEN 1 AND 4000 THEN
        RAISE EXCEPTION 'invalid onboarding response' USING ERRCODE = '22023';
    END IF;

    INSERT INTO ai.onboarding_sessions (phone_digest, phone, current_step, draft, status)
    VALUES (public.digest(normalized, 'sha256'), normalized, p_step, p_draft, 'active')
    ON CONFLICT (phone_digest) DO UPDATE
    SET current_step = EXCLUDED.current_step,
        draft = EXCLUDED.draft,
        status = 'active',
        version = ai.onboarding_sessions.version + 1,
        updated_at = NOW(),
        completed_at = NULL;

    INSERT INTO ai.onboarding_responses (request_id, phone_digest, response)
    VALUES (p_request_id, public.digest(normalized, 'sha256'), p_response)
    ON CONFLICT (request_id) DO NOTHING;
END
$$;

DROP FUNCTION IF EXISTS ai.cancel_onboarding_session(TEXT, UUID, TEXT);
CREATE OR REPLACE FUNCTION ai.cancel_onboarding_session(
    p_request_id UUID,
    p_response TEXT
)
RETURNS VOID
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $$
DECLARE
    normalized TEXT := identity.normalize_whatsapp_phone(
        current_setting('app.current_phone', FALSE)
    );
BEGIN
    UPDATE ai.onboarding_sessions
    SET status = 'cancelled', updated_at = NOW()
    WHERE phone_digest = public.digest(normalized, 'sha256') AND status = 'active';

    INSERT INTO ai.onboarding_responses (request_id, phone_digest, response)
    VALUES (p_request_id, public.digest(normalized, 'sha256'), p_response)
    ON CONFLICT (request_id) DO NOTHING;
END
$$;

DROP FUNCTION IF EXISTS ai.complete_whatsapp_onboarding(TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, DATE, UUID, TEXT);
CREATE OR REPLACE FUNCTION ai.complete_whatsapp_onboarding(
    p_name TEXT,
    p_farm_name TEXT,
    p_crop_type TEXT,
    p_seed_variety TEXT,
    p_area_hectares TEXT,
    p_planted_at DATE,
    p_request_id UUID,
    p_response TEXT
)
RETURNS TABLE (
    created_user_id UUID,
    created_farmer_id UUID,
    created_farm_id UUID,
    created_crop_id UUID
)
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $$
DECLARE
    normalized TEXT := identity.normalize_whatsapp_phone(
        current_setting('app.current_phone', FALSE)
    );
    v_user_id UUID;
    v_farmer_id UUID;
    v_farm_id UUID;
    v_crop_id UUID;
    v_area_hectares NUMERIC;
BEGIN
    BEGIN
        v_area_hectares := p_area_hectares::NUMERIC;
    EXCEPTION WHEN invalid_text_representation THEN
        RAISE EXCEPTION 'invalid crop area' USING ERRCODE = '22023';
    END;
    IF length(btrim(p_name)) NOT BETWEEN 2 AND 100
       OR length(btrim(p_farm_name)) NOT BETWEEN 2 AND 120 THEN
        RAISE EXCEPTION 'invalid onboarding name' USING ERRCODE = '22023';
    END IF;
    IF p_crop_type NOT IN ('rice','corn','soybean','sugarcane','cassava','tomato','chili','cabbage','shallot','melon','other') THEN
        RAISE EXCEPTION 'invalid crop type' USING ERRCODE = '22023';
    END IF;
    IF v_area_hectares <= 0 OR v_area_hectares > 100000 OR p_planted_at > CURRENT_DATE THEN
        RAISE EXCEPTION 'invalid crop area or planting date' USING ERRCODE = '22023';
    END IF;

    -- Serializes duplicate confirmation/retry requests for one WhatsApp owner.
    PERFORM pg_advisory_xact_lock(hashtextextended(normalized, 0));

    INSERT INTO identity.users (phone, name, role, status, last_active_at)
    VALUES (normalized, btrim(p_name), 'farmer', 'active', NOW())
    ON CONFLICT (phone) DO UPDATE
    SET name = EXCLUDED.name, last_active_at = NOW(), updated_at = NOW()
    RETURNING id INTO v_user_id;

    INSERT INTO farm.farmers (user_id)
    VALUES (v_user_id)
    ON CONFLICT (user_id) DO UPDATE SET updated_at = NOW()
    RETURNING id INTO v_farmer_id;

    SELECT f.id INTO v_farm_id
    FROM farm.farms f
    WHERE f.farmer_id = v_farmer_id
    ORDER BY f.created_at DESC, f.id
    LIMIT 1;

    IF v_farm_id IS NULL THEN
        INSERT INTO farm.farms (farmer_id, name, area_hectares, size_category)
        VALUES (
            v_farmer_id,
            btrim(p_farm_name),
            v_area_hectares,
            CASE
                WHEN v_area_hectares < 1 THEN 'small'::farm.farm_size
                WHEN v_area_hectares < 5 THEN 'medium'::farm.farm_size
                WHEN v_area_hectares < 20 THEN 'large'::farm.farm_size
                ELSE 'enterprise'::farm.farm_size
            END
        )
        RETURNING id INTO v_farm_id;
    ELSE
        UPDATE farm.farms
        SET name = btrim(p_farm_name), area_hectares = v_area_hectares, updated_at = NOW()
        WHERE id = v_farm_id;
    END IF;

    INSERT INTO farm.crops (
        farm_id, crop_type, seed_variety, area_hectares, planted_at, status, notes
    ) VALUES (
        v_farm_id,
        p_crop_type::farm.crop_type,
        NULLIF(btrim(p_seed_variety), ''),
        v_area_hectares,
        p_planted_at,
        'growing',
        'Didaftarkan melalui onboarding WhatsApp dengan konfirmasi pengguna'
    ) RETURNING id INTO v_crop_id;

    UPDATE ai.onboarding_sessions
    SET status = 'completed', completed_at = NOW(), updated_at = NOW()
    WHERE phone_digest = public.digest(normalized, 'sha256') AND status = 'active';

    INSERT INTO ai.onboarding_responses (request_id, phone_digest, response)
    VALUES (p_request_id, public.digest(normalized, 'sha256'), p_response)
    ON CONFLICT (request_id) DO NOTHING;

    RETURN QUERY SELECT v_user_id, v_farmer_id, v_farm_id, v_crop_id;
END
$$;

-- Gateway can only operate its durable delivery queue.
REVOKE ALL ON ALL TABLES IN SCHEMA ai FROM agrisense_gateway;
GRANT SELECT, INSERT, UPDATE ON ai.inbound_messages TO agrisense_gateway;

-- A future authenticated API may SET ROLE agrisense_tenant and SET LOCAL
-- app.current_user_id. These grants are broad only in operation type; FORCE
-- RLS still narrows every row to the authenticated owner.
GRANT SELECT, INSERT, UPDATE, DELETE ON identity.users TO agrisense_tenant;
GRANT SELECT, INSERT, UPDATE, DELETE ON
    farm.farmers, farm.farms, farm.crops, farm.activities, farm.harvests
    TO agrisense_tenant;
GRANT SELECT, INSERT, UPDATE, DELETE ON
    ai.conversations, ai.messages, ai.agent_runs, ai.workflow_runs,
    ai.onboarding_sessions, ai.onboarding_responses
    TO agrisense_tenant;

-- Brain gets RLS-protected audit writes plus tightly scoped SECURITY DEFINER RPCs.
REVOKE ALL ON ALL TABLES IN SCHEMA identity FROM agrisense_brain;
REVOKE ALL ON ALL TABLES IN SCHEMA farm FROM agrisense_brain;
REVOKE ALL ON ALL TABLES IN SCHEMA ai FROM agrisense_brain;
GRANT SELECT, INSERT, UPDATE ON ai.conversations TO agrisense_brain;
GRANT INSERT ON ai.agent_runs TO agrisense_brain;

REVOKE ALL ON FUNCTION identity.normalize_whatsapp_phone(TEXT) FROM PUBLIC;
REVOKE ALL ON FUNCTION identity.current_user_id() FROM PUBLIC;
REVOKE ALL ON FUNCTION identity.current_phone_digest() FROM PUBLIC;
REVOKE ALL ON FUNCTION identity.whatsapp_farmer_context() FROM PUBLIC;
REVOKE ALL ON FUNCTION identity.user_context() FROM PUBLIC;
REVOKE ALL ON FUNCTION ai.get_onboarding_session() FROM PUBLIC;
REVOKE ALL ON FUNCTION ai.get_onboarding_response(UUID) FROM PUBLIC;
REVOKE ALL ON FUNCTION ai.save_onboarding_session(TEXT, JSONB, UUID, TEXT) FROM PUBLIC;
REVOKE ALL ON FUNCTION ai.cancel_onboarding_session(UUID, TEXT) FROM PUBLIC;
REVOKE ALL ON FUNCTION ai.complete_whatsapp_onboarding(TEXT, TEXT, TEXT, TEXT, TEXT, DATE, UUID, TEXT) FROM PUBLIC;

GRANT EXECUTE ON FUNCTION identity.normalize_whatsapp_phone(TEXT) TO agrisense_brain;
GRANT EXECUTE ON FUNCTION identity.current_user_id() TO agrisense_brain, agrisense_tenant;
GRANT EXECUTE ON FUNCTION identity.current_phone_digest() TO agrisense_brain, agrisense_tenant;
GRANT EXECUTE ON FUNCTION identity.whatsapp_farmer_context() TO agrisense_brain;
GRANT EXECUTE ON FUNCTION identity.user_context() TO agrisense_brain;
GRANT EXECUTE ON FUNCTION ai.get_onboarding_session() TO agrisense_brain;
GRANT EXECUTE ON FUNCTION ai.get_onboarding_response(UUID) TO agrisense_brain;
GRANT EXECUTE ON FUNCTION ai.save_onboarding_session(TEXT, JSONB, UUID, TEXT) TO agrisense_brain;
GRANT EXECUTE ON FUNCTION ai.cancel_onboarding_session(UUID, TEXT) TO agrisense_brain;
GRANT EXECUTE ON FUNCTION ai.complete_whatsapp_onboarding(TEXT, TEXT, TEXT, TEXT, TEXT, DATE, UUID, TEXT) TO agrisense_brain;

COMMIT;
