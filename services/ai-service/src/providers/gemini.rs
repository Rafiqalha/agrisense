//! Gemini GenerateContent integration for text and image understanding.
//!
//! API reference: https://ai.google.dev/api/generate-content

use super::{
    ChatMessage, GenerateRequest, GenerateResponse, TextGeneration, VisionAnalysisType,
    VisionRequest, VisionResponse, VisionService,
};
use anyhow::{ensure, Context};
use async_trait::async_trait;
use base64::Engine;
use reqwest::Url;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::{
    net::IpAddr,
    time::{Duration, Instant},
};

pub const DEFAULT_MODEL: &str = "gemini-3.6-flash";
const MAX_IMAGES: usize = 4;
const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct InvalidInput(pub &'static str);

#[derive(Clone)]
pub struct GeminiProvider {
    api_key: String,
    model: String,
    endpoint: String,
    client: reqwest::Client,
}

impl GeminiProvider {
    pub fn new(api_key: String, model: String) -> anyhow::Result<Self> {
        ensure!(
            !api_key.trim().is_empty(),
            "GEMINI_API_KEY must be configured"
        );
        ensure!(
            api_key.trim() == api_key,
            "GEMINI_API_KEY contains surrounding whitespace"
        );
        ensure!(
            model.starts_with("gemini-")
                && model
                    .chars()
                    .all(|value| value.is_ascii_alphanumeric() || matches!(value, '-' | '.' | '_')),
            "GEMINI_MODEL must be a valid Gemini model name"
        );
        let endpoint = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent"
        );
        Ok(Self {
            api_key,
            model,
            endpoint,
            client: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(5))
                // Leave headroom inside the WhatsApp gateway delivery timeout.
                .timeout(Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
        })
    }

    async fn request_body(&self, request: GenerateRequest) -> anyhow::Result<Value> {
        if request.messages.is_empty() {
            return Err(InvalidInput("messages must not be empty").into());
        }
        if request.image_urls.len() > MAX_IMAGES {
            return Err(InvalidInput("at most 4 images are supported per request").into());
        }
        if request
            .temperature
            .is_some_and(|value| !value.is_finite() || !(0.0..=2.0).contains(&value))
        {
            return Err(InvalidInput("temperature must be between 0 and 2").into());
        }
        if request.max_tokens == Some(0) {
            return Err(InvalidInput("max_tokens must be positive").into());
        }
        if !request.image_urls.is_empty()
            && request.messages.last().map(|message| message.role.as_str()) != Some("user")
        {
            return Err(InvalidInput("images must be attached to the final user message").into());
        }

        let final_index = request.messages.len() - 1;
        let mut contents = Vec::with_capacity(request.messages.len());
        for (index, message) in request.messages.into_iter().enumerate() {
            let role = match message.role.as_str() {
                "user" => "user",
                "assistant" => "model",
                _ => return Err(InvalidInput("unsupported message role").into()),
            };
            let mut parts = vec![json!({"text": message.content})];
            if index == final_index {
                for source in &request.image_urls {
                    parts.push(image_part(&self.client, source).await?);
                }
            }
            contents.push(json!({"role": role, "parts": parts}));
        }

        let mut generation_config = Map::new();
        generation_config.insert(
            "maxOutputTokens".into(),
            json!(request.max_tokens.unwrap_or(2048)),
        );
        // Prevent short WhatsApp replies from spending their output budget
        // entirely on internal reasoning.
        generation_config.insert("thinkingConfig".into(), json!({"thinkingLevel": "minimal"}));
        if let Some(temperature) = request.temperature {
            generation_config.insert("temperature".into(), json!(temperature));
        }

        let mut body = json!({
            "contents": contents,
            "generationConfig": generation_config,
        });
        if let Some(system) = request
            .system_prompt
            .filter(|value| !value.trim().is_empty())
        {
            body["systemInstruction"] = json!({"parts": [{"text": system}]});
        }
        Ok(body)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiResponse {
    #[serde(default)]
    candidates: Vec<Candidate>,
    #[serde(default)]
    usage_metadata: Usage,
    #[serde(default)]
    model_version: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Candidate {
    content: Option<ResponseContent>,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct ResponseContent {
    #[serde(default)]
    parts: Vec<ResponsePart>,
}

#[derive(Deserialize)]
struct ResponsePart {
    text: Option<String>,
    #[serde(default)]
    thought: bool,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Usage {
    #[serde(default)]
    prompt_token_count: u32,
    #[serde(default)]
    candidates_token_count: u32,
}

#[async_trait]
impl TextGeneration for GeminiProvider {
    fn provider_name(&self) -> &str {
        "gemini"
    }

    async fn generate(&self, request: GenerateRequest) -> anyhow::Result<GenerateResponse> {
        let body = self.request_body(request).await?;
        let started = Instant::now();
        let response = self
            .client
            .post(&self.endpoint)
            // Keep credentials out of URLs, proxy logs, and traces.
            .header("x-goog-api-key", &self.api_key)
            .json(&body)
            .send()
            .await
            .context("Gemini request failed")?;
        ensure!(
            response.status().is_success(),
            "Gemini returned HTTP {}",
            response.status().as_u16()
        );
        let response: GeminiResponse = response.json().await.context("Invalid Gemini response")?;
        let candidate = response
            .candidates
            .into_iter()
            .next()
            .context("Gemini returned no candidates")?;
        ensure!(
            candidate.finish_reason.as_deref() == Some("STOP"),
            "Gemini completion did not finish normally"
        );
        let content = candidate
            .content
            .into_iter()
            .flat_map(|content| content.parts)
            .filter(|part| !part.thought)
            .filter_map(|part| part.text)
            .collect::<Vec<_>>()
            .join("");
        ensure!(!content.trim().is_empty(), "Gemini returned empty content");

        Ok(GenerateResponse {
            provider: "gemini".into(),
            content,
            model: if response.model_version.is_empty() {
                self.model.clone()
            } else {
                response.model_version
            },
            input_tokens: response.usage_metadata.prompt_token_count,
            output_tokens: response.usage_metadata.candidates_token_count,
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
                max_tokens: Some(256),
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
impl VisionService for GeminiProvider {
    fn provider_name(&self) -> &str {
        "gemini"
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
        let response = self
            .generate(GenerateRequest {
                system_prompt: Some(format!(
                    "{task} Respond in Indonesian. Separate visible observations from hypotheses, state uncertainty, and do not claim a definitive diagnosis from an image alone."
                )),
                messages: vec![ChatMessage {
                    role: "user".into(),
                    content: request.prompt,
                }],
                temperature: Some(0.3),
                max_tokens: Some(2048),
                image_urls: vec![request.image_url],
            })
            .await?;
        let structured = serde_json::from_str::<Value>(&response.content).ok();
        Ok(VisionResponse {
            analysis: response.content,
            structured_result: structured,
            // The API does not expose a calibrated confidence probability here.
            confidence: 0.0,
            model: response.model,
            latency_ms: response.latency_ms,
        })
    }
}

async fn image_part(client: &reqwest::Client, source: &str) -> anyhow::Result<Value> {
    if let Some(rest) = source.strip_prefix("data:") {
        let (mime, encoded) = rest
            .split_once(";base64,")
            .ok_or(InvalidInput("image must be a base64 data URI"))?;
        validate_mime(mime)?;
        if encoded.is_empty() || encoded.len() > 12 * 1024 * 1024 {
            return Err(InvalidInput("invalid or oversized base64 image").into());
        }
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| InvalidInput("invalid or oversized base64 image"))?;
        if decoded.len() > MAX_IMAGE_BYTES {
            return Err(InvalidInput("invalid or oversized base64 image").into());
        }
        return Ok(json!({
            "inlineData": {"mimeType": mime, "data": encoded}
        }));
    }

    let url = Url::parse(source)
        .map_err(|_| InvalidInput("image must be an HTTP(S) URL or base64 data URI"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || source.len() > 8192
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(InvalidInput("invalid image URL; local file paths are unsupported").into());
    }
    validate_public_destination(&url).await?;

    let response = client
        .get(url)
        .send()
        .await
        .context("image download failed")?;
    ensure!(
        response.status().is_success(),
        "image download returned HTTP {}",
        response.status().as_u16()
    );
    if response
        .content_length()
        .is_some_and(|size| size > MAX_IMAGE_BYTES as u64)
    {
        return Err(InvalidInput("downloaded image is too large").into());
    }
    let mime = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .ok_or(InvalidInput(
            "image response is missing a supported content type",
        ))?
        .to_owned();
    validate_mime(&mime)?;

    let mut bytes = Vec::new();
    let mut response = response;
    while let Some(chunk) = response
        .chunk()
        .await
        .context("could not read image response")?
    {
        if bytes.len() + chunk.len() > MAX_IMAGE_BYTES {
            return Err(InvalidInput("downloaded image is too large").into());
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.is_empty() {
        return Err(InvalidInput("downloaded image is empty").into());
    }
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok(json!({
        "inlineData": {"mimeType": mime, "data": encoded}
    }))
}

fn validate_mime(mime: &str) -> anyhow::Result<()> {
    if matches!(
        mime,
        "image/jpeg" | "image/png" | "image/gif" | "image/webp"
    ) {
        Ok(())
    } else {
        Err(InvalidInput("supported images: JPEG, PNG, GIF, and WebP").into())
    }
}

async fn validate_public_destination(url: &Url) -> anyhow::Result<()> {
    let host = url
        .host_str()
        .ok_or(InvalidInput("image URL must have a public host"))?;
    if host.eq_ignore_ascii_case("localhost") || host.ends_with(".localhost") {
        return Err(InvalidInput("image URL must have a public host").into());
    }
    let port = url
        .port_or_known_default()
        .ok_or(InvalidInput("image URL has an unsupported port"))?;
    let addresses = tokio::net::lookup_host((host, port))
        .await
        .context("could not resolve image host")?
        .collect::<Vec<_>>();
    if addresses.is_empty() || addresses.iter().any(|address| !is_public_ip(address.ip())) {
        return Err(InvalidInput("image URL must resolve only to public addresses").into());
    }
    Ok(())
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(a == 0
                || a == 10
                || a == 127
                || (a == 100 && (64..=127).contains(&b))
                || (a == 169 && b == 254)
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && b == 168)
                || (a == 192 && b == 0 && c == 0)
                || (a == 198 && (b == 18 || b == 19))
                || a >= 224)
        }
        IpAddr::V6(ip) => {
            if let Some(v4) = ip.to_ipv4_mapped() {
                return is_public_ip(IpAddr::V4(v4));
            }
            let octets = ip.octets();
            !(ip.is_unspecified()
                || ip.is_loopback()
                || ip.is_multicast()
                || (octets[0] & 0xfe) == 0xfc
                || (octets[0] == 0xfe && (octets[1] & 0xc0) == 0x80))
        }
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
    ) -> (GeminiProvider, Captured, tokio::task::JoinHandle<()>) {
        let captured: Captured = Arc::new(Mutex::new(None));
        let app = Router::new()
            .route(
                "/generateContent",
                post(
                    move |State(captured): State<Captured>,
                          headers: HeaderMap,
                          Json(body): Json<Value>| {
                        let response = response.clone();
                        async move {
                            *captured.lock().unwrap() = Some((
                                headers["x-goog-api-key"].to_str().unwrap().to_owned(),
                                body,
                            ));
                            (status, Json(response))
                        }
                    },
                ),
            )
            .with_state(captured.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut provider = GeminiProvider::new("test-key".into(), DEFAULT_MODEL.into()).unwrap();
        provider.endpoint = format!("http://{}/generateContent", listener.local_addr().unwrap());
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

    fn completion(text: Option<&str>, finish: &str) -> Value {
        let content = text.map_or_else(|| json!({}), |text| json!({"parts": [{"text": text}]}));
        json!({
            "candidates": [{"content": content, "finishReason": finish}],
            "usageMetadata": {"promptTokenCount": 13, "candidatesTokenCount": 7},
            "modelVersion": DEFAULT_MODEL
        })
    }

    #[tokio::test]
    async fn text_request_uses_header_system_instruction_and_usage() {
        let (provider, captured, task) =
            mock(StatusCode::OK, completion(Some("Tanaman melon."), "STOP")).await;
        let response = provider.generate(request()).await.unwrap();
        let (key, body) = captured.lock().unwrap().clone().unwrap();
        assert_eq!(key, "test-key");
        assert_eq!(
            body["systemInstruction"]["parts"][0]["text"],
            "Jawab dalam bahasa Indonesia."
        );
        assert_eq!(body["contents"][0]["role"], "user");
        assert_eq!(
            body["generationConfig"]["thinkingConfig"]["thinkingLevel"],
            "minimal"
        );
        assert_eq!((response.input_tokens, response.output_tokens), (13, 7));
        assert_eq!(response.provider, "gemini");
        assert_eq!(response.content, "Tanaman melon.");
        task.abort();
    }

    #[tokio::test]
    async fn vision_sends_inline_image_and_reports_actual_model() {
        let (provider, captured, task) =
            mock(StatusCode::OK, completion(Some("Daun melon."), "STOP")).await;
        let response = provider
            .analyze_image(VisionRequest {
                image_url: "data:image/png;base64,aW1hZ2U=".into(),
                prompt: "Periksa daun".into(),
                analysis_type: VisionAnalysisType::DiseaseDetection,
            })
            .await
            .unwrap();
        let (_, body) = captured.lock().unwrap().clone().unwrap();
        assert_eq!(
            body["contents"][0]["parts"][1]["inlineData"]["mimeType"],
            "image/png"
        );
        assert_eq!(response.model, DEFAULT_MODEL);
        assert_eq!(response.analysis, "Daun melon.");
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
            (StatusCode::OK, json!({"candidates": []})),
            (StatusCode::OK, completion(None, "STOP")),
            (StatusCode::OK, completion(Some("Incomplete"), "MAX_TOKENS")),
        ] {
            let (provider, _, task) = mock(status, body).await;
            let error = provider.generate(request()).await.unwrap_err().to_string();
            assert!(!error.contains("secret-provider-body"));
            task.abort();
        }
    }

    #[tokio::test]
    async fn invalid_media_and_wrong_roles_are_rejected() {
        let provider = GeminiProvider::new("test-key".into(), DEFAULT_MODEL.into()).unwrap();
        for source in [
            "/etc/passwd",
            "file:///etc/passwd",
            "data:audio/ogg;base64,YQ==",
            "data:image/png;base64,!",
            "data:image/png;base64,",
        ] {
            let mut value = request();
            value.image_urls.push(source.into());
            assert!(provider.request_body(value).await.is_err(), "{source}");
        }
        let mut value = request();
        value.messages[0].role = "assistant".into();
        value
            .image_urls
            .push("data:image/png;base64,aW1hZ2U=".into());
        assert!(provider.request_body(value).await.is_err());
    }

    #[test]
    fn credentials_and_model_are_required() {
        assert!(GeminiProvider::new(" ".into(), DEFAULT_MODEL.into()).is_err());
        assert!(GeminiProvider::new("test".into(), "../other".into()).is_err());
    }

    #[test]
    fn private_network_addresses_are_rejected() {
        assert!(!is_public_ip("127.0.0.1".parse().unwrap()));
        assert!(!is_public_ip("10.0.0.1".parse().unwrap()));
        assert!(!is_public_ip("::1".parse().unwrap()));
        assert!(is_public_ip("8.8.8.8".parse().unwrap()));
    }
}
