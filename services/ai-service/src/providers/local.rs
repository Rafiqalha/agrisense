//! Local provider — Ollama-compatible endpoint.
//! Use this for running Llama, Mistral, or other local models.

use async_trait::async_trait;
use super::{AiProvider, GenerateRequest, GenerateResponse};

pub struct LocalProvider {
    base_url: String,
    client: reqwest::Client,
}

impl LocalProvider {
    pub fn new(base_url: String) -> Self {
        Self { base_url, client: reqwest::Client::new() }
    }
}

#[async_trait]
impl AiProvider for LocalProvider {
    fn name(&self) -> &str { "local" }
    async fn generate(&self, _r: GenerateRequest) -> anyhow::Result<GenerateResponse> { anyhow::bail!("Not implemented") }
    async fn embed(&self, _t: &str) -> anyhow::Result<Vec<f32>> { anyhow::bail!("Not implemented") }
    async fn classify_intent(&self, _m: &str, _c: &str) -> anyhow::Result<String> { anyhow::bail!("Not implemented") }
    async fn analyze_image(&self, _u: &str, _p: &str) -> anyhow::Result<String> { anyhow::bail!("Not implemented") }
}
