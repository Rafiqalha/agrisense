use async_trait::async_trait;
use super::{
    TextGeneration, VisionService, EmbeddingService,
    GenerateRequest, GenerateResponse, ChatMessage,
    VisionRequest, VisionResponse,
};

/// Gemini implements: TextGeneration + VisionService + EmbeddingService
/// Does NOT implement: SpeechService (use Whisper), SafetyService (use Llama Guard)
pub struct GeminiProvider {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl GeminiProvider {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            client: reqwest::Client::new(),
        }
    }

    fn base_url(&self, model: &str) -> String {
        format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            model, self.api_key
        )
    }
}

#[async_trait]
impl TextGeneration for GeminiProvider {
    fn provider_name(&self) -> &str { "gemini" }

    async fn generate(&self, request: GenerateRequest) -> anyhow::Result<GenerateResponse> {
        let url = self.base_url(&self.model);

        let mut contents = Vec::new();

        // System instruction (if present)
        if let Some(ref system) = request.system_prompt {
            contents.push(serde_json::json!({
                "role": "user",
                "parts": [{ "text": system }]
            }));
            contents.push(serde_json::json!({
                "role": "model",
                "parts": [{ "text": "Understood. I will follow these instructions." }]
            }));
        }

        // Messages
        for msg in &request.messages {
            let role = if msg.role == "assistant" { "model" } else { &msg.role };
            contents.push(serde_json::json!({
                "role": role,
                "parts": [{ "text": msg.content }]
            }));
        }

        let body = serde_json::json!({
            "contents": contents,
            "generationConfig": {
                "temperature": request.temperature.unwrap_or(0.4),
                "maxOutputTokens": request.max_tokens.unwrap_or(2048),
            }
        });

        let start = std::time::Instant::now();
        let resp = self.client.post(&url).json(&body).send().await?;
        let latency_ms = start.elapsed().as_millis() as u64;

        let json: serde_json::Value = resp.json().await?;
        let content = json["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string();

        Ok(GenerateResponse {
            content,
            model: self.model.clone(),
            input_tokens: json["usageMetadata"]["promptTokenCount"].as_u64().unwrap_or(0) as u32,
            output_tokens: json["usageMetadata"]["candidatesTokenCount"].as_u64().unwrap_or(0) as u32,
            latency_ms,
        })
    }

    async fn classify_intent(&self, message: &str, context: &str) -> anyhow::Result<String> {
        let prompt = format!(
            "You are an agricultural AI assistant for Indonesian farmers.\n\
            Classify the following message into one of these intents:\n\
            CHECK_STOCK, REPORT_DISEASE, ASK_WEATHER, RECORD_EXPENSE, RECORD_REVENUE,\n\
            CHECK_HARVEST_STATUS, ASK_FERTILIZER_RECOMMENDATION, REQUEST_LOAN,\n\
            CHECK_CREDIT_SCORE, BUY_PRODUCT, CHECK_PRICE, REPORT_ACTIVITY, UNKNOWN.\n\n\
            Context: {}\n\
            Message: {}\n\n\
            Respond with ONLY the intent name, nothing else.",
            context, message
        );
        let req = GenerateRequest {
            system_prompt: None,
            messages: vec![ChatMessage { role: "user".into(), content: prompt }],
            temperature: Some(0.0),
            max_tokens: Some(30),
        };
        let resp = self.generate(req).await?;
        Ok(resp.content.trim().to_string())
    }
}

#[async_trait]
impl VisionService for GeminiProvider {
    fn provider_name(&self) -> &str { "gemini-vision" }

