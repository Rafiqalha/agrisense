//! Transactional Outbox Pattern
//!
//! Instead of:
//!   1. Write to DB
//!   2. Publish to NATS  ← can fail → event lost
//!
//! We do:
//!   1. Write domain data + outbox row in SAME transaction
//!   2. Outbox publisher polls/streams unpublished rows
//!   3. Publisher sends to NATS
//!   4. Publisher marks row as published
//!
//! Usage in a service:
//! ```ignore
//! let tx = pool.begin().await?;
//!
//! // Domain write
//! sqlx::query("INSERT INTO farm.farmers ...").execute(&mut *tx).await?;
//!
//! // Outbox write (same transaction!)
//! OutboxEntry::new("farmer", farmer_id, "farmer_created", subject, &event)
//!     .insert(&mut *tx)
//!     .await?;
//!
//! tx.commit().await?;
//! // Event WILL be published, even if NATS was temporarily down.
//! ```

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxEntry {
    pub id: Uuid,
    pub aggregate_type: String,
    pub aggregate_id: Uuid,
    pub event_type: String,
    pub nats_subject: String,
    pub payload: serde_json::Value,
    pub idempotency_key: String,
    pub created_at: DateTime<Utc>,
}

impl OutboxEntry {
    /// Create a new outbox entry. Call `.insert()` within the same DB transaction
    /// as your domain write to guarantee atomicity.
    pub fn new<T: Serialize>(
        aggregate_type: &str,
        aggregate_id: Uuid,
        event_type: &str,
        nats_subject: &str,
        payload: &T,
    ) -> Self {
        let event_id = Uuid::new_v4();
        Self {
            id: event_id,
            aggregate_type: aggregate_type.into(),
            aggregate_id,
            event_type: event_type.into(),
            nats_subject: nats_subject.into(),
            payload: serde_json::to_value(payload).unwrap_or_default(),
            idempotency_key: format!("{}:{}:{}", aggregate_type, aggregate_id, event_id),
            created_at: Utc::now(),
        }
    }
}

/// Outbox publisher configuration
#[derive(Debug, Clone)]
pub struct OutboxPublisherConfig {
    /// How often to poll for unpublished events (ms)
    pub poll_interval_ms: u64,
    /// How many events to process per batch
    pub batch_size: u32,
    /// Maximum retry count before moving to dead letter
    pub max_retries: u16,
}

impl Default for OutboxPublisherConfig {
    fn default() -> Self {
        Self {
            poll_interval_ms: 500,
            batch_size: 100,
            max_retries: 5,
        }
    }
}
