//! Response cache — exact match on normalized query.
//! Redis-backed, with TTL based on CachePolicy.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedResponse {
    pub query_hash: String,
    pub response: String,
    pub model: String,
    pub cached_at: chrono::DateTime<chrono::Utc>,
    pub hit_count: u64,
}

pub fn normalize_query(query: &str) -> String {
    query
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn query_hash(normalized: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    normalized.hash(&mut hasher);
    format!("resp_cache:{:x}", hasher.finish())
}
