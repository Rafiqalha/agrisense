use anyhow::Result;
use tracing::info;

mod providers;
mod vision;
mod speech;
mod rag;
mod embeddings;
mod moderation;
mod prompts;
mod config;

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

    info!(version = env!("CARGO_PKG_VERSION"), provider = %cfg.ai_provider, "Starting ai-service");

    // Build the active provider based on config
    let provider: Box<dyn providers::AiProvider + Send + Sync> = match cfg.ai_provider.as_str() {
        "gemini"    => Box::new(providers::gemini::GeminiProvider::new(cfg.gemini_api_key.clone(), cfg.gemini_model.clone())),
        "openai"    => Box::new(providers::openai::OpenAiProvider::new(cfg.openai_api_key.clone())),
        "anthropic" => Box::new(providers::anthropic::AnthropicProvider::new(cfg.anthropic_api_key.clone())),
        "local"     => Box::new(providers::local::LocalProvider::new(cfg.local_model_url.clone())),
        other       => anyhow::bail!("Unknown AI provider: {}", other),
    };

    info!(provider = provider.name(), "AI provider initialized");

    let app = axum::Router::new()
        .route("/health", axum::routing::get(|| async { "ok" }))
        .route("/classify", axum::routing::post(|| async { "TODO" }))
        .route("/vision/analyze", axum::routing::post(|| async { "TODO" }))
        .route("/speech/transcribe", axum::routing::post(|| async { "TODO" }))
        .route("/rag/query", axum::routing::post(|| async { "TODO" }))
        .route("/embeddings/generate", axum::routing::post(|| async { "TODO" }));

    let addr = format!("0.0.0.0:{}", cfg.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!(addr = %addr, "ai-service listening");
    axum::serve(listener, app).await?;
    Ok(())
}
