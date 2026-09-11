-- ─── Farm Schema ───────────────────────────────────────────────────────────────
-- Domain: Farm operations, crops, activities, harvests
-- Owned by: farm-service

CREATE SCHEMA IF NOT EXISTS farm;
CREATE EXTENSION IF NOT EXISTS postgis;   -- for geo queries

CREATE TYPE farm.crop_type AS ENUM (
    'rice', 'corn', 'soybean', 'sugarcane', 'cassava',
    'tomato', 'chili', 'cabbage', 'shallot', 'melon', 'other'
);

CREATE TYPE farm.farm_size AS ENUM ('small', 'medium', 'large', 'enterprise');

CREATE TABLE farm.farmers (
    id          UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id     UUID NOT NULL,   -- references identity.users
    nik         VARCHAR(20),     -- Nomor Induk Kependudukan
    birth_date  DATE,
    address     TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE farm.farms (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    farmer_id       UUID NOT NULL REFERENCES farm.farmers(id),
    name            VARCHAR(255) NOT NULL,
    location        GEOGRAPHY(POINT, 4326),
    area_hectares   DECIMAL(10,4) NOT NULL,
    size_category   farm.farm_size,
    soil_type       VARCHAR(100),
    irrigation_type VARCHAR(100),
    notes           TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_farm_farms_farmer_id ON farm.farms (farmer_id);
CREATE INDEX idx_farm_farms_location  ON farm.farms USING GIST (location);

CREATE TABLE farm.crops (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    farm_id         UUID NOT NULL REFERENCES farm.farms(id),
    crop_type       farm.crop_type NOT NULL,
    seed_variety    VARCHAR(255),
    cultivation_system VARCHAR(50),
    cultivation_unit_count INTEGER CHECK (cultivation_unit_count IS NULL OR cultivation_unit_count > 0),
    area_per_unit_hectares DECIMAL(10,4) CHECK (area_per_unit_hectares IS NULL OR area_per_unit_hectares > 0),
    area_hectares   DECIMAL(10,4),
    planted_at      DATE NOT NULL,
    expected_harvest_at DATE,
    status          VARCHAR(50) DEFAULT 'growing',
    notes           TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_farm_crops_farm_id    ON farm.crops (farm_id);
CREATE INDEX idx_farm_crops_planted_at ON farm.crops (planted_at);

CREATE TABLE farm.activities (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    farm_id         UUID NOT NULL REFERENCES farm.farms(id),
    crop_id         UUID REFERENCES farm.crops(id),
    activity_type   VARCHAR(100) NOT NULL CHECK (activity_type IN (
                        'watering', 'fertilizing', 'spraying', 'pruning', 'inspection'
                    )),
    description     TEXT,
    quantity        DECIMAL(10,3),
    unit            VARCHAR(50),
    cost_idr        BIGINT,
    performed_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    source_request_id UUID,
    recorded_via    VARCHAR(30) NOT NULL DEFAULT 'manual'
                    CHECK (recorded_via IN ('manual', 'whatsapp', 'api')),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX uq_farm_activities_source_request
    ON farm.activities (source_request_id)
    WHERE source_request_id IS NOT NULL;
CREATE INDEX idx_farm_activities_owner_time
    ON farm.activities (farm_id, performed_at DESC);

CREATE TABLE farm.harvests (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    crop_id         UUID NOT NULL REFERENCES farm.crops(id),
    farm_id         UUID NOT NULL REFERENCES farm.farms(id),
    yield_kg        DECIMAL(12,2) NOT NULL,
    quality_grade   VARCHAR(10),
    price_per_kg    BIGINT,
    buyer           VARCHAR(255),
    harvested_at    TIMESTAMPTZ NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_farm_harvests_crop_id ON farm.harvests (crop_id);
CREATE INDEX idx_farm_harvests_farm_id ON farm.harvests (farm_id);

COMMENT ON SCHEMA farm IS 'Farm operations domain';
