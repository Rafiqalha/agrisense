//! Idempotency — prevent processing the same webhook/message twice.
//!
//! Redis-backed deduplication with TTL.
//! Key format: "wa_idem:<idempotency_key>"
//! Value: "1" (exists = already processed)
//! TTL: 24 hours (Meta can retry within this window)

/// Check if this message has already been processed.
pub async fn is_duplicate(
    cache: &mut shared_cache::CacheClient,
    idempotency_key: &str,
) -> bool {
    let key = format!("wa_idem:{}", idempotency_key);
    match cache.get::<String>(&key).await {
        Ok(Some(_)) => true,
        _ => false,
    }
}

/// Mark a message as processed (prevents future duplicates).
pub async fn mark_processed(
    cache: &mut shared_cache::CacheClient,
    idempotency_key: &str,
    ttl_secs: u64,
) -> anyhow::Result<()> {
    let key = format!("wa_idem:{}", idempotency_key);
    cache.set(&key, &"1".to_string(), ttl_secs).await
}
