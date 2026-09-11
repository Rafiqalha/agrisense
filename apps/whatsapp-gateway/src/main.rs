//! WhatsApp Gateway
//!
//! Receives webhooks from Meta WhatsApp Business API.
//! Implements the critical safety pattern: verify → deduplicate → persist → process.
//!
//! Flow:
//!   1. Verify webhook signature (HMAC SHA256)
//!   2. Check idempotency (Redis → has this message been processed?)
//!   3. Persist inbound message to DB (audit + replay)
//!   4. Generate and persist the reply once
//!   5. Deliver the persisted reply and mark as processed
//!
//! Failures after step 4 retry delivery without repeating the AI request.

use anyhow::{Context, Result};
use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Router,
};
use serde::Deserialize;
use std::{future::Future, time::Duration};
use tracing::{error, info, warn};

mod config;
mod elevenlabs;
mod inbound;
mod signature;
mod whatsapp;

#[derive(Clone)]
struct AppState {
    brain_service_url: String,
    http_client: reqwest::Client,
    db: shared_db::DbPool,
    webhook_verify_token: String,
    app_secret: String,
    brain_token: shared_auth::InternalToken,
    request_slots: std::sync::Arc<tokio::sync::Semaphore>,
    /// Outbound WhatsApp client. `None` when WHATSAPP_PHONE_NUMBER_ID /
    /// WHATSAPP_ACCESS_TOKEN are not configured (e.g. local dev without Meta).
    whatsapp: Option<whatsapp::WhatsAppClient>,
    /// Speech-to-text and text-to-speech. Voice messages fail closed when the
    /// two required ElevenLabs credentials are not configured.
    elevenlabs: Option<elevenlabs::ElevenLabsClient>,
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

    info!(
        version = env!("CARGO_PKG_VERSION"),
        "Starting whatsapp-gateway"
    );

    let database_url = required_env("DATABASE_URL")?;
    if std::env::var("APP_ENV").as_deref() == Ok("production") {
        anyhow::ensure!(
            database_url.starts_with("postgres://agrisense_gateway:"),
            "whatsapp-gateway must use the dedicated agrisense_gateway database role in production"
        );
        anyhow::ensure!(
            !database_url.contains("gateway-local-only"),
            "GATEWAY_DB_PASSWORD must be replaced before production"
        );
    }
    let webhook_verify_token = required_env("WHATSAPP_WEBHOOK_VERIFY_TOKEN")?;
    let app_secret = required_env("WHATSAPP_APP_SECRET")?;
    let brain_token = shared_auth::InternalToken::new(required_env("GATEWAY_BRAIN_TOKEN")?)?;
    let db = shared_db::create_pool(&database_url, 10)
        .await
        .context("failed to connect WhatsApp gateway to PostgreSQL")?;
    let whatsapp = match (
        std::env::var("WHATSAPP_PHONE_NUMBER_ID").ok(),
        std::env::var("WHATSAPP_ACCESS_TOKEN").ok(),
    ) {
        (Some(phone_number_id), Some(access_token))
            if !phone_number_id.is_empty() && !access_token.is_empty() =>
        {
            let graph_api_version =
                std::env::var("WHATSAPP_GRAPH_API_VERSION").unwrap_or_else(|_| "v25.0".into());
            let client = whatsapp::WhatsAppClient::new(
                phone_number_id,
                access_token,
                graph_api_version.clone(),
            )?;
            info!(graph_api_version, "Outbound WhatsApp client initialized");
            Some(client)
        }
        _ => None,
    };
    if std::env::var("APP_ENV").as_deref() == Ok("production") && whatsapp.is_none() {
        anyhow::bail!(
            "WHATSAPP_PHONE_NUMBER_ID and WHATSAPP_ACCESS_TOKEN are required in production"
        );
    }
    if whatsapp.is_none() {
        warn!("WhatsApp outbound credentials not set; replies will only be logged");
    }
    let elevenlabs = configured_elevenlabs()?;
    if elevenlabs.is_some() {
        info!("ElevenLabs voice-note pipeline initialized");
    } else {
        warn!("ElevenLabs credentials not set; WhatsApp voice notes are disabled");
    }

