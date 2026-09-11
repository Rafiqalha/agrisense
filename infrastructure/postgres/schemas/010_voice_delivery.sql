-- Durable checkpoints for the WhatsApp voice-note pipeline.
-- STT results and uploaded Meta media IDs are reused across delivery retries.

ALTER TABLE ai.inbound_messages
    ADD COLUMN IF NOT EXISTS transcribed_text TEXT,
    ADD COLUMN IF NOT EXISTS transcribed_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS transcription_provider VARCHAR(50),
    ADD COLUMN IF NOT EXISTS transcription_model VARCHAR(100),
    ADD COLUMN IF NOT EXISTS outbound_media_id VARCHAR(255),
    ADD COLUMN IF NOT EXISTS outbound_media_created_at TIMESTAMPTZ;
