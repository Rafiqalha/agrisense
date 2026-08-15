use anyhow::Result;
use tracing::info;

mod config;
mod intents;
mod agents;
mod workflows;
mod memory;
mod orchestrator;
mod mcp;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    let cfg = config::AppConfig::load()?;

    shared_observability::init(shared_observability::ObservabilityConfig {
        service_name: "brain-service".into(),
        log_level: cfg.log_level.clone(),
        log_format: if cfg.log_format == "json" {
            shared_observability::LogFormat::Json
        } else {
            shared_observability::LogFormat::Pretty
        },
        otlp_endpoint: cfg.otlp_endpoint.clone(),
    });

    info!(version = env!("CARGO_PKG_VERSION"), "Starting brain-service");

    // ── Infrastructure ─────────────────────────────────────────────────────
    let db = shared_db::create_pool(&cfg.database_url, cfg.database_max_connections).await?;
    let cache = shared_cache::CacheClient::new(&cfg.redis_url).await?;
    let nats = async_nats::connect(&cfg.nats_url).await?;

    info!("Connected to PostgreSQL, Redis, NATS");

    // ── MCP Registry ───────────────────────────────────────────────────────
    let mcp_registry = mcp::registry::McpRegistry::new();
    // Services register their tools on startup via NATS or HTTP
    // brain-service acts as the central registry

    // ── Orchestrator ───────────────────────────────────────────────────────
    let orchestrator = orchestrator::Orchestrator::new(
        mcp_registry,
        cache,
        nats.clone(),
    );

    // ── HTTP Server ────────────────────────────────────────────────────────
    let app = axum::Router::new()
        .nest("/mcp", mcp::router())
        .nest("/orchestrate", orchestrator::router())
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .layer(tower_http::cors::CorsLayer::permissive())
        .with_state(orchestrator);

    let addr = format!("0.0.0.0:{}", cfg.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!(addr = %addr, "brain-service listening");

    axum::serve(listener, app).await?;
    Ok(())
}
