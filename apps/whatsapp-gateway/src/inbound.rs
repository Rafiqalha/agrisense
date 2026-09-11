use sqlx::{PgPool, Row};

const MAX_PROCESSING_ATTEMPTS: i32 = 8;

#[derive(Debug)]
pub struct NewInbound<'a> {
    pub external_message_id: &'a str,
    pub idempotency_key: &'a str,
    pub sender_phone: &'a str,
    pub message_type: &'a str,
    pub content: &'a str,
    pub media_url: Option<&'a str>,
    pub raw_payload: &'a serde_json::Value,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PersistOutcome {
    Inserted,
    Duplicate,
}

#[derive(Debug)]
pub struct StoredInbound {
    pub id: uuid::Uuid,
    pub sender_phone: String,
    pub message_type: String,
    pub content: String,
    pub media_url: Option<String>,
    pub transcribed_text: Option<String>,
    pub generated_reply: Option<String>,
    pub outbound_media_id: Option<String>,
    pub processing_attempts: i32,
}

pub async fn persist(pool: &PgPool, message: &NewInbound<'_>) -> anyhow::Result<PersistOutcome> {
    let inserted = sqlx::query_scalar::<_, uuid::Uuid>(
        r#"
        INSERT INTO ai.inbound_messages (
            external_message_id,
            idempotency_key,
            sender_phone,
            message_type,
            content,
            media_url,
            raw_payload,
            signature_valid
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, TRUE)
        ON CONFLICT DO NOTHING
        RETURNING id
        "#,
    )
    .bind(message.external_message_id)
    .bind(message.idempotency_key)
    .bind(message.sender_phone)
    .bind(message.message_type)
    .bind(message.content)
    .bind(message.media_url)
    .bind(message.raw_payload)
    .fetch_optional(pool)
    .await?;

    Ok(if inserted.is_some() {
        PersistOutcome::Inserted
    } else {
        PersistOutcome::Duplicate
    })
}

pub async fn claim_next(pool: &PgPool) -> anyhow::Result<Option<StoredInbound>> {
    let row = sqlx::query(
        r#"
        WITH candidate AS (
            SELECT id
            FROM ai.inbound_messages
            WHERE processing_attempts < $1
              AND (
                    (status IN ('received', 'failed')
                     AND (next_attempt_at IS NULL OR next_attempt_at <= NOW()))
                 OR (status = 'processing'
                     AND COALESCE(last_attempt_at, received_at) < NOW() - INTERVAL '2 minutes')
              )
            ORDER BY received_at
            FOR UPDATE SKIP LOCKED
            LIMIT 1
        )
        UPDATE ai.inbound_messages AS message
        SET status = 'processing',
            processing_attempts = message.processing_attempts + 1,
            last_attempt_at = NOW(),
            next_attempt_at = NULL,
            error_message = NULL
        FROM candidate
        WHERE message.id = candidate.id
        RETURNING message.id,
                  message.sender_phone,
                  message.message_type,
                  COALESCE(message.content, '') AS content,
                  message.media_url,
                  message.transcribed_text,
                  message.generated_reply,
                  message.outbound_media_id,
                  message.processing_attempts
        "#,
    )
    .bind(MAX_PROCESSING_ATTEMPTS)
    .fetch_optional(pool)
    .await?;

    row.map(|row| {
        Ok(StoredInbound {
            id: row.try_get("id")?,
            sender_phone: row.try_get("sender_phone")?,
            message_type: row.try_get("message_type")?,
            content: row.try_get("content")?,
            media_url: row.try_get("media_url")?,
            transcribed_text: row.try_get("transcribed_text")?,
            generated_reply: row.try_get("generated_reply")?,
            outbound_media_id: row.try_get("outbound_media_id")?,
            processing_attempts: row.try_get("processing_attempts")?,
        })
    })
    .transpose()
}

pub async fn save_transcription(
    pool: &PgPool,
    id: uuid::Uuid,
    transcript: &str,
    model: &str,
) -> anyhow::Result<()> {
    let result = sqlx::query(
        r#"
        UPDATE ai.inbound_messages
        SET transcribed_text = $2,
            transcribed_at = NOW(),
            transcription_provider = 'elevenlabs',
            transcription_model = $3
        WHERE id = $1
          AND transcribed_text IS NULL
        "#,
    )
    .bind(id)
    .bind(transcript)
    .bind(model)
    .execute(pool)
    .await?;

    anyhow::ensure!(
        result.rows_affected() == 1,
        "transcript was already stored or the inbound message no longer exists"
    );
    Ok(())
}

pub async fn save_outbound_media_id(
    pool: &PgPool,
    id: uuid::Uuid,
    media_id: &str,
) -> anyhow::Result<()> {
    let result = sqlx::query(
        r#"
        UPDATE ai.inbound_messages
        SET outbound_media_id = $2,
            outbound_media_created_at = NOW()
        WHERE id = $1
          AND outbound_media_id IS NULL
        "#,
    )
    .bind(id)
    .bind(media_id)
    .execute(pool)
    .await?;

    anyhow::ensure!(
        result.rows_affected() == 1,
        "outbound media ID was already stored or the inbound message no longer exists"
    );
    Ok(())
}

pub async fn save_generated_reply(
    pool: &PgPool,
    id: uuid::Uuid,
    reply: &str,
) -> anyhow::Result<()> {
    let result = sqlx::query(
        r#"
        UPDATE ai.inbound_messages
        SET generated_reply = $2,
            reply_generated_at = NOW()
        WHERE id = $1
          AND generated_reply IS NULL
        "#,
    )
    .bind(id)
    .bind(reply)
    .execute(pool)
    .await?;

    anyhow::ensure!(
        result.rows_affected() == 1,
        "generated reply was already stored or the inbound message no longer exists"
    );
    Ok(())
}

pub async fn mark_processed(
    pool: &PgPool,
    id: uuid::Uuid,
    outbound_message_id: Option<&str>,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        UPDATE ai.inbound_messages
        SET status = 'processed',
            processed_at = NOW(),
            outbound_message_id = $2,
            error_message = NULL,
            next_attempt_at = NULL
        WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(outbound_message_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_failed(
    pool: &PgPool,
    id: uuid::Uuid,
    attempts: i32,
    error: &str,
) -> anyhow::Result<()> {
    let delay_seconds = retry_delay_seconds(attempts);
    sqlx::query(
        r#"
        UPDATE ai.inbound_messages
        SET status = 'failed',
            error_message = LEFT($2, 2000),
            next_attempt_at = NOW() + make_interval(secs => $3)
        WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(error)
    .bind(delay_seconds)
    .execute(pool)
    .await?;
    Ok(())
}

fn retry_delay_seconds(attempts: i32) -> i32 {
    let exponent = attempts.clamp(1, 8) as u32;
    2_i32.pow(exponent).min(300)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_delay_is_bounded_exponential() {
        assert_eq!(retry_delay_seconds(1), 2);
        assert_eq!(retry_delay_seconds(4), 16);
        assert_eq!(retry_delay_seconds(8), 256);
        assert_eq!(retry_delay_seconds(100), 256);
    }
}
