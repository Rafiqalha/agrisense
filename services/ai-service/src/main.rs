use anyhow::Result;
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

    let _capabilities = AiCapabilities { text, vision, speech, embedding, safety };
    info!("All AI capabilities initialized");

    // ── HTTP Server ────────────────────────────────────────────────────────
    let app = axum::Router::new()
        .route("/health", axum::routing::get(|| async { "ok" }))
        // Capability endpoints
        .route("/generate", axum::routing::post(|| async { "TODO: text generation" }))
        .route("/classify", axum::routing::post(|| async { "TODO: intent classification" }))
        .route("/vision/analyze", axum::routing::post(|| async { "TODO: image analysis" }))
        .route("/speech/transcribe", axum::routing::post(|| async { "TODO: voice note transcription" }))
        .route("/embeddings/generate", axum::routing::post(|| async { "TODO: embedding generation" }))
        .route("/safety/moderate", axum::routing::post(|| async { "TODO: content moderation" }))
        // RAG
        .route("/rag/query", axum::routing::post(|| async { "TODO: RAG retrieval" }))
        .layer(tower_http::trace::TraceLayer::new_for_http());

    let addr = format!("0.0.0.0:{}", cfg.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!(addr = %addr, "ai-service listening");
    axum::serve(listener, app).await?;
    Ok(())
}
