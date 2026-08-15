//! Embedding cache — avoid re-computing embeddings for the same text.
//! Redis-backed. Key: hash(text) → Vec<f32>

pub fn embedding_cache_key(text: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    format!("emb_cache:{:x}", hasher.finish())
}
