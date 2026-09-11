//! ElevenLabs speech client used by the WhatsApp voice-note pipeline.
//!
//! Audio stays inside the gateway boundary: inbound WhatsApp bytes are sent to
//! Speech-to-Text, while the final Brain response is sent to Text-to-Speech.
//! API keys and raw audio are never logged.

use anyhow::{Context, Result};
use bytes::Bytes;
use reqwest::{multipart, Client, StatusCode};
use serde::Deserialize;
use std::time::Duration;

const ELEVENLABS_ORIGIN: &str = "https://api.elevenlabs.io";
const MAX_AUDIO_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone)]
pub struct ElevenLabsClient {
    api_key: String,
    voice_id: String,
    stt_model: String,
    tts_model: String,
    language_code: String,
    output_format: OutputFormat,
    api_base: String,
    client: Client,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct OutputFormat {
    api_value: String,
    mime_type: &'static str,
    filename: &'static str,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Transcript {
    pub text: String,
    pub model: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct SynthesizedAudio {
    pub bytes: Bytes,
    pub mime_type: &'static str,
    pub filename: &'static str,
}

#[derive(Deserialize)]
struct TranscriptResponse {
    text: String,
}

impl ElevenLabsClient {
    pub fn new(
        api_key: String,
        voice_id: String,
        stt_model: String,
        tts_model: String,
        language_code: String,
        output_format: String,
    ) -> Result<Self> {
        Self::new_with_base(
            api_key,
            voice_id,
            stt_model,
            tts_model,
            language_code,
            output_format,
            ELEVENLABS_ORIGIN.to_owned(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new_with_base(
        api_key: String,
        voice_id: String,
        stt_model: String,
        tts_model: String,
        language_code: String,
        output_format: String,
        api_base: String,
    ) -> Result<Self> {
        validate_secret(&api_key)?;
        validate_identifier("ELEVENLABS_VOICE_ID", &voice_id)?;
        validate_identifier("ELEVENLABS_STT_MODEL", &stt_model)?;
        validate_identifier("ELEVENLABS_TTS_MODEL", &tts_model)?;
        validate_language_code(&language_code)?;
        let output_format = OutputFormat::parse(&output_format)?;
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(90))
            .build()
            .context("could not build ElevenLabs HTTP client")?;

        Ok(Self {
            api_key,
            voice_id,
            stt_model,
            tts_model,
            language_code,
            output_format,
            api_base: api_base.trim_end_matches('/').to_owned(),
            client,
        })
    }

    pub async fn transcribe(&self, audio: Bytes, mime_type: &str) -> Result<Transcript> {
        anyhow::ensure!(!audio.is_empty(), "inbound voice note is empty");
        anyhow::ensure!(
            audio.len() <= MAX_AUDIO_BYTES,
            "inbound voice note exceeds the 16 MiB safety limit"
        );
        let filename = filename_for_mime(mime_type);
        let file = multipart::Part::bytes(audio.to_vec())
            .file_name(filename)
            .mime_str(mime_type)
            .context("WhatsApp returned an invalid audio MIME type")?;
        let form = multipart::Form::new()
            .text("model_id", self.stt_model.clone())
            .text("language_code", self.language_code.clone())
            .text("tag_audio_events", "false")
            .text("diarize", "false")
            .part("file", file);

        let response = self
            .client
            .post(format!("{}/v1/speech-to-text", self.api_base))
            .header("xi-api-key", &self.api_key)
            .multipart(form)
            .send()
            .await
            .context("ElevenLabs transcription request failed")?;
        let response = ensure_success(response, "ElevenLabs transcription").await?;
        let body: TranscriptResponse = response
            .json()
            .await
            .context("ElevenLabs transcription returned invalid JSON")?;
        let text = body.text.trim().to_owned();
        anyhow::ensure!(!text.is_empty(), "ElevenLabs returned an empty transcript");

        Ok(Transcript {
            text,
            model: self.stt_model.clone(),
        })
    }

    pub async fn synthesize(&self, text: &str) -> Result<SynthesizedAudio> {
        anyhow::ensure!(!text.trim().is_empty(), "TTS input must not be empty");
        let url = format!(
            "{}/v1/text-to-speech/{}?output_format={}",
            self.api_base, self.voice_id, self.output_format.api_value
        );
        let response = self
            .client
            .post(url)
            .header("xi-api-key", &self.api_key)
            .json(&serde_json::json!({
                "text": text,
                "model_id": self.tts_model,
            }))
            .send()
            .await
            .context("ElevenLabs speech generation request failed")?;
        let response = ensure_success(response, "ElevenLabs speech generation").await?;
        if let Some(length) = response.content_length() {
            anyhow::ensure!(
                length <= MAX_AUDIO_BYTES as u64,
                "ElevenLabs audio exceeds the 16 MiB WhatsApp limit"
            );
        }
        let bytes = response
            .bytes()
            .await
            .context("could not read ElevenLabs audio response")?;
        anyhow::ensure!(!bytes.is_empty(), "ElevenLabs returned empty audio");
        anyhow::ensure!(
            bytes.len() <= MAX_AUDIO_BYTES,
            "ElevenLabs audio exceeds the 16 MiB WhatsApp limit"
        );

        Ok(SynthesizedAudio {
            bytes,
            mime_type: self.output_format.mime_type,
            filename: self.output_format.filename,
        })
    }
}

impl OutputFormat {
    fn parse(value: &str) -> Result<Self> {
        match value.trim() {
            "mp3_44100_128" => Ok(Self {
                api_value: "mp3_44100_128".to_owned(),
                mime_type: "audio/mpeg",
                filename: "agrisense-reply.mp3",
            }),
            _ => {
                anyhow::bail!("ELEVENLABS_TTS_OUTPUT_FORMAT currently supports only mp3_44100_128")
            }
        }
    }
}

fn validate_secret(value: &str) -> Result<()> {
    anyhow::ensure!(
        !value.trim().is_empty(),
        "ELEVENLABS_API_KEY must not be empty"
    );
    Ok(())
}

fn validate_identifier(name: &str, value: &str) -> Result<()> {
    anyhow::ensure!(!value.trim().is_empty(), "{name} must not be empty");
    anyhow::ensure!(
        value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_')),
        "{name} contains unsupported characters"
    );
    Ok(())
}

fn validate_language_code(value: &str) -> Result<()> {
    anyhow::ensure!(
        matches!(value.len(), 2 | 3)
            && value
                .chars()
                .all(|character| character.is_ascii_lowercase()),
        "ELEVENLABS_LANGUAGE_CODE must be a lowercase ISO 639-1 or ISO 639-3 code"
    );
    Ok(())
}

fn filename_for_mime(mime_type: &str) -> &'static str {
    match mime_type.split(';').next().unwrap_or_default().trim() {
        "audio/ogg" | "audio/opus" => "voice-note.ogg",
        "audio/mpeg" | "audio/mp3" => "voice-note.mp3",
        "audio/mp4" | "audio/aac" => "voice-note.m4a",
        "audio/amr" => "voice-note.amr",
        "audio/wav" | "audio/x-wav" => "voice-note.wav",
        _ => "voice-note.bin",
    }
}

async fn ensure_success(response: reqwest::Response, operation: &str) -> Result<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }

    let body = response.text().await.unwrap_or_default();
    anyhow::bail!(
        "{operation} failed with HTTP {status}: {}",
        elevenlabs_error_summary(status, &body)
    )
}

fn elevenlabs_error_summary(status: StatusCode, body: &str) -> String {
    let parsed: serde_json::Value = match serde_json::from_str(body) {
        Ok(value) => value,
        Err(_) => {
            return status
                .canonical_reason()
                .unwrap_or("request rejected")
                .to_owned()
        }
    };
    let detail = &parsed["detail"];
    detail["message"]
        .as_str()
        .or_else(|| detail.as_str())
        .unwrap_or_else(|| status.canonical_reason().unwrap_or("request rejected"))
        .chars()
        .take(300)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        extract::OriginalUri,
        http::{HeaderMap, StatusCode},
        routing::post,
        Router,
    };
    use std::sync::{Arc, Mutex};

