//! WhatsApp webhook signature verification (HMAC SHA256)
//!
//! Meta sends X-Hub-Signature-256 header with every webhook delivery.
//! Format: "sha256=<hex_digest>"
//!
//! We MUST verify this to prevent forged webhooks.

use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Verify the Meta webhook signature against the raw body bytes.
///
/// # Arguments
/// * `app_secret` - Your WhatsApp App Secret (from Meta dashboard)
/// * `body` - Raw request body bytes
/// * `signature_header` - Value of X-Hub-Signature-256 header ("sha256=...")
pub fn verify_signature(app_secret: &str, body: &[u8], signature_header: &str) -> bool {
    let expected = match signature_header.strip_prefix("sha256=") {
        Some(hex) => hex,
        None => return false,
    };

    let Ok(mut mac) = HmacSha256::new_from_slice(app_secret.as_bytes()) else {
        return false;
    };

    mac.update(body);

    let result = mac.finalize();
    let computed = hex::encode(result.into_bytes());

    // Constant-time comparison to prevent timing attacks
    constant_time_eq(computed.as_bytes(), expected.as_bytes())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b.iter()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_signature() {
        let secret = "test_secret_123";
        let body = b"test body content";
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let sig = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));
        assert!(verify_signature(secret, body, &sig));
    }

    #[test]
    fn test_invalid_signature() {
        assert!(!verify_signature("secret", b"body", "sha256=invalidhex"));
    }

    #[test]
    fn test_missing_prefix() {
        assert!(!verify_signature("secret", b"body", "invalid"));
    }
}