    async fn analyze_image(&self, request: VisionRequest) -> anyhow::Result<VisionResponse> {
        let url = self.base_url("gemini-2.0-flash");

        let analysis_prompt = match request.analysis_type {
            super::VisionAnalysisType::DiseaseDetection => format!(
                "You are an expert agronomist. Analyze this crop image for diseases.\n\
                Identify: disease name, severity (low/medium/high/critical), affected area %.\n\
                Respond in JSON: {{\"disease\": \"...\", \"severity\": \"...\", \"confidence\": 0.0-1.0, \"treatment\": \"...\"}}\n\
                Additional context: {}", request.prompt
            ),
            super::VisionAnalysisType::PestIdentification => format!(
                "Identify any pests in this agricultural image. Include pest name, damage level, and recommended treatment.\n\
                Context: {}", request.prompt
            ),
            _ => request.prompt.clone(),
        };

        // Resolve the image into a Gemini part (base64 inline).
        let image_part = fetch_image_part(&self.client, &request.image_url).await?;

        let body = serde_json::json!({
            "contents": [{
                "parts": [
                    { "text": analysis_prompt },
                    image_part,
                ]
            }]
        });

        let start = std::time::Instant::now();
        let resp = self.client.post(&url).json(&body).send().await?;
        let latency_ms = start.elapsed().as_millis() as u64;

        let json: serde_json::Value = resp.json().await?;
        let analysis = json["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string();

        // Try to parse structured result from response
        let structured = serde_json::from_str::<serde_json::Value>(&analysis).ok();
        let confidence = structured
            .as_ref()
            .and_then(|v| v["confidence"].as_f64())
            .unwrap_or(0.0) as f32;

        Ok(VisionResponse {
            analysis: analysis.clone(),
            structured_result: structured,
            confidence,
            model: "gemini-2.0-flash".into(),
            latency_ms,
        })
    }
}

/// Build the Gemini `part` object carrying the image bytes.
///
/// Accepts three forms of `image_url`:
///   - `data:<mime>;base64,<data>`  → inline as-is (whatsapp-gateway sends this)
///   - `http(s)://...`              → download, then inline as base64
///   - local file path              → read, then inline as base64
///
/// Always inlines as base64: the Gemini REST API does not reliably fetch
/// arbitrary public HTTP URLs, so we never rely on that.
async fn fetch_image_part(
    client: &reqwest::Client,
    image_url: &str,
) -> anyhow::Result<serde_json::Value> {
    use base64::Engine;

    if let Some(rest) = image_url.strip_prefix("data:") {
        // data:<mime>;base64,<data>
        let (mime, b64) = rest
            .split_once(";base64,")
            .ok_or_else(|| anyhow::anyhow!("invalid data URI (missing ;base64,)"))?;
        return Ok(serde_json::json!({
            "inline_data": { "mime_type": mime, "data": b64 }
        }));
    }

    let (mime, bytes): (String, Vec<u8>) =
        if image_url.starts_with("http://") || image_url.starts_with("https://") {
            let resp = client.get(image_url).send().await?;
            let mime = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("image/jpeg")
                .to_string();
            (mime, resp.bytes().await?.to_vec())
        } else {
            // Local file path (dev convenience)
            ("image/jpeg".to_string(), std::fs::read(image_url)?)
        };

    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(serde_json::json!({
        "inline_data": { "mime_type": mime, "data": b64 }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn data_uri_is_inlined_as_is() {
        let client = reqwest::Client::new();
        let part = fetch_image_part(&client, "data:image/png;base64,QUJD")
            .await
            .expect("data uri parses");
        assert_eq!(part["inline_data"]["mime_type"], "image/png");
        assert_eq!(part["inline_data"]["data"], "QUJD");
    }
}

#[async_trait]
impl EmbeddingService for GeminiProvider {
    fn provider_name(&self) -> &str { "gemini-embedding" }

    async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/text-embedding-004:embedContent?key={}",
            self.api_key
        );
        let body = serde_json::json!({
            "model": "models/text-embedding-004",
            "content": { "parts": [{ "text": text }] }
        });
        let resp: serde_json::Value = self.client.post(&url).json(&body).send().await?.json().await?;
        let values = resp["embedding"]["values"]
            .as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_f64().map(|f| f as f32)).collect())
            .unwrap_or_default();
        Ok(values)
    }

    async fn embed_batch(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        // Gemini batch embedding API
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            results.push(self.embed(text).await?);
        }
        Ok(results)
    }

    fn dimension(&self) -> usize { 768 }
}
