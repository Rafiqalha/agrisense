//! WhatsApp Business API client — outbound messaging + media retrieval.
//!
//! Closes the conversation loop: the gateway does not only RECEIVE webhooks,
//! it also SENDS replies back to the farmer via the Meta Graph API.
//!
//!   - send_text_message: POST /{phone_number_id}/messages
//!   - download_media:    GET /{media_id} → {url} → raw bytes + mime type
//!   - upload_media:      POST /{phone_number_id}/media
//!   - send_audio_message: POST /{phone_number_id}/messages
//!
//! Media flow: when a farmer sends an image/voice note, the webhook only
//! carries a media ID. We resolve it to bytes here, then forward a data URI
//! to brain-service so downstream AI capabilities can consume media without
//! needing WhatsApp credentials.

use anyhow::{Context, Result};
use base64::Engine;
use reqwest::Client;

const GRAPH_API_ORIGIN: &str = "https://graph.facebook.com";

#[derive(Debug, PartialEq, Eq)]
pub struct SendMessageResult {
    pub message_id: Option<String>,
}

#[derive(Clone)]
pub struct WhatsAppClient {
    phone_number_id: String,
    access_token: String,
    graph_api_base: String,
    client: Client,
}

impl WhatsAppClient {
    pub fn new(
        phone_number_id: String,
        access_token: String,
        graph_api_version: String,
    ) -> Result<Self> {
        anyhow::ensure!(
            !phone_number_id.is_empty()
                && phone_number_id.chars().all(|value| value.is_ascii_digit()),
            "WHATSAPP_PHONE_NUMBER_ID must contain digits only"
        );
        let graph_api_version = normalize_graph_api_version(&graph_api_version)?;

        Ok(Self {
            phone_number_id,
            access_token,
            graph_api_base: format!("{GRAPH_API_ORIGIN}/{graph_api_version}"),
            client: Client::new(),
        })
    }

    /// Send a plain text message to a WhatsApp user.
    pub async fn send_text_message(&self, to: &str, body: &str) -> Result<SendMessageResult> {
        let url = format!("{}/{}/messages", self.graph_api_base, self.phone_number_id);
        let payload = serde_json::json!({
            "messaging_product": "whatsapp",
            "to": to,
            "type": "text",
            "text": { "body": body, "preview_url": false }
        });

        let response = self
            .client
            .post(&url)
            .bearer_auth(&self.access_token)
            .json(&payload)
            .send()
            .await
            .context("WhatsApp send request failed")?;
        let json = decode_meta_json(response, "WhatsApp send").await?;
        let message_id = json["messages"]
            .as_array()
            .and_then(|messages| messages.first())
            .and_then(|message| message["id"].as_str())
            .map(str::to_owned);

        Ok(SendMessageResult { message_id })
    }

    /// Upload generated audio to Meta and return the reusable media ID.
    pub async fn upload_media(
        &self,
        bytes: bytes::Bytes,
        mime_type: &str,
        filename: &str,
    ) -> Result<String> {
        anyhow::ensure!(!bytes.is_empty(), "outbound media is empty");
        let file = reqwest::multipart::Part::bytes(bytes.to_vec())
            .file_name(filename.to_owned())
            .mime_str(mime_type)
            .context("outbound media has an invalid MIME type")?;
        let form = reqwest::multipart::Form::new()
            .text("messaging_product", "whatsapp")
            .text("type", mime_type.to_owned())
            .part("file", file);
        let url = format!("{}/{}/media", self.graph_api_base, self.phone_number_id);
        let response = self
            .client
            .post(url)
            .bearer_auth(&self.access_token)
            .multipart(form)
            .send()
            .await
            .context("WhatsApp media upload request failed")?;
        let json = decode_meta_json(response, "WhatsApp media upload").await?;
        let media_id = json["id"]
            .as_str()
            .context("WhatsApp media upload did not return a media ID")?;
        anyhow::ensure!(!media_id.is_empty(), "WhatsApp returned an empty media ID");
        Ok(media_id.to_owned())
    }

    /// Send an already-uploaded audio file to a WhatsApp user.
    pub async fn send_audio_message(&self, to: &str, media_id: &str) -> Result<SendMessageResult> {
        anyhow::ensure!(!media_id.is_empty(), "WhatsApp audio media ID is empty");
        let url = format!("{}/{}/messages", self.graph_api_base, self.phone_number_id);
        let payload = serde_json::json!({
            "messaging_product": "whatsapp",
            "to": to,
            "type": "audio",
            "audio": { "id": media_id }
        });
        let response = self
            .client
            .post(url)
            .bearer_auth(&self.access_token)
            .json(&payload)
            .send()
            .await
            .context("WhatsApp audio send request failed")?;
        let json = decode_meta_json(response, "WhatsApp audio send").await?;
        let message_id = json["messages"]
            .as_array()
            .and_then(|messages| messages.first())
            .and_then(|message| message["id"].as_str())
            .map(str::to_owned);

        Ok(SendMessageResult { message_id })
    }

