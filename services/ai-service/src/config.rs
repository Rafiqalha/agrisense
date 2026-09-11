use serde::Deserialize;

#[derive(Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_log_format")]
    pub log_format: String,
    #[serde(default = "default_provider")]
    pub ai_provider: String,
    #[serde(default)]
    pub gemini_api_key: String,
    #[serde(default = "default_model")]
    pub gemini_model: String,
    pub brain_ai_token: String,
}

fn default_port() -> u16 {
    3008
}
fn default_log_level() -> String {
    "info".into()
}
fn default_log_format() -> String {
    "pretty".into()
}
fn default_provider() -> String {
    "gemini".into()
}
fn default_model() -> String {
    "gemini-3.6-flash".into()
}

impl AppConfig {
    pub fn load() -> anyhow::Result<Self> {
        let mut cfg: Self = config::Config::builder()
            .add_source(config::Environment::default().try_parsing(true))
            .build()?
            .try_deserialize()?;
        if let Ok(port) = std::env::var("AI_SERVICE_PORT") {
            cfg.port = port.parse()?;
        }
        Ok(cfg)
    }
}
