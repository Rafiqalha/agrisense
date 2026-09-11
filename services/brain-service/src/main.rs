use anyhow::Result;
use axum::{extract::DefaultBodyLimit, extract::State, http::StatusCode};
use std::sync::Arc;
use tracing::info;

mod activity;
mod agents;
mod audit;
mod config;
mod context;
mod intents;
mod mcp;
mod memory;
mod onboarding;
mod orchestrator;
mod workflows;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    let cfg = config::AppConfig::load()?;
    if std::env::var("APP_ENV").as_deref() == Ok("production") {
        anyhow::ensure!(
            cfg.database_url.starts_with("postgres://agrisense_brain:"),
            "brain-service must use the dedicated agrisense_brain database role in production"
        );
        anyhow::ensure!(
            !cfg.database_url.contains("brain-local-only"),
            "BRAIN_DB_PASSWORD must be replaced before production"
        );
    }
    let gateway_token = shared_auth::InternalToken::new(cfg.gateway_brain_token.clone())?;
    let ai_token = shared_auth::InternalToken::new(cfg.brain_ai_token.clone())?;
    let farm_token = shared_auth::InternalToken::new(cfg.brain_farm_token.clone())?;
    let mcp_registration_token =
        shared_auth::InternalToken::new(cfg.mcp_registration_token.clone())?;
    anyhow::ensure!(
        cfg.jwt_secret.len() >= 32,
        "JWT_SECRET must contain at least 32 bytes"
    );
    if std::env::var("APP_ENV").as_deref() == Ok("production") {
        anyhow::ensure!(
            cfg.jwt_secret != "change-me-in-production-use-256-bit-random",
            "JWT_SECRET must be replaced before production"
        );
    }
    let user_auth = shared_auth::AuthService::new(&cfg.jwt_secret);

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

    info!(
        version = env!("CARGO_PKG_VERSION"),
        "Starting brain-service"
    );

    // ── Infrastructure ─────────────────────────────────────────────────────
    let db = shared_db::create_pool(&cfg.database_url, cfg.database_max_connections).await?;
    let cache = shared_cache::CacheClient::new(&cfg.redis_url).await?;
    let nats = async_nats::connect(&cfg.nats_url).await?;

    info!("Connected to PostgreSQL, Redis, NATS");

    // ── MCP Registry ───────────────────────────────────────────────────────
    let mcp_registry = mcp::registry::McpRegistry::new();
    // Builtin tools (executed in-process by brain-service). Domain services
    // register their own tools at startup via POST /mcp/register.
    mcp_registry
        .register_service(shared_mcp::McpServiceManifest {
            service_name: "farm-service".into(),
            service_version: env!("CARGO_PKG_VERSION").into(),
            tools: mcp::tools::builtin_tools(),
            endpoint: "builtin://brain-service".into(),
        })
        .await;
    info!(
        tools = mcp_registry.list_tools().await.len(),
        "Builtin MCP tools registered"
    );

    // ── Orchestrator ───────────────────────────────────────────────────────
    let orchestrator = Arc::new(orchestrator::Orchestrator::new(
        mcp_registry,
        cache,
        nats.clone(),
        db,
        orchestrator::ServiceEndpoints {
            ai: cfg.ai_service_url.clone(),
            farm: cfg.farm_service_url.clone(),
        },
        memory::MemoryPolicy::new(
            cfg.conversation_memory_ttl_seconds,
            cfg.conversation_memory_max_messages,
            cfg.conversation_memory_max_chars,
        )?,
        orchestrator::SecurityContext {
            gateway_token,
            ai_token,
            farm_token,
            mcp_registration_token,
            user_auth,
        },
    ));

    // ── HTTP Server ────────────────────────────────────────────────────────
    let app = axum::Router::new()
        .route("/health", axum::routing::get(|| async { "ok" }))
        .route("/live", axum::routing::get(|| async { "ok" }))
        .route("/ready", axum::routing::get(readiness))
        .nest("/mcp", mcp::router())
        .nest("/orchestrate", orchestrator::router())
        .layer(DefaultBodyLimit::max(16 * 1024 * 1024))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(orchestrator);

    let addr = format!("0.0.0.0:{}", cfg.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!(addr = %addr, "brain-service listening");

    axum::serve(listener, app).await?;
    Ok(())
}

async fn readiness(State(orch): State<orchestrator::SharedOrchestrator>) -> StatusCode {
    let (database_ready, redis_ready) =
        tokio::join!(shared_db::is_ready(&orch.db), orch.cache.is_ready(),);
    let nats_ready = orch.nats.connection_state() == async_nats::connection::State::Connected;

    if database_ready && redis_ready && nats_ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}
