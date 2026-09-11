-- ─── Demo Seed Data ───────────────────────────────────────────────────────────
-- One farmer, one farm, one melon crop planted 32 days ago.
-- Enables the Phase 1 vertical slice demo:
--
--   "Berapa umur tanaman melon saya?"
--     → intent: CHECK_HARVEST_STATUS
--     → tool:   farm.get_current_crop
--     → result: melon, 32 days
--     → reply:  "Tanaman melon Anda saat ini berumur 32 hari."
--
-- Idempotent: safe to re-run (ON CONFLICT DO NOTHING / WHERE NOT EXISTS).
-- Phone: +6281234567890 (use this as the WhatsApp "from" in webhook payloads).

-- 1. Identity user (platform domain)
INSERT INTO identity.users (id, phone, name, role, status, region)
VALUES (
    '11111111-1111-1111-1111-111111111111',
    '+6281234567890',
    'Pak Tani',
    'farmer',
    'active',
    'Klaten, Jawa Tengah'
)
ON CONFLICT (phone) DO NOTHING;

-- 2. Farmer profile (farm domain)
INSERT INTO farm.farmers (id, user_id, nik, address)
SELECT
    '22222222-2222-2222-2222-222222222222',
    '11111111-1111-1111-1111-111111111111',
    '3301234567890001',
    'Desa Demo, Klaten'
WHERE NOT EXISTS (
    SELECT 1 FROM farm.farmers WHERE id = '22222222-2222-2222-2222-222222222222'
);

-- 3. Farm (farm domain)
INSERT INTO farm.farms (id, farmer_id, name, area_hectares, size_category, soil_type)
SELECT
    '33333333-3333-3333-3333-333333333333',
    '22222222-2222-2222-2222-222222222222',
    'Kebun Melon Pak Tani',
    0.0500,
    'small',
    'Latosol'
WHERE NOT EXISTS (
    SELECT 1 FROM farm.farms WHERE id = '33333333-3333-3333-3333-333333333333'
);

-- 4. Melon crop, planted 32 days ago.
--    planted_at is relative so age_days is always 32 regardless of when seeded.
INSERT INTO farm.crops (id, farm_id, crop_type, seed_variety, area_hectares, planted_at, expected_harvest_at, status, notes)
SELECT
    '44444444-4444-4444-4444-444444444444',
    '33333333-3333-3333-3333-333333333333',
    'melon',
    'Golden Langkawi',
    0.0500,
    (NOW() - INTERVAL '32 days')::DATE,
    (NOW() + INTERVAL '38 days')::DATE,
    'growing',
    'Demo crop untuk vertical slice Phase 1'
WHERE NOT EXISTS (
    SELECT 1 FROM farm.crops WHERE id = '44444444-4444-4444-4444-444444444444'
);

COMMENT ON TABLE identity.users IS 'Demo user seeded: +6281234567890 (Pak Tani)';
