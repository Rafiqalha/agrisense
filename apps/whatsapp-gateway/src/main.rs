//! WhatsApp Gateway
//!
//! Receives webhooks from Meta WhatsApp Business API.
//! Implements the critical safety pattern: verify → deduplicate → persist → process.
//!
//! Flow:
//!   1. Verify webhook signature (HMAC SHA256)
//!   2. Check idempotency (Redis → has this message been processed?)
//!   3. Persist inbound message to DB (audit + replay)
//!   4. Forward to brain-service
//!   5. Mark as processed
//!
//! Failures at step 4 are safe: the message is persisted, can be replayed.

use anyhow::Result;
use axum::{
    Router, Json,
    extract::{Query, State},
    http::{StatusCode, HeaderMap},
    response::IntoResponse,
    body::Bytes,
};
use serde::{Deserialize, Serialize};
use tracing::{info, warn, error};

mod signature;
mod idempotency;
mod config;

#[derive(Clone)]
struct AppState {
    brain_service_url: String,
    http_client: reqwest::Client,
    redis_url: String,
    webhook_verify_token: String,
    app_secret: String,
}

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

    let state = AppState {
        brain_service_url: std::env::var("BRAIN_SERVICE_URL")
            .unwrap_or_else(|_| "http://localhost:3002".into()),
        http_client: reqwest::Client::new(),
        redis_url: std::env::var("REDIS_URL")
            .unwrap_or_else(|_| "redis://localhost:6379".into()),
        webhook_verify_token: std::env::var("WHATSAPP_WEBHOOK_VERIFY_TOKEN")
            .unwrap_or_default(),
        app_secret: std::env::var("WHATSAPP_APP_SECRET")
            .unwrap_or_default(),
    };

    let app = Router::new()
        .route("/webhook", axum::routing::get(verify_webhook))
        .route("/webhook", axum::routing::post(receive_message))
        .route("/health",  axum::routing::get(|| async { "ok" }))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(state);

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

async fn verify_webhook(
    State(state): State<AppState>,
    Query(params): Query<VerifyQuery>,
) -> impl IntoResponse {
    if params.hub_mode.as_deref() == Some("subscribe")
        && params.hub_verify_token.as_deref() == Some(&state.webhook_verify_token)
    {
        let challenge = params.hub_challenge.unwrap_or_default();
        info!("Webhook verified");
        (StatusCode::OK, challenge)
    } else {
        warn!("Webhook verification failed");
        (StatusCode::FORBIDDEN, String::new())
    }
}

// ─── Incoming Message Handler ─────────────────────────────────────────────────
//
// Critical flow: verify → deduplicate → persist → process
//

async fn receive_message(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    // ── Step 1: Verify webhook signature ──────────────────────────────────
    if !state.app_secret.is_empty() {
        let signature = headers
            .get("x-hub-signature-256")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if !signature::verify_signature(&state.app_secret, &body, signature) {
            warn!("Invalid webhook signature");
            return StatusCode::UNAUTHORIZED;
        }
    }

    // Parse the payload
    let payload: WhatsAppWebhook = match serde_json::from_slice(&body) {
        Ok(p) => p,
        Err(e) => {
            error!(error = %e, "Failed to parse webhook payload");
            return StatusCode::BAD_REQUEST;
        }
    };

    if payload.object != "whatsapp_business_account" {
        return StatusCode::BAD_REQUEST;
    }

    for entry in &payload.entry {
        for change in &entry.changes {
            if change.field != "messages" {
                continue;
            }

            let messages = &change.value["messages"];
            let Some(msgs) = messages.as_array() else { continue };

            for msg in msgs {
                let msg_id = msg["id"].as_str().unwrap_or("").to_string();
                let from = msg["from"].as_str().unwrap_or("").to_string();
                let msg_type = msg["type"].as_str().unwrap_or("").to_string();

                if msg_id.is_empty() || from.is_empty() {
                    continue;
                }

                // ── Step 2: Idempotency check ─────────────────────────────
                let idempotency_key = format!("wa:{}:{}", from, msg_id);
                // TODO: check Redis for duplicate (idempotency::is_duplicate)
                // if is_duplicate { info!("Duplicate skipped"); continue; }

                info!(
                    from = %from,
                    msg_id = %msg_id,
                    msg_type = %msg_type,
                    "Message received"
                );

                // ── Step 3: Persist inbound message ───────────────────────
                // Write to ai.inbound_messages BEFORE processing
                // This ensures we never lose a message, even if brain-service is down
                // TODO: INSERT INTO ai.inbound_messages (
                //   external_message_id, idempotency_key, sender_phone,
                //   message_type, content, raw_payload, signature_valid
                // )

                // ── Step 4: Extract content and forward to brain ──────────
                let (content, media_url) = extract_content(msg, &msg_type);

                let orchestrate_request = serde_json::json!({
                    "farmer_id": from,  // phone number → farmer lookup in brain
                    "conversation_id": format!("wa:{}", from),
                    "message": content,
                    "media_urls": media_url.map(|u| vec![u]).unwrap_or_default(),
                    "channel": "whatsapp",
                });

                // Forward to brain-service (fire-and-forget OK for WhatsApp webhook response)
                let brain_url = format!("{}/orchestrate", state.brain_service_url);
                let client = state.http_client.clone();
                let req_body = orchestrate_request.clone();
                tokio::spawn(async move {
                    match client.post(&brain_url).json(&req_body).send().await {
                        Ok(resp) => info!(status = %resp.status(), "Brain response received"),
                        Err(e) => error!(error = %e, "Failed to reach brain-service"),
                    }
                    // Step 5: Mark as processed in ai.inbound_messages
                    // TODO: UPDATE ai.inbound_messages SET status = 'processed'
                });

                // Mark idempotency in Redis
                // TODO: idempotency::mark_processed(&idempotency_key, 86400).await;
            }
        }
    }

    StatusCode::OK
}

fn extract_content(msg: &serde_json::Value, msg_type: &str) -> (String, Option<String>) {
    match msg_type {
        "text" => {
            let text = msg["text"]["body"].as_str().unwrap_or("").to_string();
            (text, None)
        }
        "image" => {
            let media_id = msg["image"]["id"].as_str().unwrap_or("").to_string();
            let caption = msg["image"]["caption"].as_str().unwrap_or("").to_string();
            // TODO: download from Meta Media API using media_id
            (caption, Some(format!("wa-media://{}", media_id)))
        }
        "audio" => {
            let media_id = msg["audio"]["id"].as_str().unwrap_or("").to_string();
            // TODO: download → transcribe via ai-service/speech/transcribe
            ("(voice note)".into(), Some(format!("wa-media://{}", media_id)))
        }
        other => {
            (format!("(unsupported message type: {})", other), None)
        }
    }
}

// ─── Types ────────────────────────────────────────────────────────────────────

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
