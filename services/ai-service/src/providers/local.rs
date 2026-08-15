//! Local provider — Ollama-compatible.
//! Implements TextGeneration + EmbeddingService for self-hosted models.

use async_trait::async_trait;
use super::{TextGeneration, EmbeddingService, GenerateRequest, GenerateResponse};

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
impl TextGeneration for LocalProvider {
    fn provider_name(&self) -> &str { "local" }
    async fn generate(&self, _r: GenerateRequest) -> anyhow::Result<GenerateResponse> { anyhow::bail!("Not implemented") }
    async fn classify_intent(&self, _m: &str, _c: &str) -> anyhow::Result<String> { anyhow::bail!("Not implemented") }
}

#[async_trait]
impl EmbeddingService for LocalProvider {
    fn provider_name(&self) -> &str { "local-embedding" }
    async fn embed(&self, _text: &str) -> anyhow::Result<Vec<f32>> { anyhow::bail!("Not implemented") }
    async fn embed_batch(&self, _texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> { anyhow::bail!("Not implemented") }
    fn dimension(&self) -> usize { 4096 }  // typical Llama embedding dim
}
