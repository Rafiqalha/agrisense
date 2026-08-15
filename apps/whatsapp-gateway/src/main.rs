//! WhatsApp Gateway
//!
//! Receives webhooks from Meta WhatsApp Business API.
//! Parses text, image, and voice note messages.
//! Forwards to brain-service orchestrator.

use anyhow::Result;
use axum::{Router, Json, extract::Query};
use serde::{Deserialize, Serialize};
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    shared_observability::init(shared_observability::ObservabilityConfig {
        service_name: "whatsapp-gateway".into(),
        log_level: std::env::var("LOG_LEVEL").unwrap_or_else(|_| "info".into()),
        log_format: shared_observability::LogFormat::Pretty,
        otlp_endpoint: None,
    });

    info!(version = env!("CARGO_PKG_VERSION"), "Starting whatsapp-gateway");

    let app = Router::new()
        .route("/webhook", axum::routing::get(verify_webhook))
        .route("/webhook", axum::routing::post(receive_message))
        .route("/health",  axum::routing::get(|| async { "ok" }))
        .layer(tower_http::trace::TraceLayer::new_for_http());

    let port = std::env::var("WHATSAPP_GATEWAY_PORT").unwrap_or_else(|_| "3001".into());
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!(addr = %addr, "whatsapp-gateway listening");
    axum::serve(listener, app).await?;
    Ok(())
}

// ─── Webhook Verification (Meta challenge) ────────────────────────────────────
#[derive(Deserialize)]
struct VerifyQuery {
    #[serde(rename = "hub.mode")]
    hub_mode: Option<String>,
    #[serde(rename = "hub.verify_token")]
    hub_verify_token: Option<String>,
    #[serde(rename = "hub.challenge")]
    hub_challenge: Option<String>,
}

async fn verify_webhook(Query(params): Query<VerifyQuery>) -> axum::response::Response {
    let verify_token = std::env::var("WHATSAPP_WEBHOOK_VERIFY_TOKEN").unwrap_or_default();
    if params.hub_mode.as_deref() == Some("subscribe")
        && params.hub_verify_token.as_deref() == Some(&verify_token)
    {
        let challenge = params.hub_challenge.unwrap_or_default();
        info!("Webhook verified");
        axum::response::Response::new(challenge.into())
    } else {
        axum::http::StatusCode::FORBIDDEN.into_response()
    }
}

// ─── Incoming Message Handler ─────────────────────────────────────────────────
#[derive(Debug, Deserialize)]
struct WhatsAppWebhook {
    object: String,
    entry: Vec<WebhookEntry>,
}

#[derive(Debug, Deserialize)]
struct WebhookEntry {
    changes: Vec<WebhookChange>,
}

#[derive(Debug, Deserialize)]
struct WebhookChange {
    value: serde_json::Value,
    field: String,
}

async fn receive_message(Json(payload): Json<WhatsAppWebhook>) -> axum::http::StatusCode {
    if payload.object != "whatsapp_business_account" {
        return axum::http::StatusCode::BAD_REQUEST;
    }

    for entry in &payload.entry {
        for change in &entry.changes {
            if change.field == "messages" {
                let messages = &change.value["messages"];
                if let Some(msgs) = messages.as_array() {
                    for msg in msgs {
                        let msg_type = msg["type"].as_str().unwrap_or("");
                        let from = msg["from"].as_str().unwrap_or("");

                        match msg_type {
                            "text" => {
                                let text = msg["text"]["body"].as_str().unwrap_or("");
                                info!(from = %from, msg_type = "text", "Message received");
                                // TODO: forward to brain-service
                            }
                            "image" => {
                                let media_id = msg["image"]["id"].as_str().unwrap_or("");
                                info!(from = %from, msg_type = "image", media_id = %media_id, "Image received");
                                // TODO: download from Meta, forward to ai-service vision, then brain-service
                            }
                            "audio" => {
                                let media_id = msg["audio"]["id"].as_str().unwrap_or("");
                                info!(from = %from, msg_type = "audio", media_id = %media_id, "Voice note received");
                                // TODO: transcribe via ai-service speech, then brain-service
                            }
                            other => {
                                info!(from = %from, msg_type = %other, "Unhandled message type");
                            }
                        }
                    }
                }
            }
        }
    }

    axum::http::StatusCode::OK
}

trait IntoResponse {
    fn into_response(self) -> axum::response::Response;
}

impl IntoResponse for axum::http::StatusCode {
    fn into_response(self) -> axum::response::Response {
        axum::response::Response::builder()
            .status(self)
            .body(axum::body::Body::empty())
            .unwrap()
    }
}
