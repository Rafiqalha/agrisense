//! Safety / Content Moderation providers
//! - LlamaGuardProvider: local Llama Guard model via Ollama
//! - NeMoGuardrailsProvider: NVIDIA NeMo Guardrails (future)

use super::{ModerationCategory, ModerationRequest, ModerationResponse, SafetyService};
use async_trait::async_trait;

/// Llama Guard via local Ollama instance
pub struct LlamaGuardProvider {
    base_url: String,
    client: reqwest::Client,
}

impl LlamaGuardProvider {
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl SafetyService for LlamaGuardProvider {
    fn provider_name(&self) -> &str {
        "llama-guard"
    }

    async fn moderate(&self, request: ModerationRequest) -> anyhow::Result<ModerationResponse> {
        let prompt = format!(
            "[INST] Task: Check if there is unsafe content in the following message.\n\
            <BEGIN MESSAGE>\n{}\n<END MESSAGE>\n\
            Provide your safety assessment. If unsafe, specify the violated category. [/INST]",
            request.content
        );

        let body = serde_json::json!({
            "model": "llama-guard",
            "prompt": prompt,
            "stream": false
        });

        let start = std::time::Instant::now();
        let resp: serde_json::Value = self
            .client
            .post(format!("{}/api/generate", self.base_url))
            .json(&body)
            .send()
            .await?
            .json()
            .await?;
        let latency_ms = start.elapsed().as_millis() as u64;

        let response_text = resp["response"].as_str().unwrap_or("safe");
        let is_safe = response_text.to_lowercase().starts_with("safe");

        Ok(ModerationResponse {
            safe: is_safe,
            categories: if is_safe {
                vec![]
            } else {
                vec![ModerationCategory {
                    name: "unsafe_content".into(),
                    flagged: true,
                    score: 1.0,
                }]
            },
            risk_level: if is_safe { "low".into() } else { "high".into() },
            model: "llama-guard".into(),
            latency_ms,
        })
    }
}

/// Fallback: No-op safety (for development only)
pub struct NoOpSafetyProvider;

#[async_trait]
impl SafetyService for NoOpSafetyProvider {
    fn provider_name(&self) -> &str {
        "noop-safety"
    }

    async fn moderate(&self, _request: ModerationRequest) -> anyhow::Result<ModerationResponse> {
        Ok(ModerationResponse {
            safe: true,
            categories: vec![],
            risk_level: "low".into(),
            model: "noop".into(),
            latency_ms: 0,
        })
    }
}
