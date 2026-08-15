-- ─── Analytics Schema ─────────────────────────────────────────────────────────
-- Domain: Aggregated insights, trends, regional data
-- Owned by: analytics-service
-- Future B2B: Sold to distributors, government, banks

CREATE SCHEMA IF NOT EXISTS analytics;

-- Aggregated disease trends by region
CREATE TABLE analytics.disease_trends (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    region          VARCHAR(100) NOT NULL,
    disease_name    VARCHAR(255) NOT NULL,
    crop_type       VARCHAR(100),
    period_start    DATE NOT NULL,
    period_end      DATE NOT NULL,
    case_count      INTEGER NOT NULL DEFAULT 0,
    severity_avg    DECIMAL(3,2),
    affected_area_ha DECIMAL(12,4),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_analytics_disease_trends_region      ON analytics.disease_trends (region);
CREATE INDEX idx_analytics_disease_trends_period      ON analytics.disease_trends (period_start, period_end);
CREATE INDEX idx_analytics_disease_trends_disease     ON analytics.disease_trends (disease_name);

-- Harvest yield aggregations
CREATE TABLE analytics.harvest_aggregates (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    region          VARCHAR(100) NOT NULL,
    crop_type       VARCHAR(100) NOT NULL,
    period_start    DATE NOT NULL,
    period_end      DATE NOT NULL,
    total_yield_kg  DECIMAL(20,2),
    avg_yield_per_ha DECIMAL(10,4),
    farm_count      INTEGER,
    farmer_count    INTEGER,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Fertilizer usage patterns
CREATE TABLE analytics.input_usage (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    region          VARCHAR(100) NOT NULL,
    product_name    VARCHAR(255) NOT NULL,
    category        VARCHAR(100),
    period_start    DATE NOT NULL,
    period_end      DATE NOT NULL,
    total_units     DECIMAL(15,2),
    total_value_idr BIGINT,
    farm_count      INTEGER,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Platform-level metrics (for B2B reporting)
CREATE TABLE analytics.platform_metrics (
    id                  UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    metric_date         DATE NOT NULL UNIQUE,
    active_farmers      INTEGER,
    new_registrations   INTEGER,
    messages_processed  INTEGER,
    diseases_detected   INTEGER,
    recommendations_given INTEGER,
    loans_requested     INTEGER,
    total_harvest_kg    DECIMAL(20,2),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

COMMENT ON SCHEMA analytics IS 'Analytics domain - future B2B data product';
COMMENT ON TABLE analytics.disease_trends IS 'Sold to government/distributors: regional disease intelligence';
COMMENT ON TABLE analytics.platform_metrics IS 'Daily platform KPIs for investors and partners';
