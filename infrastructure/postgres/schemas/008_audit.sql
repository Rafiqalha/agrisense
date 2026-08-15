-- ─── Audit Schema ──────────────────────────────────────────────────────────────
-- Domain: Full audit trail for compliance, debugging, security
-- Owned by: platform-service
--
-- Every mutation across the system is recorded here.
-- Required for:
--   - "Siapa yang mengubah data ini?"
--   - "Agent mana yang melakukan action ini?"
--   - "Tool apa yang dipanggil?"
--   - "Model apa yang digunakan?"
--   - "Prompt version berapa?"
--
-- AI systems MUST have full audit trails.

CREATE SCHEMA IF NOT EXISTS audit;

CREATE TYPE audit.action_type AS ENUM (
    'create', 'update', 'delete',
    'login', 'logout',
    'api_call', 'webhook_received',
    'mcp_tool_call', 'agent_run',
    'event_published', 'event_consumed',
    'permission_check', 'rate_limit_hit'
);

CREATE TABLE audit.logs (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    -- Who
    actor_type      VARCHAR(50) NOT NULL,     -- "farmer", "admin", "agent", "system", "webhook"
    actor_id        VARCHAR(255),             -- user_id, agent_name, service_name
    -- What
    action          audit.action_type NOT NULL,
    resource_type   VARCHAR(100) NOT NULL,    -- "farm", "transaction", "mcp_tool", "agent_run"
    resource_id     VARCHAR(255),             -- UUID of the affected resource
    -- Details
    description     TEXT,
    metadata        JSONB,                    -- flexible: model_name, prompt_version, tool_calls, etc.
    -- Context
    service         VARCHAR(100) NOT NULL,    -- which service generated this
    correlation_id  UUID,                     -- trace across services
    ip_address      INET,
    user_agent      TEXT,
    -- Risk
    risk_level      VARCHAR(20) DEFAULT 'normal' CHECK (risk_level IN ('low', 'normal', 'elevated', 'high', 'critical')),
    -- Timestamp
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Hot queries
CREATE INDEX idx_audit_logs_actor       ON audit.logs (actor_type, actor_id);
CREATE INDEX idx_audit_logs_resource    ON audit.logs (resource_type, resource_id);
CREATE INDEX idx_audit_logs_action      ON audit.logs (action);
CREATE INDEX idx_audit_logs_created_at  ON audit.logs (created_at);
CREATE INDEX idx_audit_logs_correlation ON audit.logs (correlation_id);
CREATE INDEX idx_audit_logs_risk        ON audit.logs (risk_level) WHERE risk_level IN ('elevated', 'high', 'critical');

-- Partition by month for performance (optional, enable when data grows)
-- CREATE TABLE audit.logs_2026_08 PARTITION OF audit.logs
--     FOR VALUES FROM ('2026-08-01') TO ('2026-09-01');

COMMENT ON SCHEMA audit IS 'Full system audit trail — compliance + debugging + AI accountability';
COMMENT ON TABLE audit.logs IS 'Every mutation, tool call, agent run, and permission check';
