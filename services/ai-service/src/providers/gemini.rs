use async_trait::async_trait;
use super::{AiProvider, GenerateRequest, GenerateResponse};

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
}

#[async_trait]
impl AiProvider for GeminiProvider {
    fn name(&self) -> &str { "gemini" }

    async fn generate(&self, request: GenerateRequest) -> anyhow::Result<GenerateResponse> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model, self.api_key
        );

        let body = serde_json::json!({
            "contents": request.messages.iter().map(|m| serde_json::json!({
                "role": m.role,
                "parts": [{ "text": m.content }]
            })).collect::<Vec<_>>(),
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

    async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        // Gemini embedding API
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

    async fn classify_intent(&self, message: &str, context: &str) -> anyhow::Result<String> {
        let prompt = format!(
            "Classify the following Indonesian farmer message into one of these intents: \
            CHECK_STOCK, REPORT_DISEASE, ASK_WEATHER, RECORD_EXPENSE, RECORD_REVENUE, \
            CHECK_HARVEST_STATUS, ASK_FERTILIZER_RECOMMENDATION, REQUEST_LOAN, BUY_PRODUCT, UNKNOWN.\n\
            Context: {}\nMessage: {}\nRespond with only the intent name.",
            context, message
        );
        let req = GenerateRequest {
            system_prompt: None,
            messages: vec![super::ChatMessage { role: "user".into(), content: prompt }],
            temperature: Some(0.0),
            max_tokens: Some(20),
        };
        let resp = self.generate(req).await?;
        Ok(resp.content.trim().to_string())
    }

    async fn analyze_image(&self, image_url: &str, prompt: &str) -> anyhow::Result<String> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:generateContent?key={}",
            self.api_key
        );
        let body = serde_json::json!({
            "contents": [{
                "parts": [
                    { "text": prompt },
                    { "image_url": { "url": image_url } }
                ]
            }]
        });
        let resp: serde_json::Value = self.client.post(&url).json(&body).send().await?.json().await?;
        let content = resp["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string();
        Ok(content)
    }
}