    /// Resolve a WhatsApp media ID to raw bytes + mime type.
    ///
    /// Meta webhooks carry media IDs, not URLs. Two calls:
    ///   1. GET /{media_id} → JSON containing the download `url`
    ///   2. GET {url} with Bearer token → raw media bytes
    pub async fn download_media(&self, media_id: &str) -> Result<(bytes::Bytes, String)> {
        let url = format!("{}/{}", self.graph_api_base, media_id);
        let response = self
            .client
            .get(&url)
            .bearer_auth(&self.access_token)
            .send()
            .await
            .context("WhatsApp media metadata request failed")?;
        let meta = decode_meta_json(response, "WhatsApp media metadata").await?;

        let download_url = meta["url"]
            .as_str()
            .context("WhatsApp media metadata did not contain a download URL")?;
        let mime = meta["mime_type"]
            .as_str()
            .unwrap_or("application/octet-stream")
            .to_string();

        let response = self
            .client
            .get(download_url)
            .bearer_auth(&self.access_token)
            .send()
            .await
            .context("WhatsApp media download request failed")?;
        let status = response.status();
        anyhow::ensure!(
            status.is_success(),
            "WhatsApp media download failed with HTTP {status}"
        );
        let bytes = response
            .bytes()
            .await
            .context("could not read WhatsApp media response")?;

        Ok((bytes, mime))
    }
}

fn normalize_graph_api_version(value: &str) -> Result<String> {
    let value = value.trim();
    let Some(version) = value.strip_prefix('v') else {
        anyhow::bail!("WHATSAPP_GRAPH_API_VERSION must use the vNN.N format");
    };
    let mut components = version.split('.');
    let major = components.next().unwrap_or_default();
    let minor = components.next().unwrap_or_default();
    anyhow::ensure!(
        components.next().is_none()
            && !major.is_empty()
            && !minor.is_empty()
            && major.chars().all(|value| value.is_ascii_digit())
            && minor.chars().all(|value| value.is_ascii_digit()),
        "WHATSAPP_GRAPH_API_VERSION must use the vNN.N format"
    );
    Ok(format!("v{major}.{minor}"))
}

async fn decode_meta_json(
    response: reqwest::Response,
    operation: &str,
) -> Result<serde_json::Value> {
    let status = response.status();
    let body = response
        .text()
        .await
        .with_context(|| format!("could not read {operation} response"))?;
    let json: serde_json::Value = serde_json::from_str(&body)
        .with_context(|| format!("{operation} returned invalid JSON with HTTP {status}"))?;

    if !status.is_success() {
        anyhow::bail!(
            "{operation} failed with HTTP {status}: {}",
            meta_error_summary(&json)
        );
    }
    Ok(json)
}

fn meta_error_summary(response: &serde_json::Value) -> String {
    let error = &response["error"];
    let message = error["message"]
        .as_str()
        .unwrap_or("unknown Meta API error");
    let error_type = error["type"].as_str().unwrap_or("unknown");
    let code = error["code"]
        .as_i64()
        .map_or_else(|| "unknown".to_owned(), |value| value.to_string());
    let subcode = error["error_subcode"]
        .as_i64()
        .map(|value| format!(", subcode={value}"))
        .unwrap_or_default();
    format!("{message} (type={error_type}, code={code}{subcode})")
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
    fn graph_api_version_requires_a_safe_version_segment() {
        assert_eq!(normalize_graph_api_version("v25.0").unwrap(), "v25.0");
        assert!(normalize_graph_api_version("25.0").is_err());
        assert!(normalize_graph_api_version("v25").is_err());
        assert!(normalize_graph_api_version("v25.0/messages").is_err());
    }

    #[test]
    fn meta_error_summary_keeps_actionable_fields() {
        let response = serde_json::json!({
            "error": {
                "message": "Unsupported post request",
                "type": "GraphMethodException",
                "code": 100,
                "error_subcode": 33,
                "fbtrace_id": "trace"
            }
        });
        assert_eq!(
            meta_error_summary(&response),
            "Unsupported post request (type=GraphMethodException, code=100, subcode=33)"
        );
    }

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
