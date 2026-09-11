//! OpenAI provider — implements TextGeneration.
//! Also hosts Whisper for SpeechService.

use super::{
    GenerateRequest, GenerateResponse, SpeechService, TextGeneration, TranscribeRequest,
    TranscribeResponse,
};
use async_trait::async_trait;

pub struct OpenAiProvider {
    api_key: String,
    client: reqwest::Client,
}

impl OpenAiProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl TextGeneration for OpenAiProvider {
    fn provider_name(&self) -> &str {
        "openai"
    }

    async fn generate(&self, _request: GenerateRequest) -> anyhow::Result<GenerateResponse> {
        // TODO: implement OpenAI chat completion
        anyhow::bail!("OpenAI TextGeneration not yet implemented")
    }

    async fn classify_intent(&self, _message: &str, _context: &str) -> anyhow::Result<String> {
        anyhow::bail!("OpenAI classify not yet implemented")
    }
}

/// Whisper STT — primary speech-to-text for WhatsApp voice notes
pub struct WhisperProvider {
    api_key: String,
    client: reqwest::Client,
}

impl WhisperProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl SpeechService for WhisperProvider {
    fn provider_name(&self) -> &str {
        "whisper"
    }

    async fn transcribe(&self, request: TranscribeRequest) -> anyhow::Result<TranscribeResponse> {
        // Download audio from URL
        let audio_bytes = self
            .client
            .get(&request.audio_url)
            .send()
            .await?
            .bytes()
            .await?;

        // Send to Whisper API
        let form = reqwest::multipart::Form::new()
            .text("model", "whisper-1")
            .text("language", request.language.unwrap_or_else(|| "id".into()))
            .text("response_format", "verbose_json")
            .part(
                "file",
                reqwest::multipart::Part::bytes(audio_bytes.to_vec())
                    .file_name("voice_note.ogg")
                    .mime_str("audio/ogg")?,
            );

        let start = std::time::Instant::now();
        let resp: serde_json::Value = self
            .client
            .post("https://api.openai.com/v1/audio/transcriptions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .multipart(form)
            .send()
            .await?
            .json()
            .await?;
        let latency_ms = start.elapsed().as_millis() as u64;

        Ok(TranscribeResponse {
            transcript: resp["text"].as_str().unwrap_or("").to_string(),
            language: resp["language"].as_str().unwrap_or("id").to_string(),
            confidence: 0.95, // Whisper doesn't return confidence per-segment easily
            duration_seconds: resp["duration"].as_f64().unwrap_or(0.0) as f32,
            model: "whisper-1".into(),
            latency_ms,
        })
    }
}
