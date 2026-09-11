use anyhow::Result;
use axum::{
    extract::{DefaultBodyLimit, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;
use tracing::info;

mod cache;
mod config;
mod embeddings;
mod moderation;
mod prompts;
mod providers;
mod rag;
mod speech;
mod vision;

use providers::AiCapabilities;

type SharedCapabilities = Arc<AiCapabilities>;

#[derive(Clone)]
struct InternalBoundary {
    token: shared_auth::InternalToken,
    request_slots: Arc<tokio::sync::Semaphore>,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let cfg = config::AppConfig::load()?;
    let brain_token = shared_auth::InternalToken::new(cfg.brain_ai_token.clone())?;

    shared_observability::init(shared_observability::ObservabilityConfig {
        service_name: "ai-service".into(),
        log_level: cfg.log_level.clone(),
        log_format: if cfg.log_format == "json" {
            shared_observability::LogFormat::Json
        } else {
            shared_observability::LogFormat::Pretty
        },
        otlp_endpoint: None,
    });

    info!(version = env!("CARGO_PKG_VERSION"), "Starting ai-service");

    anyhow::ensure!(cfg.ai_provider == "gemini", "AI_PROVIDER must be gemini");
    let provider = providers::gemini::GeminiProvider::new(cfg.gemini_api_key, cfg.gemini_model)?;
    info!(
        provider = "gemini",
        "Text and vision initialized; speech, embeddings, moderation and RAG unavailable"
    );
    let capabilities = Arc::new(AiCapabilities {
        text: Box::new(provider.clone()),
        vision: Box::new(provider),
    });

    // ── HTTP Server ────────────────────────────────────────────────────────
    let protected = Router::new()
        // Capability endpoints
        .route("/generate", axum::routing::post(generate_handler))
        .route("/classify", axum::routing::post(classify_handler))
        .route("/vision/analyze", axum::routing::post(vision_handler))
        .route(
            "/speech/transcribe",
            axum::routing::post(unsupported_capability),
        )
        .route(
            "/embeddings/generate",
            axum::routing::post(unsupported_capability),
        )
        .route(
            "/safety/moderate",
            axum::routing::post(unsupported_capability),
        )
        // RAG
        .route("/rag/query", axum::routing::post(unsupported_capability))
        .route_layer(middleware::from_fn_with_state(
            InternalBoundary {
                token: brain_token,
                request_slots: Arc::new(tokio::sync::Semaphore::new(32)),
            },
            require_brain_service,
        ));
    let app = Router::new()
        .route("/health", axum::routing::get(|| async { "ok" }))
        .route("/live", axum::routing::get(|| async { "ok" }))
        .route("/ready", axum::routing::get(|| async { "ok" }))
        .merge(protected)
        .layer(DefaultBodyLimit::max(16 * 1024 * 1024))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(capabilities);

    let addr = format!("0.0.0.0:{}", cfg.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!(addr = %addr, "ai-service listening");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn require_brain_service(
    State(boundary): State<InternalBoundary>,
    request: Request,
    next: Next,
) -> Response {
    let authorization = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    if !boundary.token.authorize_header(authorization) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Ok(_permit) = boundary.request_slots.try_acquire() else {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    };
    next.run(request).await
}

// ─── Handlers ────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct GenerateHttpRequest {
    #[serde(default)]
    system_prompt: Option<String>,
    messages: Vec<providers::ChatMessage>,
    #[serde(default)]
    temperature: Option<f32>,
    #[serde(default)]
    max_tokens: Option<u32>,
    #[serde(default)]
    image_urls: Vec<String>,
}

async fn generate_handler(
    State(caps): State<SharedCapabilities>,
    Json(req): Json<GenerateHttpRequest>,
) -> Result<Json<providers::GenerateResponse>, ApiError> {
    let resp = caps
        .text
        .generate(providers::GenerateRequest {
            system_prompt: req.system_prompt,
            messages: req.messages,
            temperature: req.temperature,
            max_tokens: req.max_tokens,
            image_urls: req.image_urls,
        })
        .await?;
    Ok(Json(resp))
}

#[derive(Debug, Deserialize)]
struct ClassifyHttpRequest {
    message: String,
    #[serde(default)]
    context: String,
}

async fn classify_handler(
    State(caps): State<SharedCapabilities>,
    Json(req): Json<ClassifyHttpRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let intent = caps
        .text
        .classify_intent(&req.message, &req.context)
        .await?;
    Ok(Json(serde_json::json!({ "intent": intent })))
}

#[derive(Debug, Deserialize)]
struct VisionHttpRequest {
    image_url: String,
    prompt: String,
    #[serde(default = "default_analysis_type")]
    analysis_type: providers::VisionAnalysisType,
}

fn default_analysis_type() -> providers::VisionAnalysisType {
    providers::VisionAnalysisType::General
}

async fn vision_handler(
    State(caps): State<SharedCapabilities>,
    Json(req): Json<VisionHttpRequest>,
) -> Result<Json<providers::VisionResponse>, ApiError> {
    let resp = caps
        .vision
        .analyze_image(providers::VisionRequest {
            image_url: req.image_url,
            prompt: req.prompt,
            analysis_type: req.analysis_type,
        })
        .await?;
    Ok(Json(resp))
}

async fn unsupported_capability() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({
            "error": "capability_not_supported",
            "provider": "gemini",
            "message": "Deployment ini mendukung teks dan gambar; speech, embedding, moderation dan RAG belum tersedia."
        })),
    )
}

// ─── Error ───────────────────────────────────────────────────────────────────

struct ApiError(anyhow::Error);

impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        if let Some(error) = self.0.downcast_ref::<providers::gemini::InvalidInput>() {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": error.to_string() })),
            )
                .into_response();
        }
        tracing::error!(error = %self.0, "ai-service request failed");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": self.0.to_string() }).to_string(),
        )
            .into_response()
    }
}
