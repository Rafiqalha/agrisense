pub mod gemini;
pub mod openai;
pub mod anthropic;
pub mod local;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Core AI provider abstraction.
/// All AI functionality in AgriSense routes through this trait.
/// Adding a new model = implementing this trait. Nothing else changes.
#[async_trait]
pub trait AiProvider {
    /// Provider identifier
    fn name(&self) -> &str;

    /// Text generation / chat completion
    async fn generate(&self, request: GenerateRequest) -> anyhow::Result<GenerateResponse>;

    /// Generate embeddings for RAG
    async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>>;

    /// Classify intent from text (structured output)
    async fn classify_intent(&self, message: &str, context: &str) -> anyhow::Result<String>;

    /// Analyze image (vision)
    async fn analyze_image(&self, image_url: &str, prompt: &str) -> anyhow::Result<String>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateRequest {
    pub system_prompt: Option<String>,
    pub messages: Vec<ChatMessage>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateResponse {
    pub content: String,
    pub model: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub latency_ms: u64,
}
