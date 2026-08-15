use async_trait::async_trait;
use super::{AiProvider, GenerateRequest, GenerateResponse};

pub struct OpenAiProvider {
    api_key: String,
    client: reqwest::Client,
}

impl OpenAiProvider {
    pub fn new(api_key: String) -> Self {
        Self { api_key, client: reqwest::Client::new() }
    }
}

#[async_trait]
impl AiProvider for OpenAiProvider {
    fn name(&self) -> &str { "openai" }

    async fn generate(&self, _request: GenerateRequest) -> anyhow::Result<GenerateResponse> {
        // TODO: implement OpenAI completion
        anyhow::bail!("OpenAI provider not yet implemented")
    }

    async fn embed(&self, _text: &str) -> anyhow::Result<Vec<f32>> {
        anyhow::bail!("OpenAI embed not yet implemented")
    }

    async fn classify_intent(&self, _message: &str, _context: &str) -> anyhow::Result<String> {
        anyhow::bail!("OpenAI classify not yet implemented")
    }

    async fn analyze_image(&self, _image_url: &str, _prompt: &str) -> anyhow::Result<String> {
        anyhow::bail!("OpenAI vision not yet implemented")
    }
}
