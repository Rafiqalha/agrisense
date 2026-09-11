//! Bounded conversation memory backed by Redis.
//!
//! Only text and a marker that an image was attached are retained. Raw image
//! URLs, WhatsApp media IDs, and base64 payloads never enter conversation
//! memory. Redis keys and owners use SHA-256 digests rather than phone numbers.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MEMORY_VERSION: u8 = 1;
const MAX_SINGLE_MESSAGE_CHARS: usize = 4_000;

#[derive(Debug, Clone, Copy)]
pub struct MemoryPolicy {
    pub ttl_seconds: u64,
    pub max_messages: usize,
    pub max_chars: usize,
}

impl MemoryPolicy {
    pub fn new(ttl_seconds: u64, max_messages: usize, max_chars: usize) -> Result<Self> {
        anyhow::ensure!(
            (300..=604_800).contains(&ttl_seconds),
            "conversation memory TTL must be between 300 and 604800 seconds"
        );
        anyhow::ensure!(
            (2..=20).contains(&max_messages),
            "conversation memory message limit must be between 2 and 20"
        );
        anyhow::ensure!(
            (1_000..=50_000).contains(&max_chars),
            "conversation memory character limit must be between 1000 and 50000"
        );
        Ok(Self {
            ttl_seconds,
            max_messages,
            max_chars,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMemory {
    version: u8,
    owner_digest: String,
    messages: Vec<MemoryMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryMessage {
    pub role: MessageRole,
    pub content: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
}

impl MessageRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }
}

impl ConversationMemory {
    pub fn new(farmer_id: &str) -> Result<Self> {
        validate_identifier("farmer_id", farmer_id)?;
        Ok(Self {
            version: MEMORY_VERSION,
            owner_digest: digest(farmer_id),
            messages: Vec::new(),
        })
    }

    pub async fn load(
        cache: &shared_cache::CacheClient,
        conversation_id: &str,
        farmer_id: &str,
    ) -> Result<Self> {
        validate_identifier("conversation_id", conversation_id)?;
        validate_identifier("farmer_id", farmer_id)?;
        let stored = cache
            .get::<Self>(&conversation_key(conversation_id))
            .await?;
        Ok(match stored {
            Some(memory)
                if memory.version == MEMORY_VERSION && memory.owner_digest == digest(farmer_id) =>
            {
                memory
            }
            Some(_) => {
                tracing::warn!("Discarded conversation memory with a mismatched owner or version");
                Self::new(farmer_id)?
            }
            None => Self::new(farmer_id)?,
        })
    }

    pub async fn save(
        &self,
        cache: &shared_cache::CacheClient,
        conversation_id: &str,
        policy: MemoryPolicy,
    ) -> Result<()> {
        validate_identifier("conversation_id", conversation_id)?;
        cache
            .set(&conversation_key(conversation_id), self, policy.ttl_seconds)
            .await
    }

    pub fn messages(&self) -> &[MemoryMessage] {
        &self.messages
    }

    pub fn append_turn(
        &mut self,
        user_message: &str,
        assistant_message: &str,
        image_count: usize,
        policy: MemoryPolicy,
    ) {
        let user_content = if image_count == 0 {
            user_message.to_owned()
        } else {
            format!(
                "[Pengguna melampirkan {image_count} gambar pada pesan ini. Gambar tidak disimpan; gunakan jawaban asisten setelah pesan ini sebagai ringkasan visual.]\n{user_message}"
            )
        };
        self.messages.push(MemoryMessage {
            role: MessageRole::User,
            content: truncate(&user_content, MAX_SINGLE_MESSAGE_CHARS),
        });
        self.messages.push(MemoryMessage {
            role: MessageRole::Assistant,
            content: truncate(assistant_message, MAX_SINGLE_MESSAGE_CHARS),
        });
        self.enforce_bounds(policy);
    }

    fn enforce_bounds(&mut self, policy: MemoryPolicy) {
        while self.messages.len() > policy.max_messages {
            let remove_count = self.messages.len().min(2);
            self.messages.drain(0..remove_count);
        }
        while self.total_chars() > policy.max_chars && self.messages.len() > 2 {
            self.messages.drain(0..2);
        }
    }

    fn total_chars(&self) -> usize {
        self.messages
            .iter()
            .map(|message| message.content.chars().count())
            .sum()
    }
}

fn validate_identifier(name: &str, value: &str) -> Result<()> {
    anyhow::ensure!(!value.trim().is_empty(), "{name} must not be empty");
    anyhow::ensure!(value.len() <= 256, "{name} is too long");
    anyhow::ensure!(
        !value.chars().any(char::is_control),
        "{name} contains control characters"
    );
    Ok(())
}

fn conversation_key(conversation_id: &str) -> String {
    format!("conv:v1:{}", digest(conversation_id))
}

fn digest(value: &str) -> String {
    let bytes = Sha256::digest(value.as_bytes());
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> MemoryPolicy {
        MemoryPolicy::new(86_400, 4, 1_000).unwrap()
    }

    #[test]
    fn redis_key_is_deterministic_and_hides_conversation_identifier() {
        let first = conversation_key("wa:628123456789");
        assert_eq!(first, conversation_key("wa:628123456789"));
        assert_ne!(first, conversation_key("wa:628000000000"));
        assert!(!first.contains("628123456789"));
    }

    #[test]
    fn owner_and_identifier_validation_prevent_context_mixing() {
        let memory = ConversationMemory::new("farmer-a").unwrap();
        assert_eq!(memory.owner_digest, digest("farmer-a"));
        assert_ne!(memory.owner_digest, digest("farmer-b"));
        assert!(validate_identifier("conversation_id", "").is_err());
        assert!(validate_identifier("conversation_id", "bad\nkey").is_err());
    }

    #[test]
    fn memory_is_bounded_and_images_are_represented_without_payloads() {
        let mut memory = ConversationMemory::new("farmer-a").unwrap();
        memory.append_turn("lihat daun", "analisis pertama", 1, policy());
        memory.append_turn("ini daun melon", "analisis kedua", 0, policy());
        memory.append_turn("lanjut", "jawaban ketiga", 0, policy());

        assert_eq!(memory.messages.len(), 4);
        assert_eq!(memory.messages[0].content, "ini daun melon");
        let serialized = serde_json::to_string(&memory).unwrap();
        assert!(!serialized.contains("base64"));
        assert!(!serialized.contains("wa-media"));
        assert!(memory.total_chars() <= policy().max_chars);
    }

    #[test]
    fn policy_rejects_unbounded_values() {
        assert!(MemoryPolicy::new(1, 4, 1_000).is_err());
        assert!(MemoryPolicy::new(86_400, 100, 1_000).is_err());
        assert!(MemoryPolicy::new(86_400, 4, 100).is_err());
    }
}
