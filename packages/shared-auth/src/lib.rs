use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use shared_types::{UserId, UserRole};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtClaims {
    pub sub: String,          // user ID
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
