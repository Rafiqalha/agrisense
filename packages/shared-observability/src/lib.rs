use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

pub struct ObservabilityConfig {
    pub service_name: String,
    pub log_level: String,
    pub log_format: LogFormat,
    pub otlp_endpoint: Option<String>,
}

#[derive(Debug, Clone)]
pub enum LogFormat {
    Pretty,
    Json,
}

pub fn init(config: ObservabilityConfig) {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&config.log_level));

    match config.log_format {
        LogFormat::Json => {
            tracing_subscriber::registry()
                .with(filter)
                .with(tracing_subscriber::fmt::layer().json())
                .init();
        }
        LogFormat::Pretty => {
            tracing_subscriber::registry()
                .with(filter)
                .with(tracing_subscriber::fmt::layer().pretty())
                .init();
        }
    }

    tracing::info!(
        service = %config.service_name,
        "Observability initialized"
    );
}
