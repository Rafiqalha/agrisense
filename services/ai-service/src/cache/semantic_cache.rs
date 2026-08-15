//! Semantic cache — find similar (not exact) past queries.
//!
//! Uses pgvector to find cached responses for semantically similar questions.
//!
//! Flow:
//!   1. Embed the new query
//!   2. Search pgvector for cached responses with cosine similarity > threshold
//!   3. If found → return cached response (saves AI call)
//!   4. If not → call AI, cache response + embedding
//!
//! Threshold: 0.92 cosine similarity (tuned for Indonesian agricultural queries)

pub const SEMANTIC_SIMILARITY_THRESHOLD: f32 = 0.92;

// Implementation requires pgvector queries — integrated at service level, not here.
// This module defines the contract; actual SQL queries live in ai-service routes.
