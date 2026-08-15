-- ─── Outbox Pattern ────────────────────────────────────────────────────────────
-- Guarantees: DB write + event publish are atomic.
--
-- Flow:
--   1. Service writes domain data + outbox row in SAME transaction
--   2. Outbox publisher polls/listens for unpublished rows
--   3. Publisher sends to NATS JetStream
--   4. Publisher marks row as published
--
-- This eliminates the dual-write problem:
--   DB commit succeeds → NATS publish fails → event lost
--
-- Each bounded context can have its own outbox, or share this one.
-- When splitting services later, the outbox table moves with the schema.

CREATE SCHEMA IF NOT EXISTS outbox;

CREATE TYPE outbox.publish_status AS ENUM ('pending', 'published', 'failed', 'dead_letter');

CREATE TABLE outbox.events (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    -- Domain context
    aggregate_type  VARCHAR(100) NOT NULL,     -- e.g. "farmer", "farm", "disease_detection"
    aggregate_id    UUID NOT NULL,             -- e.g. farmer_id, farm_id
    -- Event metadata
    event_type      VARCHAR(100) NOT NULL,     -- e.g. "farmer_created", "disease_detected"
    nats_subject    VARCHAR(255) NOT NULL,     -- e.g. "agrisense.farmer.farmer_created"
    -- Payload (JSON serialized EventEnvelope)
    payload         JSONB NOT NULL,
    -- Idempotency
    idempotency_key VARCHAR(255) UNIQUE,       -- prevent duplicate publishes
    -- Publish tracking
    status          outbox.publish_status NOT NULL DEFAULT 'pending',
    retry_count     SMALLINT NOT NULL DEFAULT 0,
    max_retries     SMALLINT NOT NULL DEFAULT 5,
    last_error      TEXT,
    -- Timestamps
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    published_at    TIMESTAMPTZ,
    next_retry_at   TIMESTAMPTZ DEFAULT NOW()
);

-- Index for polling unpublished events (the hot path)
CREATE INDEX idx_outbox_events_pending
    ON outbox.events (next_retry_at)
    WHERE status = 'pending' OR status = 'failed';

-- Index for aggregate queries (debugging: "what events did this farmer produce?")
CREATE INDEX idx_outbox_events_aggregate
    ON outbox.events (aggregate_type, aggregate_id);

-- Dead letter monitoring
CREATE INDEX idx_outbox_events_dead_letter
    ON outbox.events (created_at)
    WHERE status = 'dead_letter';

COMMENT ON SCHEMA outbox IS 'Transactional outbox for reliable event publishing';
COMMENT ON TABLE outbox.events IS 'Dual-write eliminated: domain write + event row in same TX';
