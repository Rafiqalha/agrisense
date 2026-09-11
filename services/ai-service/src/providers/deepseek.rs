//! DeepSeek Chat Completions: https://api-docs.deepseek.com/guides/vision/
use super::{
    ChatMessage, GenerateRequest, GenerateResponse, TextGeneration, VisionAnalysisType,
    VisionRequest, VisionResponse, VisionService,
};
use anyhow::{ensure, Context};
use async_trait::async_trait;
use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

pub const VISION_MODEL: &str = "deepseek-v4-flash-vision-exp";

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct InvalidInput(pub &'static str);

#[derive(Clone)]
pub struct DeepSeekProvider {
    api_key: String,
    model: String,
    endpoint: String,
    client: reqwest::Client,
}

impl DeepSeekProvider {
    pub fn new(api_key: String, model: String) -> anyhow::Result<Self> {
        ensure!(
            !api_key.trim().is_empty(),
            "DEEPSEEK_API_KEY must be configured"
        );
        ensure!(
            api_key.trim() == api_key,
            "DEEPSEEK_API_KEY contains surrounding whitespace"
        );
        ensure!(
            model == VISION_MODEL,
            "DEEPSEEK_MODEL must be deepseek-v4-flash-vision-exp for image support"
        );
        Ok(Self {
            api_key,
            model,
            endpoint: "https://api.deepseek.com/chat/completions".into(),
            client: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(5))
                // Leave headroom within the gateway's 45-second delivery timeout.
                .timeout(Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
        })
    }

    fn request_body(&self, request: GenerateRequest) -> anyhow::Result<Value> {
        if request.messages.is_empty() {
            return Err(InvalidInput("messages must not be empty").into());
        }
        if request.image_urls.len() > 4 {
            return Err(InvalidInput("at most 4 images are supported per request").into());
        }
        if request
            .temperature
            .is_some_and(|t| !t.is_finite() || !(0.0..=2.0).contains(&t))
        {
            return Err(InvalidInput("temperature must be between 0 and 2").into());
        }
        if request.max_tokens == Some(0) {
            return Err(InvalidInput("max_tokens must be positive").into());
        }
        let mut messages = Vec::new();
        if let Some(system) = request.system_prompt {
            messages.push(json!({"role": "system", "content": system}));
        }
        for message in request.messages {
            if !matches!(message.role.as_str(), "user" | "assistant" | "system") {
                return Err(InvalidInput("unsupported message role").into());
            }
            messages.push(json!({"role": message.role, "content": message.content}));
        }
        if !request.image_urls.is_empty() {
            let last = messages.last_mut().context("missing message")?;
            if last["role"] != "user" {
                return Err(
                    InvalidInput("images must be attached to the final user message").into(),
                );
            }
            let mut content = vec![json!({"type": "text", "text": last["content"]})];
            for image in request.image_urls {
                validate_image(&image)?;
                content.push(json!({
                    "type": "image_url",
                    "image_url": {"url": image, "detail": "original"}
                }));
            }
            last["content"] = Value::Array(content);
        }
        Ok(json!({
            "model": self.model,
            "messages": messages,
            "stream": false,
            "thinking": {"type": "disabled"},
            "temperature": request.temperature.unwrap_or(0.3),
            "max_tokens": request.max_tokens.unwrap_or(2048)
        }))
    }
}

fn validate_image(source: &str) -> anyhow::Result<()> {
    if let Some(data) = source.strip_prefix("data:") {
        let (mime, encoded) = data
            .split_once(";base64,")
            .ok_or(InvalidInput("image must be a base64 data URI"))?;
        if !matches!(
            mime,
            "image/jpeg" | "image/png" | "image/gif" | "image/webp"
        ) {
            return Err(InvalidInput(
                "supported images: JPEG, PNG, GIF, WebP; audio is unsupported",
            )
            .into());
        }
        // Stricter local limit keeps requests within the internal 16 MiB body limit.
        if encoded.is_empty()
            || encoded.len() > 12 * 1024 * 1024
            || base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .is_err()
        {
            return Err(InvalidInput("invalid or oversized base64 image").into());
        }
        return Ok(());
    }
    let url = reqwest::Url::parse(source)
        .map_err(|_| InvalidInput("image must be an HTTP(S) URL or base64 data URI"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || source.len() > 8192
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(InvalidInput("invalid image URL; local file paths are unsupported").into());
    }
    // The provider fetches public URLs; this service never reads arbitrary local files.
    Ok(())
}

#[derive(Deserialize)]
struct Completion {
    model: String,
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Usage,
}

#[derive(Deserialize)]
struct Choice {
    message: CompletionMessage,
    finish_reason: String,
}

#[derive(Deserialize)]
struct CompletionMessage {
    content: Option<String>,
}

#[derive(Default, Deserialize)]
struct Usage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
}

#[async_trait]
impl TextGeneration for DeepSeekProvider {
    fn provider_name(&self) -> &str {
        "deepseek"
    }

    async fn generate(&self, request: GenerateRequest) -> anyhow::Result<GenerateResponse> {
        let body = self.request_body(request)?;
        let started = Instant::now();
        let response = self
            .client
            .post(&self.endpoint)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .context("DeepSeek request failed")?;
        // Never expose provider response bodies, which could echo user data or credentials.
        ensure!(
            response.status().is_success(),
            "DeepSeek returned HTTP {}",
            response.status().as_u16()
        );
        let completion: Completion = response.json().await.context("Invalid DeepSeek response")?;
        let choice = completion
            .choices
            .into_iter()
            .next()
            .context("DeepSeek returned no choices")?;
        ensure!(
            choice.finish_reason == "stop",
            "DeepSeek completion did not finish normally"
        );
        let content = choice
            .message
            .content
            .filter(|text| !text.trim().is_empty())
            .context("DeepSeek returned empty content")?;
        Ok(GenerateResponse {
            provider: "deepseek".into(),
            content,
            model: completion.model,
            input_tokens: completion.usage.prompt_tokens,
            output_tokens: completion.usage.completion_tokens,
            latency_ms: started.elapsed().as_millis() as u64,
        })
    }

    async fn classify_intent(&self, message: &str, context: &str) -> anyhow::Result<String> {
        const INTENTS: &[&str] = &[
            "CHECK_STOCK",
            "REPORT_DISEASE",
            "ASK_WEATHER",
            "RECORD_EXPENSE",
            "RECORD_REVENUE",
            "CHECK_HARVEST_STATUS",
            "ASK_FERTILIZER_RECOMMENDATION",
            "REQUEST_LOAN",
            "CHECK_CREDIT_SCORE",
            "BUY_PRODUCT",
            "CHECK_PRICE",
            "REPORT_ACTIVITY",
            "UNKNOWN",
        ];
        let response = self
            .generate(GenerateRequest {
                system_prompt: Some(format!(
                    "Classify the agricultural message. Return ONLY one intent: {}.",
                    INTENTS.join(", ")
                )),
                messages: vec![ChatMessage {
                    role: "user".into(),
                    content: format!("Context: {context}\nMessage: {message}"),
                }],
                temperature: Some(0.0),
                max_tokens: Some(64),
                image_urls: vec![],
            })
            .await?;
        let intent = response.content.trim();
        Ok(if INTENTS.contains(&intent) {
            intent
        } else {
            "UNKNOWN"
        }
        .into())
    }
}

#[async_trait]
impl VisionService for DeepSeekProvider {
    fn provider_name(&self) -> &str {
        "deepseek"
    }

    async fn analyze_image(&self, request: VisionRequest) -> anyhow::Result<VisionResponse> {
        let task = match request.analysis_type {
            VisionAnalysisType::DiseaseDetection => {
                "Identify possible crop diseases and visible symptoms."
            }
            VisionAnalysisType::PestIdentification => "Identify visible pests and crop damage.",
            VisionAnalysisType::NutrientDeficiency => {
                "Describe visible signs of possible nutrient deficiency."
            }
            VisionAnalysisType::HarvestReadiness => "Assess visible signs of harvest readiness.",
            VisionAnalysisType::General => "Describe the image and answer the question.",
        };
        let response = self.generate(GenerateRequest {
            system_prompt: Some(format!("{task} Respond in Indonesian. State visual uncertainty; do not invent measurements or claim a definitive diagnosis from an image alone.")),
            messages: vec![ChatMessage { role: "user".into(), content: request.prompt }],
            image_urls: vec![request.image_url],
            temperature: Some(0.3),
            max_tokens: Some(2048),
        }).await?;
        let structured = serde_json::from_str::<Value>(&response.content).ok();
        // This model does not provide a calibrated confidence score.
        Ok(VisionResponse {
            analysis: response.content,
            structured_result: structured,
            confidence: 0.0,
            model: response.model,
            latency_ms: response.latency_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        extract::State,
        http::{HeaderMap, StatusCode},
        routing::post,
        Json, Router,
    };
    use std::sync::{Arc, Mutex};

    type Captured = Arc<Mutex<Option<(String, Value)>>>;

    async fn mock(
        status: StatusCode,
        response: Value,
    ) -> (DeepSeekProvider, Captured, tokio::task::JoinHandle<()>) {
        let captured: Captured = Arc::new(Mutex::new(None));
        let app = Router::new()
            .route(
                "/chat/completions",
                post(
                    move |State(captured): State<Captured>,
                          headers: HeaderMap,
                          Json(body): Json<Value>| {
                        let response = response.clone();
                        async move {
                            *captured.lock().unwrap() =
                                Some((headers["authorization"].to_str().unwrap().to_owned(), body));
                            (status, Json(response))
                        }
                    },
                ),
            )
            .with_state(captured.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut provider = DeepSeekProvider::new("test-key".into(), VISION_MODEL.into()).unwrap();
        provider.endpoint = format!("http://{}/chat/completions", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (provider, captured, task)
    }

    fn request() -> GenerateRequest {
        GenerateRequest {
            system_prompt: Some("Jawab dalam bahasa Indonesia.".into()),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: "Apa ini?".into(),
            }],
            temperature: Some(0.2),
            max_tokens: Some(100),
            image_urls: vec![],
        }
    }

    fn completion(content: Value, finish: &str) -> Value {
        json!({"model": VISION_MODEL, "choices": [{"message": {"content": content}, "finish_reason": finish}],
            "usage": {"prompt_tokens": 13, "completion_tokens": 7}})
    }

    #[tokio::test]
    async fn text_request_preserves_roles_auth_and_usage() {
        let (provider, captured, task) =
            mock(StatusCode::OK, completion(json!("Tanaman padi."), "stop")).await;
        let response = provider.generate(request()).await.unwrap();
        let (auth, body) = captured.lock().unwrap().clone().unwrap();
        assert_eq!(auth, "Bearer test-key");
        assert_eq!(body["model"], VISION_MODEL);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["content"], "Apa ini?");
        assert_eq!(body["thinking"]["type"], "disabled");
        assert_eq!((response.input_tokens, response.output_tokens), (13, 7));
        assert_eq!(response.content, "Tanaman padi.");
        task.abort();
    }

    #[tokio::test]
    async fn vision_sends_real_image_block_and_reports_actual_model() {
        let (provider, captured, task) =
            mock(StatusCode::OK, completion(json!("Daun."), "stop")).await;
        let image = "data:image/png;base64,aW1hZ2U=";
        let response = provider
            .analyze_image(VisionRequest {
                image_url: image.into(),
                prompt: "Periksa daun".into(),
                analysis_type: VisionAnalysisType::DiseaseDetection,
            })
            .await
            .unwrap();
        let (_, body) = captured.lock().unwrap().clone().unwrap();
        assert_eq!(body["messages"][1]["role"], "user");
        assert_eq!(body["messages"][1]["content"][1]["type"], "image_url");
        assert_eq!(body["messages"][1]["content"][1]["image_url"]["url"], image);
        assert_eq!(response.model, VISION_MODEL);
        assert_eq!(response.analysis, "Daun.");
        task.abort();
    }

    #[tokio::test]
    async fn provider_errors_empty_and_truncated_responses_are_not_success() {
        for (status, body) in [
            (
                StatusCode::UNAUTHORIZED,
                json!({"error": "secret-provider-body"}),
            ),
            (
                StatusCode::TOO_MANY_REQUESTS,
                json!({"error": "rate limited"}),
            ),
            (StatusCode::OK, json!({"choices": []})),
            (StatusCode::OK, completion(json!(""), "stop")),
            (StatusCode::OK, completion(Value::Null, "stop")),
            (StatusCode::OK, completion(json!("Incomplete"), "length")),
        ] {
            let (provider, _, task) = mock(status, body).await;
            let error = provider.generate(request()).await.unwrap_err().to_string();
            assert!(!error.contains("secret-provider-body"));
            task.abort();
        }
    }

    #[test]
    fn invalid_media_and_wrong_roles_are_rejected() {
        for source in [
            "/etc/passwd",
            "file:///etc/passwd",
            "data:audio/ogg;base64,YQ==",
            "data:image/png;base64,!",
            "data:image/png;base64,",
        ] {
            assert!(validate_image(source).is_err(), "{source}");
        }
        assert!(validate_image("https://example.com/leaf.jpg").is_ok());
        let provider = DeepSeekProvider::new("test-key".into(), VISION_MODEL.into()).unwrap();
        let mut req = request();
        req.messages[0].role = "assistant".into();
        req.image_urls.push("https://example.com/leaf.jpg".into());
        assert!(provider.request_body(req).is_err());
    }

    #[test]
    fn credentials_and_vision_model_are_required() {
        assert!(DeepSeekProvider::new(" ".into(), VISION_MODEL.into()).is_err());
        assert!(DeepSeekProvider::new("test".into(), "deepseek-v4-flash".into()).is_err());
    }
}