    let state = AppState {
        brain_service_url: std::env::var("BRAIN_SERVICE_URL")
            .unwrap_or_else(|_| "http://localhost:3002".into()),
        http_client: reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(45))
            .build()?,
        db,
        webhook_verify_token,
        app_secret,
        brain_token,
        request_slots: std::sync::Arc::new(tokio::sync::Semaphore::new(128)),
        whatsapp,
        elevenlabs,
    };

    tokio::spawn(run_delivery_worker(state.clone()));

    let app = Router::new()
        .route("/webhook", axum::routing::get(verify_webhook))
        .route("/webhook", axum::routing::post(receive_message))
        .route("/webhook/", axum::routing::post(receive_message))
        .route("/health", axum::routing::get(|| async { "ok" }))
        .route("/live", axum::routing::get(|| async { "ok" }))
        .route("/ready", axum::routing::get(gateway_readiness))
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(state);

    let port = std::env::var("WHATSAPP_GATEWAY_PORT").unwrap_or_else(|_| "3001".into());
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!(addr = %addr, "whatsapp-gateway listening");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn gateway_readiness(State(state): State<AppState>) -> StatusCode {
    if shared_db::is_ready(&state.db).await {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

fn required_env(name: &str) -> anyhow::Result<String> {
    let value = std::env::var(name).with_context(|| format!("{name} must be configured"))?;
    anyhow::ensure!(!value.trim().is_empty(), "{name} must not be empty");
    Ok(value)
}

fn configured_elevenlabs() -> Result<Option<elevenlabs::ElevenLabsClient>> {
    let api_key = optional_nonempty_env("ELEVENLABS_API_KEY");
    let voice_id = optional_nonempty_env("ELEVENLABS_VOICE_ID");
    match (api_key, voice_id) {
        (None, None) => Ok(None),
        (Some(api_key), Some(voice_id)) => elevenlabs::ElevenLabsClient::new(
            api_key,
            voice_id,
            std::env::var("ELEVENLABS_STT_MODEL").unwrap_or_else(|_| "scribe_v2".into()),
            std::env::var("ELEVENLABS_TTS_MODEL")
                .unwrap_or_else(|_| "eleven_multilingual_v2".into()),
            std::env::var("ELEVENLABS_LANGUAGE_CODE").unwrap_or_else(|_| "id".into()),
            std::env::var("ELEVENLABS_TTS_OUTPUT_FORMAT")
                .unwrap_or_else(|_| "mp3_44100_128".into()),
        )
        .map(Some),
        _ => anyhow::bail!(
            "ELEVENLABS_API_KEY and ELEVENLABS_VOICE_ID must either both be set or both be absent"
        ),
    }
}

fn optional_nonempty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
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
    let _permit = match state.request_slots.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => return StatusCode::TOO_MANY_REQUESTS,
    };

    // ── Step 1: Verify webhook signature ──────────────────────────────────
    let signature = headers
        .get("x-hub-signature-256")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if !signature::verify_signature(&state.app_secret, &body, signature) {
        warn!("Invalid webhook signature");
        return StatusCode::UNAUTHORIZED;
    }

    // Parse the payload
    let raw_payload: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(e) => {
            error!(error = %e, "Failed to parse webhook payload");
            return StatusCode::BAD_REQUEST;
        }
    };
    let payload: WhatsAppWebhook = match serde_json::from_value(raw_payload.clone()) {
        Ok(payload) => payload,
        Err(e) => {
            error!(error = %e, "Webhook payload does not match the expected schema");
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
            let Some(msgs) = messages.as_array() else {
                continue;
            };

            for msg in msgs {
                let msg_id = msg["id"].as_str().unwrap_or("").to_string();
                let from = msg["from"].as_str().unwrap_or("").to_string();
                let msg_type = msg["type"].as_str().unwrap_or("").to_string();

                if msg_id.is_empty() || from.is_empty() {
                    continue;
                }

                info!(
                    from = %from,
                    msg_id = %msg_id,
                    msg_type = %msg_type,
                    "Message received"
                );

                let (content, media_id) = extract_content(msg, &msg_type);
                let idempotency_key = format!("wa:{from}:{msg_id}");
                let normalized_type = normalize_message_type(&msg_type);
                let record = inbound::NewInbound {
                    external_message_id: &msg_id,
                    idempotency_key: &idempotency_key,
                    sender_phone: &from,
                    message_type: normalized_type,
                    content: &content,
                    media_url: media_id.as_deref(),
                    raw_payload: &raw_payload,
                };

                match inbound::persist(&state.db, &record).await {
                    Ok(inbound::PersistOutcome::Inserted) => {
                        info!(msg_id = %msg_id, "Inbound message durably persisted");
                    }
                    Ok(inbound::PersistOutcome::Duplicate) => {
                        info!(msg_id = %msg_id, "Duplicate message acknowledged without reprocessing");
                    }
                    Err(e) => {
                        error!(error = %e, msg_id = %msg_id, "Could not persist inbound message");
                        return StatusCode::SERVICE_UNAVAILABLE;
                    }
                }
            }
        }
    }

    StatusCode::OK
}

