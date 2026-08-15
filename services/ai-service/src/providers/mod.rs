pub mod gemini;
pub mod openai;
pub mod anthropic;
pub mod local;
pub mod safety;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

// ─── Capability-Based AI Abstraction ──────────────────────────────────────────
//
// Instead of a monolith AiProvider that does everything:
//
//   ❌ trait AiProvider { generate, embed, vision, speech, moderate }
//
// We separate by CAPABILITY:
//
//   ✅ trait TextGeneration { generate, classify_intent }
//   ✅ trait VisionService { analyze_image }
//   ✅ trait SpeechService { transcribe }
//   ✅ trait EmbeddingService { embed }
//   ✅ trait SafetyService { moderate }
//
// Why? Because:
//   - Gemini handles text + vision
//   - Whisper handles speech (not Gemini)
//   - Llama Guard handles safety (not Gemini)
//   - pgvector handles local embeddings (optionally Gemini)
//
// Each capability can have a DIFFERENT provider behind it.
// An agent calls VisionService, not GeminiProvider.

// ─── Text Generation ──────────────────────────────────────────────────────────

#[async_trait]
pub trait TextGeneration: Send + Sync {
    fn provider_name(&self) -> &str;

    /// Chat completion / text generation
    async fn generate(&self, request: GenerateRequest) -> anyhow::Result<GenerateResponse>;

    /// Classify intent from raw text (structured output)
    async fn classify_intent(&self, message: &str, context: &str) -> anyhow::Result<String>;
}

// ─── Vision ───────────────────────────────────────────────────────────────────

#[async_trait]
pub trait VisionService: Send + Sync {
    fn provider_name(&self) -> &str;

    /// Analyze an image — disease detection, pest identification, etc.
    async fn analyze_image(&self, request: VisionRequest) -> anyhow::Result<VisionResponse>;
}

// ─── Speech (STT) ─────────────────────────────────────────────────────────────

#[async_trait]
pub trait SpeechService: Send + Sync {
    fn provider_name(&self) -> &str;

    /// Transcribe audio to text (voice notes from WhatsApp)
    async fn transcribe(&self, request: TranscribeRequest) -> anyhow::Result<TranscribeResponse>;
}

// ─── Embeddings ───────────────────────────────────────────────────────────────

#[async_trait]
pub trait EmbeddingService: Send + Sync {
    fn provider_name(&self) -> &str;

    /// Generate embedding vector for RAG / semantic search
    async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>>;

    /// Batch embed for bulk ingestion
    async fn embed_batch(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>>;

    /// Embedding dimension (e.g. 768 for text-embedding-004)
    fn dimension(&self) -> usize;
}

// ─── Safety / Moderation ──────────────────────────────────────────────────────

#[async_trait]
pub trait SafetyService: Send + Sync {
    fn provider_name(&self) -> &str;

    /// Check if content is safe to process/return
    async fn moderate(&self, request: ModerationRequest) -> anyhow::Result<ModerationResponse>;
}

// ─── Request / Response Types ─────────────────────────────────────────────────

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisionRequest {
    pub image_url: String,
    pub prompt: String,
    pub analysis_type: VisionAnalysisType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisionAnalysisType {
    DiseaseDetection,
    PestIdentification,
    NutrientDeficiency,
    HarvestReadiness,
    General,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisionResponse {
    pub analysis: String,
    pub structured_result: Option<serde_json::Value>,
    pub confidence: f32,
    pub model: String,
    pub latency_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscribeRequest {
    pub audio_url: String,
    pub language: Option<String>,    // default: "id" (Indonesian)
    pub format: Option<String>,      // "ogg", "mp3", "wav"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscribeResponse {
    pub transcript: String,
    pub language: String,
    pub confidence: f32,
    pub duration_seconds: f32,
    pub model: String,
    pub latency_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModerationRequest {
    pub content: String,
    pub content_type: ModerationContentType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModerationContentType {
    UserInput,
    AiOutput,
    ImageDescription,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModerationResponse {
    pub safe: bool,
    pub categories: Vec<ModerationCategory>,
    pub risk_level: String,      // "low", "medium", "high"
    pub model: String,
    pub latency_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModerationCategory {
    pub name: String,
    pub flagged: bool,
    pub score: f32,
}

// ─── Composite: AI Capabilities Bundle ────────────────────────────────────────
//
// For convenience, a service can hold all capabilities together.
// Each capability can be backed by a DIFFERENT provider.
//
// Example:
//   text:      GeminiProvider
//   vision:    GeminiProvider
//   speech:    WhisperProvider
//   embedding: GeminiProvider
//   safety:    LlamaGuardProvider
//

pub struct AiCapabilities {
    pub text: Box<dyn TextGeneration>,
    pub vision: Box<dyn VisionService>,
    pub speech: Box<dyn SpeechService>,
    pub embedding: Box<dyn EmbeddingService>,
    pub safety: Box<dyn SafetyService>,
}
