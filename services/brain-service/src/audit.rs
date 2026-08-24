//! Agent run audit — the "system correctness" trail.
//!
//! Every agent run is persisted to `ai.agent_runs` so we can later answer:
//!   - "Why did AI cost spike?"        → token/cost columns
//!   - "Which tools did the agent use?" → tools_called JSONB
//!   - "Did the model contradict the DB?" → input_message + response + context
//!
//! Principle (locked architecture): AI correctness (is the answer sensible?)
//! and system correctness (did the agent use the right data/tools?) are
//! different concerns — audit logging is part of the loop skeleton from the
//! first iteration, not a retrofit.

use anyhow::Result;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AgentRunAudit {
    pub conversation_id: Option<Uuid>,
    pub farmer_id: Uuid,
    pub agent_type: String,
    pub input_intent: Option<String>,
    pub input_message: String,
    pub response: String,
    pub model_provider: Option<String>,
    pub model_name: Option<String>,
    pub tools_called: Option<Vec<serde_json::Value>>,
    pub latency_ms: i64,
    pub success: bool,
}

/// Find the farmer's active conversation for a channel, or create one.
/// Conversations are per (farmer, channel) — here channel is always 'whatsapp'.
pub async fn get_or_create_conversation(
    pool: &shared_db::DbPool,
    farmer_id: Uuid,
    channel: &str,
) -> Result<Uuid> {
    let existing: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM ai.conversations
         WHERE farmer_id = $1 AND channel = $2
         ORDER BY started_at DESC
         LIMIT 1",
    )
    .bind(farmer_id)
    .bind(channel)
    .fetch_optional(pool)
    .await?;

    if let Some((id,)) = existing {
        return Ok(id);
    }

    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO ai.conversations (id, farmer_id, channel) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(farmer_id)
        .bind(channel)
        .execute(pool)
        .await?;
    Ok(id)
}

/// Persist one agent run to `ai.agent_runs` (audit + cost + quality tracking).
pub async fn insert_agent_run(pool: &shared_db::DbPool, run: &AgentRunAudit) -> Result<()> {
    sqlx::query(
        r#"INSERT INTO ai.agent_runs
           (conversation_id, farmer_id, agent_type, input_intent, input_message, response,
            model_provider, model_name, tools_called, tools_count, latency_ms, success, completed_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, NOW())"#,
    )
    .bind(run.conversation_id)
    .bind(run.farmer_id)
    .bind(&run.agent_type)
    .bind(&run.input_intent)
    .bind(&run.input_message)
    .bind(&run.response)
    .bind(&run.model_provider)
    .bind(&run.model_name)
    .bind(&run.tools_called)
    .bind(run.tools_called.as_ref().map(|t| t.len() as i16))
    .bind(run.latency_ms)
    .bind(run.success)
    .execute(pool)
    .await?;
    Ok(())
}