fn normalize_message_type(message_type: &str) -> &str {
    match message_type {
        "text" | "image" | "audio" | "video" | "document" | "location" | "contact" => message_type,
        _ => "unknown",
    }
}

async fn run_delivery_worker(state: AppState) {
    loop {
        match inbound::claim_next(&state.db).await {
            Ok(Some(message)) => {
                let message_id = message.id;
                let attempts = message.processing_attempts;
                match process_inbound(&state, &message).await {
                    Ok(outbound_message_id) => {
                        if let Err(e) = inbound::mark_processed(
                            &state.db,
                            message_id,
                            outbound_message_id.as_deref(),
                        )
                        .await
                        {
                            error!(error = %e, %message_id, "Could not mark inbound message processed");
                        }
                    }
                    Err(e) => {
                        let error_chain = format!("{e:#}");
                        error!(error = %error_chain, %message_id, attempts, "Inbound processing failed");
                        if let Err(mark_error) =
                            inbound::mark_failed(&state.db, message_id, attempts, &error_chain)
                                .await
                        {
                            error!(error = %mark_error, %message_id, "Could not persist failure state");
                        }
                    }
                }
            }
            Ok(None) => tokio::time::sleep(Duration::from_millis(500)).await,
            Err(e) => {
                error!(error = %e, "Inbound worker could not claim a message");
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
}

async fn process_inbound(
    state: &AppState,
    message: &inbound::StoredInbound,
) -> Result<Option<String>> {
    let reply = resolve_reply(
        message.generated_reply.as_deref(),
        || generate_reply(state, message),
        |reply| async move { inbound::save_generated_reply(&state.db, message.id, &reply).await },
    )
    .await?;

    let outbound_message_id = if let Some(whatsapp) = &state.whatsapp {
        let result = if message.message_type == "audio" {
            let media_id = resolve_outbound_audio(state, message, &reply).await?;
            whatsapp
                .send_audio_message(&message.sender_phone, &media_id)
                .await
                .map_err(|error| {
                    anyhow::anyhow!("failed to send WhatsApp audio reply: {error:#}")
                })?
        } else {
            whatsapp
                .send_text_message(&message.sender_phone, &reply)
                .await
                .map_err(|error| anyhow::anyhow!("failed to send WhatsApp reply: {error:#}"))?
        };
        info!(
            to = %message.sender_phone,
            reply_type = if message.message_type == "audio" { "audio" } else { "text" },
            outbound_message_id = result.message_id.as_deref().unwrap_or("unavailable"),
            "Reply sent to WhatsApp"
        );
        result.message_id
    } else {
        info!(
            message_id = %message.id,
            "Outbound disabled; generated reply persisted"
        );
        None
    };

    Ok(outbound_message_id)
}

async fn resolve_outbound_audio(
    state: &AppState,
    message: &inbound::StoredInbound,
    reply: &str,
) -> Result<String> {
    if let Some(media_id) = message.outbound_media_id.as_deref() {
        anyhow::ensure!(!media_id.is_empty(), "cached outbound media ID is empty");
        return Ok(media_id.to_owned());
    }

    let elevenlabs = state
        .elevenlabs
        .as_ref()
        .context("ElevenLabs must be configured to reply to a voice note")?;
    let whatsapp = state
        .whatsapp
        .as_ref()
        .context("WhatsApp client is required to upload generated speech")?;
    let audio = elevenlabs.synthesize(reply).await?;
    let media_id = whatsapp
        .upload_media(audio.bytes, audio.mime_type, audio.filename)
        .await
        .context("failed to upload ElevenLabs speech to WhatsApp")?;
    inbound::save_outbound_media_id(&state.db, message.id, &media_id)
        .await
        .context("failed to persist outbound WhatsApp media ID")?;
    Ok(media_id)
}

async fn generate_reply(state: &AppState, message: &inbound::StoredInbound) -> Result<String> {
    let mut resolved_media = Vec::new();
    let prompt = if message.message_type == "audio" {
        resolve_transcript(
            message.transcribed_text.as_deref(),
            || transcribe_voice_note(state, message),
            |transcript| async move {
                inbound::save_transcription(
                    &state.db,
                    message.id,
                    &transcript.text,
                    &transcript.model,
                )
                .await
            },
        )
        .await?
    } else {
        message.content.clone()
    };

    if message.message_type != "audio" {
        if let Some(media_url) = message.media_url.as_deref() {
            if let Some(media_id) = media_url.strip_prefix("wa-media://") {
                let whatsapp = state
                    .whatsapp
                    .as_ref()
                    .context("WhatsApp client is required to download inbound media")?;
                let (bytes, mime) = whatsapp.download_media(media_id).await?;
                resolved_media.push(whatsapp::to_data_uri(&mime, &bytes));
            }
        }
    }

    let brain_url = format!(
        "{}/orchestrate",
        state.brain_service_url.trim_end_matches('/')
    );
    let request = serde_json::json!({
        "farmer_id": message.sender_phone,
        "request_id": message.id,
        "conversation_id": format!("wa:{}", message.sender_phone),
        "message": prompt,
        "media_urls": resolved_media,
        "channel": "whatsapp",
    });
    let response = state
        .http_client
        .post(brain_url)
        .bearer_auth(state.brain_token.expose_for_request())
        .json(&request)
        .send()
        .await
        .context("failed to reach brain-service")?;
    let status = response.status();
    anyhow::ensure!(status.is_success(), "brain-service returned {status}");

    let body: serde_json::Value = response
        .json()
        .await
        .context("brain-service returned an invalid response")?;
    let reply = body["response"]
        .as_str()
        .context("brain-service response did not contain response text")?;
    anyhow::ensure!(
        !reply.trim().is_empty(),
        "brain-service returned an empty reply"
    );

    Ok(reply.to_owned())
}

async fn transcribe_voice_note(
    state: &AppState,
    message: &inbound::StoredInbound,
) -> Result<elevenlabs::Transcript> {
    let media_url = message
        .media_url
        .as_deref()
        .context("voice note does not contain a WhatsApp media ID")?;
    let media_id = media_url
        .strip_prefix("wa-media://")
        .filter(|value| !value.is_empty())
        .context("voice note contains an invalid WhatsApp media ID")?;
    let whatsapp = state
        .whatsapp
        .as_ref()
        .context("WhatsApp client is required to download a voice note")?;
    let elevenlabs = state
        .elevenlabs
        .as_ref()
        .context("ElevenLabs must be configured to transcribe a voice note")?;
    let (bytes, mime) = whatsapp.download_media(media_id).await?;
    anyhow::ensure!(
        mime.starts_with("audio/"),
        "WhatsApp voice-note media has unexpected MIME type {mime}"
    );
    elevenlabs.transcribe(bytes, &mime).await
}

async fn resolve_transcript<Generate, GenerateFuture, Save, SaveFuture>(
    cached_transcript: Option<&str>,
    generate: Generate,
    save: Save,
) -> Result<String>
where
    Generate: FnOnce() -> GenerateFuture,
    GenerateFuture: Future<Output = Result<elevenlabs::Transcript>>,
    Save: FnOnce(elevenlabs::Transcript) -> SaveFuture,
    SaveFuture: Future<Output = Result<()>>,
{
    if let Some(transcript) = cached_transcript {
        anyhow::ensure!(
            !transcript.trim().is_empty(),
            "cached transcript must not be empty"
        );
        return Ok(transcript.to_owned());
    }

    let transcript = generate().await?;
    anyhow::ensure!(
        !transcript.text.trim().is_empty(),
        "generated transcript must not be empty"
    );
    let text = transcript.text.clone();
    save(transcript).await?;
    Ok(text)
}

async fn resolve_reply<Generate, GenerateFuture, Save, SaveFuture>(
    cached_reply: Option<&str>,
    generate: Generate,
    save: Save,
) -> Result<String>
where
    Generate: FnOnce() -> GenerateFuture,
    GenerateFuture: Future<Output = Result<String>>,
    Save: FnOnce(String) -> SaveFuture,
    SaveFuture: Future<Output = Result<()>>,
{
    if let Some(reply) = cached_reply {
        anyhow::ensure!(!reply.trim().is_empty(), "cached reply must not be empty");
        return Ok(reply.to_owned());
    }

    let reply = generate().await?;
    anyhow::ensure!(
        !reply.trim().is_empty(),
        "generated reply must not be empty"
    );
    save(reply.clone()).await?;
    Ok(reply)
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
            (caption, Some(format!("wa-media://{}", media_id)))
        }
        "audio" => {
            let media_id = msg["audio"]["id"].as_str().unwrap_or("").to_string();
            let media_url = (!media_id.is_empty()).then(|| format!("wa-media://{media_id}"));
            ("(voice note)".into(), media_url)
        }
        other => (format!("(unsupported message type: {})", other), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    #[test]
    fn unsupported_message_types_are_stored_as_unknown() {
        assert_eq!(normalize_message_type("text"), "text");
        assert_eq!(normalize_message_type("sticker"), "unknown");
    }

    #[test]
    fn required_env_rejects_blank_values() {
        let name = "AGRISENSE_TEST_REQUIRED_ENV_BLANK";
        std::env::set_var(name, "   ");
        let result = required_env(name);
        std::env::remove_var(name);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn cached_reply_skips_generation_and_persistence() {
        let generate_calls = Arc::new(AtomicUsize::new(0));
        let save_calls = Arc::new(AtomicUsize::new(0));
        let generate_counter = Arc::clone(&generate_calls);
        let save_counter = Arc::clone(&save_calls);

        let reply = resolve_reply(
            Some("cached answer"),
            move || async move {
                generate_counter.fetch_add(1, Ordering::SeqCst);
                Ok("new answer".into())
            },
            move |_| async move {
                save_counter.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
        )
        .await
        .unwrap();

        assert_eq!(reply, "cached answer");
        assert_eq!(generate_calls.load(Ordering::SeqCst), 0);
        assert_eq!(save_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn new_reply_is_persisted_before_delivery_can_continue() {
        let generate_calls = Arc::new(AtomicUsize::new(0));
        let save_calls = Arc::new(AtomicUsize::new(0));
        let generate_counter = Arc::clone(&generate_calls);
        let save_counter = Arc::clone(&save_calls);

        let reply = resolve_reply(
            None,
            move || async move {
                generate_counter.fetch_add(1, Ordering::SeqCst);
                Ok("new answer".into())
            },
            move |reply| async move {
                assert_eq!(reply, "new answer");
                save_counter.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
        )
        .await
        .unwrap();

        assert_eq!(reply, "new answer");
        assert_eq!(generate_calls.load(Ordering::SeqCst), 1);
        assert_eq!(save_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn cached_transcript_skips_stt_and_persistence() {
        let generate_calls = Arc::new(AtomicUsize::new(0));
        let save_calls = Arc::new(AtomicUsize::new(0));
        let generate_counter = Arc::clone(&generate_calls);
        let save_counter = Arc::clone(&save_calls);

        let transcript = resolve_transcript(
            Some("daun cabai saya menguning"),
            move || async move {
                generate_counter.fetch_add(1, Ordering::SeqCst);
                Ok(elevenlabs::Transcript {
                    text: "new transcript".into(),
                    model: "scribe_v2".into(),
                })
            },
            move |_| async move {
                save_counter.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
        )
        .await
        .unwrap();

        assert_eq!(transcript, "daun cabai saya menguning");
        assert_eq!(generate_calls.load(Ordering::SeqCst), 0);
        assert_eq!(save_calls.load(Ordering::SeqCst), 0);
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
