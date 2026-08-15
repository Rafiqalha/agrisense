//! Notification Module — WhatsApp outbound, SMS, email, push
//!
//! Responsibilities:
//!   - Send WhatsApp messages (via Meta API)
//!   - Send SMS (fallback)
//!   - Template management
//!   - Delivery status tracking
//!
//! When extracted: becomes notification-service (port 3011)

use axum::Router;

pub fn router() -> Router {
    Router::new()
        .route("/send", axum::routing::post(|| async { "TODO: send notification" }))
        .route("/templates", axum::routing::get(|| async { "TODO: list templates" }))
}
