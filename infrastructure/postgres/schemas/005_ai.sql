-- ─── AI Schema ────────────────────────────────────────────────────────────────
-- Domain: Conversations, agent runs, model usage tracking
-- Owned by: ai-service, brain-service

CREATE SCHEMA IF NOT EXISTS ai;

CREATE TABLE ai.conversations (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    farmer_id       UUID NOT NULL,
    channel         VARCHAR(50) NOT NULL DEFAULT 'whatsapp',
    started_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_message_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    message_count   INTEGER NOT NULL DEFAULT 0,
    metadata        JSONB
);

CREATE INDEX idx_ai_conversations_farmer_id ON ai.conversations (farmer_id);

CREATE TABLE ai.messages (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    conversation_id UUID NOT NULL REFERENCES ai.conversations(id),
    farmer_id       UUID NOT NULL,
    role            VARCHAR(20) NOT NULL CHECK (role IN ('user', 'assistant', 'system')),
    content         TEXT NOT NULL,
    media_urls      JSONB,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_ai_messages_conversation_id ON ai.messages (conversation_id);
CREATE INDEX idx_ai_messages_created_at      ON ai.messages (created_at);

-- ─── Agent Runs (expanded audit) ──────────────────────────────────────────────
-- Answers: "Why did AI cost spike?", "Why is Gemini worse for melon diagnosis?"
CREATE TABLE ai.agent_runs (
    id                  UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    conversation_id     UUID REFERENCES ai.conversations(id),
    farmer_id           UUID NOT NULL,
    -- Agent context
    agent_type          VARCHAR(100) NOT NULL,
    input_intent        VARCHAR(100),
    input_message       TEXT,
    response            TEXT,
    -- Model details (for cost + quality tracking)
    model_provider      VARCHAR(50),           -- "gemini", "openai", "anthropic", "local"
    model_name          VARCHAR(100),          -- "gemini-2.0-flash", "gpt-4o", etc.
    model_version       VARCHAR(50),           -- specific version/checkpoint
    prompt_version      VARCHAR(50),           -- "v1.2", tracks prompt engineering changes
    prompt_template     VARCHAR(255),          -- which template was used
    -- Tool calls (full detail, not just names)
    tools_called        JSONB,                 -- [{name, args, result_summary, latency_ms, success}]
    tools_count         SMALLINT DEFAULT 0,
    -- Token usage + cost
    input_tokens        INTEGER,
    output_tokens       INTEGER,
    total_tokens        INTEGER GENERATED ALWAYS AS (COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)) STORED,
    cost_usd            DECIMAL(10,6),         -- estimated cost per run
    -- Performance
    latency_ms          INTEGER,
    -- Outcome
    success             BOOLEAN NOT NULL DEFAULT true,
    error_type          VARCHAR(100),          -- "timeout", "rate_limit", "invalid_response", "tool_error"
    error_message       TEXT,
    -- Risk assessment
    risk_level          VARCHAR(20) DEFAULT 'normal'
                        CHECK (risk_level IN ('low', 'normal', 'elevated', 'high', 'critical')),
    -- Safety
    moderation_flagged  BOOLEAN DEFAULT false,
    moderation_reason   TEXT,
    -- Timestamps
    started_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at        TIMESTAMPTZ
);

CREATE INDEX idx_ai_agent_runs_farmer_id    ON ai.agent_runs (farmer_id);
CREATE INDEX idx_ai_agent_runs_started_at   ON ai.agent_runs (started_at);
CREATE INDEX idx_ai_agent_runs_agent_type   ON ai.agent_runs (agent_type);
CREATE INDEX idx_ai_agent_runs_model        ON ai.agent_runs (model_provider, model_name);
CREATE INDEX idx_ai_agent_runs_prompt_ver   ON ai.agent_runs (prompt_version);
CREATE INDEX idx_ai_agent_runs_risk         ON ai.agent_runs (risk_level) WHERE risk_level IN ('elevated', 'high', 'critical');
CREATE INDEX idx_ai_agent_runs_cost         ON ai.agent_runs (cost_usd) WHERE cost_usd > 0;

