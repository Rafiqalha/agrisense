use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use shared_types::UserRole;
use std::{fmt, sync::Arc};

const MIN_SECRET_BYTES: usize = 32;

/// A service-to-service bearer token. Debug output is always redacted and
/// comparisons are constant-time to avoid leaking a matching prefix.
#[derive(Clone)]
pub struct InternalToken(Arc<str>);

impl InternalToken {
    pub fn new(value: impl Into<String>) -> anyhow::Result<Self> {
        let value = value.into();
        anyhow::ensure!(
            value.trim() == value,
            "internal service token must not contain surrounding whitespace"
        );
        anyhow::ensure!(
            value.len() >= MIN_SECRET_BYTES,
            "internal service token must contain at least {MIN_SECRET_BYTES} bytes"
        );
        Ok(Self(Arc::from(value)))
    }

    pub fn authorize_header(&self, authorization: Option<&str>) -> bool {
        let Some(candidate) = authorization.and_then(|value| value.strip_prefix("Bearer ")) else {
            return false;
        };
        constant_time_eq(self.0.as_bytes(), candidate.as_bytes())
    }

    pub fn expose_for_request(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for InternalToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("InternalToken([REDACTED])")
    }
}

fn constant_time_eq(expected: &[u8], candidate: &[u8]) -> bool {
    if expected.len() != candidate.len() {
        return false;
    }
    expected
        .iter()
        .zip(candidate)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

pub fn bearer_value(authorization: Option<&str>) -> Option<&str> {
    authorization.and_then(|value| value.strip_prefix("Bearer "))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtClaims {
    pub sub: String, // user ID
    pub role: UserRole,
    pub farmer_id: Option<String>,
    pub exp: i64,
    pub iat: i64,
}

pub struct AuthService {
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
}

impl AuthService {
    pub fn new(secret: &str) -> Self {
        Self {
            encoding_key: EncodingKey::from_secret(secret.as_bytes()),
            decoding_key: DecodingKey::from_secret(secret.as_bytes()),
        }
    }

    pub fn generate_token(&self, claims: &JwtClaims) -> anyhow::Result<String> {
        Ok(encode(&Header::default(), claims, &self.encoding_key)?)
    }

    pub fn validate_token(&self, token: &str) -> anyhow::Result<JwtClaims> {
        let data = decode::<JwtClaims>(token, &self.decoding_key, &Validation::default())?;
        Ok(data.claims)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn internal_token_rejects_weak_secrets() {
        assert!(InternalToken::new("short").is_err());
        assert!(InternalToken::new("                                ").is_err());
    }

    #[test]
    fn internal_token_requires_exact_bearer_value() {
        let token = InternalToken::new(TOKEN).unwrap();
        assert!(token.authorize_header(Some(&format!("Bearer {TOKEN}"))));
        assert!(!token.authorize_header(Some(TOKEN)));
        assert!(!token.authorize_header(Some("Bearer wrong")));
        assert!(!token.authorize_header(None));
    }

    #[test]
    fn internal_token_debug_is_redacted() {
        let token = InternalToken::new(TOKEN).unwrap();
        let debug = format!("{token:?}");
        assert!(!debug.contains(TOKEN));
        assert!(debug.contains("REDACTED"));
    }

    #[test]
    fn jwt_round_trip_preserves_authenticated_identity() {
        let auth = AuthService::new(TOKEN);
        let claims = JwtClaims {
            sub: uuid::Uuid::new_v4().to_string(),
            role: UserRole::Farmer,
            farmer_id: Some(uuid::Uuid::new_v4().to_string()),
            iat: chrono::Utc::now().timestamp(),
            exp: (chrono::Utc::now() + chrono::Duration::minutes(5)).timestamp(),
        };
        let token = auth.generate_token(&claims).unwrap();
        let decoded = auth.validate_token(&token).unwrap();
        assert_eq!(decoded.sub, claims.sub);
        assert_eq!(decoded.role, UserRole::Farmer);
    }
}
