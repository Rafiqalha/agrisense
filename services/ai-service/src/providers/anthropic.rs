//! Anthropic Claude — implements TextGeneration

use async_trait::async_trait;
use super::{TextGeneration, GenerateRequest, GenerateResponse};

pub struct AnthropicProvider {
    api_key: String,
    client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new(api_key: String) -> Self {
        Self { api_key, client: reqwest::Client::new() }
    }
}

#[async_trait]
impl TextGeneration for AnthropicProvider {
    fn provider_name(&self) -> &str { "anthropic" }
    async fn generate(&self, _r: GenerateRequest) -> anyhow::Result<GenerateResponse> { anyhow::bail!("Not implemented") }
    async fn classify_intent(&self, _m: &str, _c: &str) -> anyhow::Result<String> { anyhow::bail!("Not implemented") }
}
