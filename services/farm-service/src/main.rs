use anyhow::Result;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    shared_observability::init(shared_observability::ObservabilityConfig {
        service_name: "farm-service".into(),
        log_level: std::env::var("LOG_LEVEL").unwrap_or_else(|_| "info".into()),
        log_format: if std::env::var("LOG_FORMAT").unwrap_or_default() == "json" {
            shared_observability::LogFormat::Json
        } else {
            shared_observability::LogFormat::Pretty
        },
        otlp_endpoint: std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok(),
    });

    info!(version = env!("CARGO_PKG_VERSION"), "Starting farm-service");

    let database_url = std::env::var("DATABASE_URL")?;
    let _db = shared_db::create_pool(&database_url, 10).await?;
    info!("Connected to PostgreSQL");

    let app = axum::Router::new()
        .route("/health", axum::routing::get(|| async { "ok" }))
        .layer(tower_http::trace::TraceLayer::new_for_http());

    let port = std::env::var("farm_service_PORT")
        .unwrap_or_else(|_| "3003".into());
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!(addr = %addr, "farm-service listening");
    axum::serve(listener, app).await?;
    Ok(())
}
