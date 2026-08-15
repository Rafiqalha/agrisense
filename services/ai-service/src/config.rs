use serde::Deserialize;

#[derive(Debug, Deserialize)]
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
    #[serde(default = "default_gemini_model")]
    pub gemini_model: String,
    #[serde(default)]
    pub openai_api_key: String,
    #[serde(default)]
    pub anthropic_api_key: String,
    #[serde(default = "default_local_url")]
    pub local_model_url: String,
    #[serde(default)]
    pub safety_enabled: bool,
}

fn default_port() -> u16 { 3008 }
fn default_log_level() -> String { "info".into() }
fn default_log_format() -> String { "pretty".into() }
fn default_provider() -> String { "gemini".into() }
fn default_gemini_model() -> String { "gemini-2.0-flash".into() }
fn default_local_url() -> String { "http://localhost:11434".into() }

impl AppConfig {
    pub fn load() -> anyhow::Result<Self> {
        let cfg = config::Config::builder()
            .add_source(config::Environment::default().separator("_"))
            .build()?;
        Ok(cfg.try_deserialize()?)
    }
}