    type Captured = Arc<Mutex<Option<(HeaderMap, String, String)>>>;

    fn test_client(api_base: String) -> ElevenLabsClient {
        ElevenLabsClient::new_with_base(
            "test-key".into(),
            "voice_123".into(),
            "scribe_v2".into(),
            "eleven_multilingual_v2".into(),
            "id".into(),
            "mp3_44100_128".into(),
            api_base,
        )
        .unwrap()
    }

    #[test]
    fn configuration_is_fail_closed() {
        assert!(ElevenLabsClient::new(
            String::new(),
            "voice".into(),
            "scribe_v2".into(),
            "eleven_multilingual_v2".into(),
            "id".into(),
            "mp3_44100_128".into(),
        )
        .is_err());
        assert!(OutputFormat::parse("pcm_44100").is_err());
        assert!(validate_language_code("id").is_ok());
        assert!(validate_language_code("ID").is_err());
    }

    #[test]
    fn maps_whatsapp_audio_mime_to_upload_filename() {
        assert_eq!(
            filename_for_mime("audio/ogg; codecs=opus"),
            "voice-note.ogg"
        );
        assert_eq!(filename_for_mime("audio/mpeg"), "voice-note.mp3");
        assert_eq!(
            filename_for_mime("application/octet-stream"),
            "voice-note.bin"
        );
    }

