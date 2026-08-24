use anyhow::Result;
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;
use std::sync::Arc;
use tracing::info;

mod providers;
mod cache;
mod vision;
mod speech;
mod rag;
mod embeddings;
mod moderation;
mod prompts;
mod config;

use providers::{AiCapabilities, TextGeneration, VisionService, SpeechService, EmbeddingService, SafetyService};

type SharedCapabilities = Arc<AiCapabilities>;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let cfg = config::AppConfig::load()?;

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

    // ── Build AI Capabilities ──────────────────────────────────────────────
    //
    // Each CAPABILITY can have a DIFFERENT provider.
    // This is the key insight:
    //   text:      Gemini (or OpenAI, or Anthropic, or Local)
    //   vision:    Gemini (multimodal)
    //   speech:    Whisper (OpenAI) — Gemini doesn't do STT well
    //   embedding: Gemini text-embedding-004
    //   safety:    Llama Guard (local) or NoOp (dev)
    //

    // Text generation — configurable via AI_PROVIDER env
    let text: Box<dyn TextGeneration> = match cfg.ai_provider.as_str() {
        "gemini"    => Box::new(providers::gemini::GeminiProvider::new(cfg.gemini_api_key.clone(), cfg.gemini_model.clone())),
        "openai"    => Box::new(providers::openai::OpenAiProvider::new(cfg.openai_api_key.clone())),
        "anthropic" => Box::new(providers::anthropic::AnthropicProvider::new(cfg.anthropic_api_key.clone())),
        "local"     => Box::new(providers::local::LocalProvider::new(cfg.local_model_url.clone())),
        other       => anyhow::bail!("Unknown AI provider: {}", other),
    };
    info!(provider = text.provider_name(), "Text generation initialized");

    // Vision — always Gemini (best multimodal for agriculture)
    let vision: Box<dyn VisionService> = Box::new(
        providers::gemini::GeminiProvider::new(cfg.gemini_api_key.clone(), "gemini-2.0-flash".into())
    );
    info!(provider = vision.provider_name(), "Vision initialized");

    // Speech — Whisper (via OpenAI API)
    let speech: Box<dyn SpeechService> = Box::new(
        providers::openai::WhisperProvider::new(cfg.openai_api_key.clone())
    );
    info!(provider = speech.provider_name(), "Speech-to-text initialized");

    // Embeddings — Gemini text-embedding-004
    let embedding: Box<dyn EmbeddingService> = Box::new(
        providers::gemini::GeminiProvider::new(cfg.gemini_api_key.clone(), "text-embedding-004".into())
    );
    info!(provider = embedding.provider_name(), dim = embedding.dimension(), "Embeddings initialized");

    // Safety — Llama Guard (local Ollama) or NoOp for development
    let safety: Box<dyn SafetyService> = if cfg.safety_enabled {
        Box::new(providers::safety::LlamaGuardProvider::new(cfg.local_model_url.clone()))
    } else {
        info!("Safety moderation DISABLED (dev mode)");
        Box::new(providers::safety::NoOpSafetyProvider)
    };
    info!(provider = safety.provider_name(), "Safety initialized");

    let capabilities = Arc::new(AiCapabilities { text, vision, speech, embedding, safety });
    info!("All AI capabilities initialized");

    // ── HTTP Server ────────────────────────────────────────────────────────
    let app = Router::new()
        .route("/health", axum::routing::get(|| async { "ok" }))
        // Capability endpoints
        .route("/generate", axum::routing::post(generate_handler))
        .route("/classify", axum::routing::post(classify_handler))
        .route("/vision/analyze", axum::routing::post(vision_handler))
        .route("/speech/transcribe", axum::routing::post(|| async { "TODO: voice note transcription" }))
        .route("/embeddings/generate", axum::routing::post(embeddings_handler))
        .route("/safety/moderate", axum::routing::post(|| async { "TODO: content moderation" }))
        // RAG
        .route("/rag/query", axum::routing::post(|| async { "TODO: RAG retrieval" }))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(capabilities);

    let addr = format!("0.0.0.0:{}", cfg.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!(addr = %addr, "ai-service listening");
    axum::serve(listener, app).await?;
    Ok(())
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
    let intent = caps.text.classify_intent(&req.message, &req.context).await?;
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

#[derive(Debug, Deserialize)]
struct EmbeddingsHttpRequest {
    texts: Vec<String>,
}

async fn embeddings_handler(
    State(caps): State<SharedCapabilities>,
    Json(req): Json<EmbeddingsHttpRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut embeddings = Vec::with_capacity(req.texts.len());
    for text in &req.texts {
        embeddings.push(caps.embedding.embed(text).await?);
    }
    Ok(Json(serde_json::json!({ "embeddings": embeddings })))
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
        tracing::error!(error = %self.0, "ai-service request failed");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": self.0.to_string() }).to_string(),
        )
            .into_response()
    }
}
