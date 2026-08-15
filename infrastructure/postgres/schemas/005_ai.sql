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

CREATE TABLE ai.agent_runs (
    id                  UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    conversation_id     UUID REFERENCES ai.conversations(id),
    farmer_id           UUID NOT NULL,
    agent_type          VARCHAR(100) NOT NULL,
    input_intent        VARCHAR(100),
    input_message       TEXT,
    response            TEXT,
    tools_used          JSONB,
    model_provider      VARCHAR(50),
    model_name          VARCHAR(100),
    input_tokens        INTEGER,
    output_tokens       INTEGER,
    latency_ms          INTEGER,
    success             BOOLEAN NOT NULL DEFAULT true,
    error_message       TEXT,
    started_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at        TIMESTAMPTZ
);

CREATE INDEX idx_ai_agent_runs_farmer_id   ON ai.agent_runs (farmer_id);
CREATE INDEX idx_ai_agent_runs_started_at  ON ai.agent_runs (started_at);
CREATE INDEX idx_ai_agent_runs_agent_type  ON ai.agent_runs (agent_type);

COMMENT ON SCHEMA ai IS 'AI conversations, agent runs, and model usage tracking';
COMMENT ON TABLE ai.agent_runs IS 'Full audit trail of every AI agent execution';