-- ─── Inbound Messages (persist-before-process) ────────────────────────────────
-- WhatsApp Gateway writes here BEFORE forwarding to brain-service.
-- Enables: audit, replay, deduplication, webhook idempotency.
CREATE TABLE ai.inbound_messages (
    id                  UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    -- Deduplication
    external_message_id VARCHAR(255) UNIQUE NOT NULL,  -- WhatsApp message ID
    webhook_id          VARCHAR(255),                  -- Meta webhook delivery ID
    idempotency_key     VARCHAR(255) UNIQUE NOT NULL,  -- composite key for dedup
    -- Source
    channel             VARCHAR(50) NOT NULL DEFAULT 'whatsapp',
    sender_phone        VARCHAR(20) NOT NULL,
    -- Content
    message_type        VARCHAR(20) NOT NULL CHECK (message_type IN ('text', 'image', 'audio', 'video', 'document', 'location', 'contact')),
    content             TEXT,
    media_url           TEXT,
    media_mime_type     VARCHAR(100),
    raw_payload         JSONB NOT NULL,                -- full webhook payload for replay
    -- Processing status
    status              VARCHAR(20) NOT NULL DEFAULT 'received'
                        CHECK (status IN ('received', 'processing', 'processed', 'failed', 'duplicate')),
    processed_at        TIMESTAMPTZ,
    error_message       TEXT,
    -- Signature verification
    signature_valid     BOOLEAN,
    -- Timestamps
    received_at         TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_ai_inbound_external_id ON ai.inbound_messages (external_message_id);
CREATE INDEX idx_ai_inbound_phone       ON ai.inbound_messages (sender_phone);
CREATE INDEX idx_ai_inbound_status      ON ai.inbound_messages (status) WHERE status IN ('received', 'processing', 'failed');
CREATE INDEX idx_ai_inbound_received_at ON ai.inbound_messages (received_at);

-- ─── Workflow Runs (durable state for multi-step flows) ───────────────────────
-- Workflow state persisted to DB, not just RAM.
-- Enables: resume after crash, retry failed steps, audit trail.
CREATE TABLE ai.workflow_runs (
    id                  UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    conversation_id     UUID REFERENCES ai.conversations(id),
    farmer_id           UUID NOT NULL,
    -- Workflow definition
    workflow_type       VARCHAR(100) NOT NULL,   -- "diagnose_crop_disease", "onboard_farmer", etc.
    -- State machine
    current_step        VARCHAR(100) NOT NULL,   -- "detect_disease", "check_inventory", "recommend"
    steps_completed     JSONB NOT NULL DEFAULT '[]',    -- [{step, result_summary, completed_at}]
    steps_remaining     JSONB NOT NULL DEFAULT '[]',    -- [{step, depends_on}]
    total_steps         SMALLINT NOT NULL,
    completed_steps     SMALLINT NOT NULL DEFAULT 0,
    -- Context (accumulated data from each step)
    context             JSONB NOT NULL DEFAULT '{}',
    -- Status
    status              VARCHAR(20) NOT NULL DEFAULT 'running'
                        CHECK (status IN ('running', 'paused', 'completed', 'failed', 'cancelled', 'timed_out')),
    error_message       TEXT,
    retry_count         SMALLINT DEFAULT 0,
    -- Timestamps
    started_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at        TIMESTAMPTZ,
    timeout_at          TIMESTAMPTZ             -- auto-cancel if not completed by this time
);

CREATE INDEX idx_ai_workflow_runs_farmer_id     ON ai.workflow_runs (farmer_id);
CREATE INDEX idx_ai_workflow_runs_conversation  ON ai.workflow_runs (conversation_id);
CREATE INDEX idx_ai_workflow_runs_status        ON ai.workflow_runs (status) WHERE status IN ('running', 'paused');
CREATE INDEX idx_ai_workflow_runs_timeout       ON ai.workflow_runs (timeout_at) WHERE status = 'running';

COMMENT ON SCHEMA ai IS 'AI conversations, agent runs, workflow state, and inbound message audit';
COMMENT ON TABLE ai.agent_runs IS 'Full audit: every AI run with model, prompt version, cost, risk, and tool calls';
COMMENT ON TABLE ai.inbound_messages IS 'Persist-before-process: every inbound message for idempotency and replay';
COMMENT ON TABLE ai.workflow_runs IS 'Durable workflow state: multi-step flows survive crashes and can be resumed';
