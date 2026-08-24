//! WhatsApp Business API client — outbound messaging + media retrieval.
//!
//! Closes the conversation loop: the gateway does not only RECEIVE webhooks,
//! it also SENDS replies back to the farmer via the Meta Graph API.
//!
//!   - send_text_message: POST /{phone_number_id}/messages
//!   - download_media:    GET /{media_id} → {url} → raw bytes + mime type
//!
//! Media flow: when a farmer sends an image/voice note, the webhook only
//! carries a media ID. We resolve it to bytes here, then forward a data URI
//! to brain-service so downstream AI capabilities (Gemini vision, STT)
//! can consume it without needing WhatsApp credentials.

use anyhow::Result;
use base64::Engine;
use reqwest::Client;

const GRAPH_API_BASE: &str = "https://graph.facebook.com/v21.0";

#[derive(Clone)]
pub struct WhatsAppClient {
    phone_number_id: String,
    access_token: String,
    client: Client,
}

impl WhatsAppClient {
    pub fn new(phone_number_id: String, access_token: String) -> Self {
        Self {
            phone_number_id,
            access_token,
            client: Client::new(),
        }
    }

    /// Send a plain text message to a WhatsApp user.
    pub async fn send_text_message(&self, to: &str, body: &str) -> Result<serde_json::Value> {
        let url = format!("{}/{}/messages", GRAPH_API_BASE, self.phone_number_id);
        let payload = serde_json::json!({
            "messaging_product": "whatsapp",
            "to": to,
            "type": "text",
            "text": { "body": body, "preview_url": false }
        });

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.access_token)
            .json(&payload)
            .send()
            .await?;

        let status = resp.status();
        let json: serde_json::Value = resp.json().await?;
        if !status.is_success() {
            anyhow::bail!("WhatsApp send failed ({}): {}", status, json);
        }
        Ok(json)
    }

    /// Resolve a WhatsApp media ID to raw bytes + mime type.
    ///
    /// Meta webhooks carry media IDs, not URLs. Two calls:
    ///   1. GET /{media_id} → JSON containing the download `url`
    ///   2. GET {url} with Bearer token → raw media bytes
    pub async fn download_media(&self, media_id: &str) -> Result<(bytes::Bytes, String)> {
        let url = format!("{}/{}", GRAPH_API_BASE, media_id);
        let meta: serde_json::Value = self
            .client
            .get(&url)
            .bearer_auth(&self.access_token)
            .send()
            .await?
            .json()
            .await?;

        let download_url = meta["url"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("media metadata missing url: {}", meta))?;
        let mime = meta["mime_type"]
            .as_str()
            .unwrap_or("application/octet-stream")
            .to_string();

        let bytes = self
            .client
            .get(download_url)
            .bearer_auth(&self.access_token)
            .send()
            .await?
            .bytes()
            .await?;

        Ok((bytes, mime))
    }
}

/// Build a `data:` URI from raw media bytes so downstream services
/// (ai-service vision/STT) can consume media without WhatsApp credentials.
pub fn to_data_uri(mime: &str, bytes: &[u8]) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    format!("data:{};base64,{}", mime, encoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_uri_embeds_mime_and_payload() {
        let uri = to_data_uri("image/jpeg", b"\xff\xd8\xff");
        assert!(uri.starts_with("data:image/jpeg;base64,"));
        // base64 of 0xff 0xd8 0xff
        assert_eq!(uri, "data:image/jpeg;base64,/9j/");
    }

    #[test]
    fn data_uri_handles_empty_bytes() {
        let uri = to_data_uri("audio/ogg", b"");
        assert_eq!(uri, "data:audio/ogg;base64,");
    }
}
