use async_trait::async_trait;
use super::{AiProvider, GenerateRequest, GenerateResponse};

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
impl AiProvider for AnthropicProvider {
    fn name(&self) -> &str { "anthropic" }
    async fn generate(&self, _r: GenerateRequest) -> anyhow::Result<GenerateResponse> { anyhow::bail!("Not implemented") }
    async fn embed(&self, _t: &str) -> anyhow::Result<Vec<f32>> { anyhow::bail!("Not implemented") }
    async fn classify_intent(&self, _m: &str, _c: &str) -> anyhow::Result<String> { anyhow::bail!("Not implemented") }
    async fn analyze_image(&self, _u: &str, _p: &str) -> anyhow::Result<String> { anyhow::bail!("Not implemented") }
}
