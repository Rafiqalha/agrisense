-- Durable WhatsApp delivery lifecycle for databases created before 005_ai.sql
-- gained retry metadata. All statements are safe to run repeatedly.

ALTER TABLE ai.inbound_messages
    ADD COLUMN IF NOT EXISTS processing_attempts INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS last_attempt_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS next_attempt_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS generated_reply TEXT,
    ADD COLUMN IF NOT EXISTS reply_generated_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS outbound_message_id VARCHAR(255);

ALTER TABLE ai.inbound_messages
    DROP CONSTRAINT IF EXISTS inbound_messages_message_type_check;

ALTER TABLE ai.inbound_messages
    ADD CONSTRAINT inbound_messages_message_type_check
    CHECK (message_type IN ('text', 'image', 'audio', 'video', 'document', 'location', 'contact', 'unknown'));

CREATE INDEX IF NOT EXISTS idx_ai_inbound_retry
    ON ai.inbound_messages (next_attempt_at, received_at)
    WHERE status IN ('received', 'processing', 'failed');