    #[test]
    fn provider_error_summary_does_not_echo_unknown_html() {
        let summary = elevenlabs_error_summary(StatusCode::BAD_GATEWAY, "<html>secret</html>");
        assert_eq!(summary, "Bad Gateway");
    }

    #[tokio::test]
    async fn transcription_sends_authenticated_multipart_audio() {
        let captured: Captured = Arc::new(Mutex::new(None));
        let handler_capture = Arc::clone(&captured);
        let app = Router::new().route(
            "/v1/speech-to-text",
            post(move |uri: OriginalUri, headers: HeaderMap, body: Body| {
                let handler_capture = Arc::clone(&handler_capture);
                async move {
                    let body = to_bytes(body, 1024 * 1024).await.unwrap();
                    *handler_capture.lock().unwrap() = Some((
                        headers,
                        String::from_utf8_lossy(&body).into_owned(),
                        uri.0.to_string(),
                    ));
                    (StatusCode::OK, r#"{"text":"daun cabai menguning"}"#)
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = test_client(format!("http://{}", listener.local_addr().unwrap()));
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let result = client
            .transcribe(Bytes::from_static(b"voice-bytes"), "audio/ogg")
            .await
            .unwrap();
        let (headers, body, uri) = captured.lock().unwrap().clone().unwrap();
        assert_eq!(headers["xi-api-key"], "test-key");
        assert!(headers["content-type"]
            .to_str()
            .unwrap()
            .starts_with("multipart/form-data; boundary="));
        assert!(body.contains("scribe_v2"));
        assert!(body.contains("language_code"));
        assert!(body.contains("voice-bytes"));
        assert_eq!(uri, "/v1/speech-to-text");
        assert_eq!(result.text, "daun cabai menguning");
        task.abort();
    }

    #[tokio::test]
    async fn speech_generation_uses_configured_voice_model_and_format() {
        let captured: Captured = Arc::new(Mutex::new(None));
        let handler_capture = Arc::clone(&captured);
        let app = Router::new().route(
            "/v1/text-to-speech/:voice_id",
            post(move |uri: OriginalUri, headers: HeaderMap, body: Body| {
                let handler_capture = Arc::clone(&handler_capture);
                async move {
                    let body = to_bytes(body, 1024 * 1024).await.unwrap();
                    *handler_capture.lock().unwrap() = Some((
                        headers,
                        String::from_utf8_lossy(&body).into_owned(),
                        uri.0.to_string(),
                    ));
                    (StatusCode::OK, b"mp3-audio".to_vec())
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = test_client(format!("http://{}", listener.local_addr().unwrap()));
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let result = client.synthesize("Halo petani").await.unwrap();
        let (headers, body, uri) = captured.lock().unwrap().clone().unwrap();
        assert_eq!(headers["xi-api-key"], "test-key");
        assert!(body.contains("Halo petani"));
        assert!(body.contains("eleven_multilingual_v2"));
        assert_eq!(
            uri,
            "/v1/text-to-speech/voice_123?output_format=mp3_44100_128"
        );
        assert_eq!(result.bytes, Bytes::from_static(b"mp3-audio"));
        assert_eq!(result.mime_type, "audio/mpeg");
        task.abort();
    }
}
