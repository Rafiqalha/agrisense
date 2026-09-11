use serde::Deserialize;

#[derive(Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_port")]
    pub port: u16,
    pub database_url: String,
    pub redis_url: String,
    pub nats_url: String,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_log_format")]
    pub log_format: String,
    pub otlp_endpoint: Option<String>,
    #[serde(default = "default_db_connections")]
    pub database_max_connections: u32,
    #[serde(default = "default_ai_service_url")]
    pub ai_service_url: String,
    #[serde(default = "default_farm_service_url")]
    pub farm_service_url: String,
    pub gateway_brain_token: String,
    pub brain_ai_token: String,
    pub brain_farm_token: String,
    pub mcp_registration_token: String,
    pub jwt_secret: String,
    #[serde(default = "default_conversation_memory_ttl_seconds")]
    pub conversation_memory_ttl_seconds: u64,
    #[serde(default = "default_conversation_memory_max_messages")]
    pub conversation_memory_max_messages: usize,
    #[serde(default = "default_conversation_memory_max_chars")]
    pub conversation_memory_max_chars: usize,
}

fn default_port() -> u16 {
    3002
}
fn default_log_level() -> String {
    "info".into()
}
fn default_log_format() -> String {
    "pretty".into()
}
fn default_db_connections() -> u32 {
    10
}
fn default_ai_service_url() -> String {
    "http://localhost:3008".into()
}
fn default_farm_service_url() -> String {
    "http://localhost:3003".into()
}
fn default_conversation_memory_ttl_seconds() -> u64 {
    86_400
}
fn default_conversation_memory_max_messages() -> usize {
    8
}
fn default_conversation_memory_max_chars() -> usize {
    12_000
}

impl AppConfig {
    pub fn load() -> anyhow::Result<Self> {
        let mut cfg: Self = config::Config::builder()
            .add_source(config::Environment::default().try_parsing(true))
            .build()?
            .try_deserialize()?;
        if let Ok(port) = std::env::var("BRAIN_SERVICE_PORT") {
            cfg.port = port.parse()?;
        }
        anyhow::ensure!(
            (300..=604_800).contains(&cfg.conversation_memory_ttl_seconds),
            "CONVERSATION_MEMORY_TTL_SECONDS must be between 300 and 604800"
        );
        anyhow::ensure!(
            (2..=20).contains(&cfg.conversation_memory_max_messages),
            "CONVERSATION_MEMORY_MAX_MESSAGES must be between 2 and 20"
        );
        anyhow::ensure!(
            (1_000..=50_000).contains(&cfg.conversation_memory_max_chars),
            "CONVERSATION_MEMORY_MAX_CHARS must be between 1000 and 50000"
        );
        if std::env::var("APP_ENV").as_deref() == Ok("production") {
            anyhow::ensure!(
                cfg.brain_farm_token != "brain-farm-local-only-change-me-32bytes",
                "BRAIN_FARM_TOKEN must be replaced before production"
            );
        }
        Ok(cfg)
    }
}
