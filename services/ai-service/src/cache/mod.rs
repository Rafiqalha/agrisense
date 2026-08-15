//! AI-layer caching — semantic, response, and embedding caches.
//!
//! Rules:
//!   CACHEABLE: agronomy knowledge, disease info, fertilizer recommendations
//!   NOT CACHEABLE: stock levels, prices, weather, credit score (changes fast)
//!
//! Flow:
//!   User question → normalize → semantic similarity → cache hit?
//!   ├── yes → return cached response
//!   └── no  → AI → cache response → return

pub mod response_cache;
pub mod embedding_cache;
pub mod semantic_cache;

use serde::{Deserialize, Serialize};

/// Determines if a query result should be cached
#[derive(Debug, Clone, PartialEq)]
pub enum CachePolicy {
    /// Cache for a long time — knowledge that changes rarely
    /// e.g., disease treatments, fertilizer dosages
    LongTerm { ttl_secs: u64 },

    /// Cache briefly — information that changes daily
    /// e.g., weather forecasts
    ShortTerm { ttl_secs: u64 },

    /// Never cache — must always query source of truth
    /// e.g., stock levels, prices, credit scores
    NoCache,
}

impl CachePolicy {
    /// Determine cache policy based on intent
    pub fn for_intent(intent: &str) -> Self {
        match intent {
            // Always fresh — real-time data
            "CHECK_STOCK" | "CHECK_PRICE" | "CHECK_CREDIT_SCORE" => Self::NoCache,

            // Short cache — changes daily
            "ASK_WEATHER" => Self::ShortTerm { ttl_secs: 3600 }, // 1 hour

            // Long cache — agricultural knowledge
            "REPORT_DISEASE" | "ASK_FERTILIZER_RECOMMENDATION" => Self::LongTerm { ttl_secs: 86400 }, // 24 hours

            // Default: short cache
            _ => Self::ShortTerm { ttl_secs: 1800 }, // 30 min
        }
    }
}
