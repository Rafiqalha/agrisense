-- ─── Agronomy Schema ──────────────────────────────────────────────────────────
-- Domain: Diseases, pests, nutrients, recommendations, knowledge base
-- Owned by: agronomy-service
-- NOTE: This is the intellectual asset of AgriSense

CREATE SCHEMA IF NOT EXISTS agronomy;
CREATE EXTENSION IF NOT EXISTS vector;  -- pgvector for RAG

CREATE TYPE agronomy.severity_level AS ENUM ('low', 'medium', 'high', 'critical');
CREATE TYPE agronomy.detection_method AS ENUM ('vision_ai', 'text_description', 'manual_report');

-- Disease knowledge base
CREATE TABLE agronomy.diseases (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name            VARCHAR(255) NOT NULL,
    local_names     JSONB,          -- {"id": "hawar daun", "jv": "..."}
    crop_types      JSONB,          -- ["rice", "corn"]
    symptoms        TEXT NOT NULL,
    treatment       TEXT NOT NULL,
    prevention      TEXT,
    severity_risk   agronomy.severity_level NOT NULL DEFAULT 'medium',
    image_urls      JSONB,
    embedding       vector(768),    -- for RAG similarity search
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_agronomy_diseases_name      ON agronomy.diseases (name);
CREATE INDEX idx_agronomy_diseases_embedding ON agronomy.diseases USING ivfflat (embedding vector_cosine_ops);

-- Disease detections per farmer
CREATE TABLE agronomy.detections (
    id                  UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    disease_id          UUID REFERENCES agronomy.diseases(id),
    farm_id             UUID NOT NULL,   -- references farm.farms
    farmer_id           UUID NOT NULL,   -- references farm.farmers
    crop_id             UUID,            -- references farm.crops
    severity            agronomy.severity_level NOT NULL,
    confidence_score    FLOAT,
    detection_method    agronomy.detection_method NOT NULL,
    image_url           TEXT,
    raw_description     TEXT,
    ai_analysis         JSONB,
    detected_at         TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_agronomy_detections_farmer_id ON agronomy.detections (farmer_id);
CREATE INDEX idx_agronomy_detections_detected_at ON agronomy.detections (detected_at);

-- Recommendations given to farmers
CREATE TABLE agronomy.recommendations (
    id                  UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    detection_id        UUID REFERENCES agronomy.detections(id),
    farmer_id           UUID NOT NULL,
    farm_id             UUID NOT NULL,
    trigger             VARCHAR(100),
    products            JSONB NOT NULL,
    dosage_instructions TEXT NOT NULL,
    application_method  TEXT,
    follow_up_days      INTEGER,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Pests knowledge base
CREATE TABLE agronomy.pests (
    id          UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name        VARCHAR(255) NOT NULL,
    local_names JSONB,
    crop_types  JSONB,
    treatment   TEXT,
    embedding   vector(768),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

COMMENT ON SCHEMA agronomy IS 'Agronomy knowledge domain - primary intellectual asset';
COMMENT ON TABLE agronomy.diseases IS 'Disease knowledge base with pgvector embeddings for RAG';
