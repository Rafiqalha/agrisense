-- Melon is a first-class crop, not free-text metadata under `other`.
-- PostgreSQL requires a new enum value to be committed before it is used, so
-- this intentionally runs outside the transaction in the preceding migration.
ALTER TYPE farm.crop_type ADD VALUE IF NOT EXISTS 'melon' BEFORE 'other';

BEGIN;

ALTER TABLE farm.crops
    ADD COLUMN IF NOT EXISTS cultivation_system VARCHAR(50),
    ADD COLUMN IF NOT EXISTS cultivation_unit_count INTEGER,
    ADD COLUMN IF NOT EXISTS area_per_unit_hectares DECIMAL(10,4);

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'crops_cultivation_unit_count_check'
          AND conrelid = 'farm.crops'::regclass
    ) THEN
        ALTER TABLE farm.crops
            ADD CONSTRAINT crops_cultivation_unit_count_check
            CHECK (cultivation_unit_count IS NULL OR cultivation_unit_count > 0);
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'crops_area_per_unit_hectares_check'
          AND conrelid = 'farm.crops'::regclass
    ) THEN
        ALTER TABLE farm.crops
            ADD CONSTRAINT crops_area_per_unit_hectares_check
            CHECK (area_per_unit_hectares IS NULL OR area_per_unit_hectares > 0);
    END IF;
END
$$;

-- Replace the context RPCs so structured cultivation data reaches the AI
-- alongside the authoritative total area.
DROP FUNCTION IF EXISTS identity.whatsapp_farmer_context();
CREATE FUNCTION identity.whatsapp_farmer_context()
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
        SELECT f.name AS farm_name, c.crop_type, c.seed_variety, c.planted_at,
               c.status, c.area_hectares, c.cultivation_system,
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

DROP FUNCTION IF EXISTS identity.user_context();
CREATE FUNCTION identity.user_context()
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
            SELECT f.name AS farm_name, c.crop_type, c.seed_variety,
                   c.planted_at, c.status, c.area_hectares,
                   c.cultivation_system, c.cultivation_unit_count,
                   c.area_per_unit_hectares, c.expected_harvest_at
            FROM farm.farmers fr
            JOIN farm.farms f ON f.farmer_id = fr.id
            JOIN farm.crops c ON c.farm_id = f.id
            WHERE fr.user_id = u.id AND c.status = 'growing'
            ORDER BY c.planted_at DESC, c.created_at DESC, c.id LIMIT 1
        ) cr ON TRUE
    ) c
    WHERE u.id = identity.current_user_id()
$$;

REVOKE ALL ON FUNCTION identity.whatsapp_farmer_context() FROM PUBLIC;
REVOKE ALL ON FUNCTION identity.user_context() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION identity.whatsapp_farmer_context() TO agrisense_brain;
GRANT EXECUTE ON FUNCTION identity.user_context() TO agrisense_brain;

DROP FUNCTION IF EXISTS ai.complete_whatsapp_onboarding(
    TEXT, TEXT, TEXT, TEXT, TEXT, DATE, UUID, TEXT, TEXT, BIGINT, TEXT
);
CREATE FUNCTION ai.complete_whatsapp_onboarding(
    p_name TEXT,
    p_farm_name TEXT,
    p_crop_type TEXT,
    p_seed_variety TEXT,
    p_area_hectares TEXT,
    p_planted_at DATE,
    p_request_id UUID,
    p_response TEXT,
    p_cultivation_system TEXT,
    p_cultivation_unit_count BIGINT,
    p_area_per_unit_hectares TEXT
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
    v_user_id UUID;
    v_farmer_id UUID;
    v_farm_id UUID;
    v_crop_id UUID;
    v_area_per_unit NUMERIC;
BEGIN
    IF p_cultivation_system IS NOT NULL
       AND p_cultivation_system NOT IN ('greenhouse', 'screen_house', 'hydroponic', 'open_field', 'other') THEN
        RAISE EXCEPTION 'invalid cultivation system' USING ERRCODE = '22023';
    END IF;
    IF p_cultivation_unit_count IS NOT NULL
       AND (p_cultivation_unit_count <= 0 OR p_cultivation_unit_count > 10000) THEN
        RAISE EXCEPTION 'invalid cultivation unit count' USING ERRCODE = '22023';
    END IF;
    IF p_area_per_unit_hectares IS NOT NULL THEN
        BEGIN
            v_area_per_unit := p_area_per_unit_hectares::NUMERIC;
        EXCEPTION WHEN invalid_text_representation THEN
            RAISE EXCEPTION 'invalid area per cultivation unit' USING ERRCODE = '22023';
        END;
        IF v_area_per_unit <= 0 OR p_cultivation_unit_count IS NULL THEN
            RAISE EXCEPTION 'area per unit requires a valid unit count' USING ERRCODE = '22023';
        END IF;
    END IF;

    SELECT result.created_user_id,
           result.created_farmer_id,
           result.created_farm_id,
           result.created_crop_id
    INTO v_user_id, v_farmer_id, v_farm_id, v_crop_id
    FROM ai.complete_whatsapp_onboarding(
        p_name, p_farm_name, p_crop_type, p_seed_variety, p_area_hectares,
        p_planted_at, p_request_id, p_response
    ) result;

    UPDATE farm.crops
    SET cultivation_system = p_cultivation_system,
        cultivation_unit_count = p_cultivation_unit_count::INTEGER,
        area_per_unit_hectares = v_area_per_unit
    WHERE id = v_crop_id;

    RETURN QUERY SELECT v_user_id, v_farmer_id, v_farm_id, v_crop_id;
END
$$;

-- Brain must use the structured form; the legacy helper remains private for
-- the wrapper above so callers cannot omit cultivation metadata accidentally.
REVOKE ALL ON FUNCTION ai.complete_whatsapp_onboarding(
    TEXT, TEXT, TEXT, TEXT, TEXT, DATE, UUID, TEXT
) FROM PUBLIC, agrisense_brain;
REVOKE ALL ON FUNCTION ai.complete_whatsapp_onboarding(
    TEXT, TEXT, TEXT, TEXT, TEXT, DATE, UUID, TEXT, TEXT, BIGINT, TEXT
) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION ai.complete_whatsapp_onboarding(
    TEXT, TEXT, TEXT, TEXT, TEXT, DATE, UUID, TEXT, TEXT, BIGINT, TEXT
) TO agrisense_brain;

COMMIT;
