use serde::Deserialize;

#[derive(Debug, Deserialize)]
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
}

fn default_port() -> u16 { 3002 }
fn default_log_level() -> String { "info".into() }
fn default_log_format() -> String { "pretty".into() }
fn default_db_connections() -> u32 { 10 }

impl AppConfig {
    pub fn load() -> anyhow::Result<Self> {
        let cfg = config::Config::builder()
            .add_source(config::Environment::default().separator("_"))
            .build()?;
        Ok(cfg.try_deserialize()?)
    }
}
