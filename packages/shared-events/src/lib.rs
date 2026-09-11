pub mod agronomy_events;
pub mod ai_events;
pub mod farm_events;
pub mod farmer_events;
pub mod finance_events;
pub mod outbox;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ─── Event Envelope ────────────────────────────────────────────────────────────
//
// Every domain event is wrapped in this envelope before publishing to NATS.
// The subject format is: agrisense.<domain>.<event_type>
//
// Examples:
//   agrisense.farmer.farmer_created
//   agrisense.agronomy.disease_detected
//   agrisense.finance.loan_requested
//

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope<T: Serialize> {
    /// Unique event ID for idempotency
    pub event_id: Uuid,
    /// ISO 8601 timestamp
    pub occurred_at: DateTime<Utc>,
    /// Source service that emitted this event
    pub source: String,
    /// Semantic event type (e.g. "farmer_created")
    pub event_type: String,
    /// Correlation ID for tracing across services
    pub correlation_id: Option<Uuid>,
    /// The domain payload
    pub payload: T,
}

impl<T: Serialize> EventEnvelope<T> {
    pub fn new(source: impl Into<String>, event_type: impl Into<String>, payload: T) -> Self {
        Self {
            event_id: Uuid::new_v4(),
            occurred_at: Utc::now(),
            source: source.into(),
            event_type: event_type.into(),
            correlation_id: None,
            payload,
        }
    }

    pub fn with_correlation(mut self, id: Uuid) -> Self {
        self.correlation_id = Some(id);
        self
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }
}

// ─── NATS Subject Builder ─────────────────────────────────────────────────────

pub struct NatsSubject;

impl NatsSubject {
    pub const PREFIX: &'static str = "agrisense";

    pub fn farmer(event: &str) -> String {
        format!("{}.farmer.{}", Self::PREFIX, event)
    }

    pub fn farm(event: &str) -> String {
        format!("{}.farm.{}", Self::PREFIX, event)
    }

    pub fn agronomy(event: &str) -> String {
        format!("{}.agronomy.{}", Self::PREFIX, event)
    }

    pub fn finance(event: &str) -> String {
        format!("{}.finance.{}", Self::PREFIX, event)
    }

    pub fn ai(event: &str) -> String {
        format!("{}.ai.{}", Self::PREFIX, event)
    }

    pub fn marketplace(event: &str) -> String {
        format!("{}.marketplace.{}", Self::PREFIX, event)
    }

    pub fn analytics(event: &str) -> String {
        format!("{}.analytics.{}", Self::PREFIX, event)
    }
}
