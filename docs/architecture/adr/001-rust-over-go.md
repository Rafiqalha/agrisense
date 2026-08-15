# ADR 001: Rust Over Go for Backend Services

**Date:** 2026-08-15
**Status:** Accepted

## Context
AgriSense needs a backend language for 8 services that will eventually serve 1 million farmers.
Key requirements: memory safety, performance, minimal runtime overhead on GKE, strong async support.

## Decision
Use **Rust** for all backend services.

## Rationale
| Factor | Rust | Go |
|--------|------|-----|
| Memory safety | Compile-time | GC pause |
| Performance | Native | Very good |
| Async model | tokio (mature) | goroutines |
| Cost at scale | Lower (no GC) | Higher |
| Error handling | Result<T,E> explicit | error interface |
| WASM future | Excellent | Possible |

For a WhatsApp-first service at 1M farmers, latency and memory footprint matter.
Rust's zero-cost abstractions mean we scale vertically longer, reducing GKE node costs.

## Trade-offs
- Steeper learning curve for new engineers
- Slower initial development velocity
- Mitigation: strong shared libraries (shared-types, shared-mcp) reduce boilerplate

## Consequences
All services: Rust + Axum + tokio + sqlx.
