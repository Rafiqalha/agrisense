# ADR 005: Synchronous vs Asynchronous Communication Patterns

**Date:** 2026-08-15
**Status:** Accepted

## Context

AgriSense uses both gRPC, MCP, and NATS JetStream. Without clear rules,
developers will default to one pattern for everything — causing either:
- Unnecessary latency (using events for real-time queries)
- Lost events (using direct calls for domain propagation)

## Decision

### Synchronous (gRPC / MCP) — for queries and commands needing immediate response

```
"Berapa stok pupuk saya?"
Brain → MCP tool: get_stock → Farm Service → response → WhatsApp
```

Use when:
- The caller NEEDS the result NOW to continue
- User is waiting for a response
- Read queries (get_farmer_profile, get_crop_status)
- Commands that return a result (record_expense → confirmation)

### Asynchronous (NATS Events) — for domain propagation and side effects

```
HarvestRecorded → analytics-service (aggregate)
                → notification-service (congratulate farmer)
                → finance-service (update revenue)
```

Use when:
- Multiple services need to react to a domain event
- The producer doesn't need to know the outcome
- Side effects that can be eventually consistent
- Audit trail and analytics ingestion

### Protocol Selection

| Interaction | Protocol | Example |
|-------------|----------|---------|
| AI Agent → Domain Tool | MCP | detect_disease, recommend_fertilizer |
| Service → Service (query) | gRPC | get_farm_status, verify_identity |
| Service → Service (command) | gRPC | register_farmer, create_order |
| Domain event propagation | NATS | FarmerCreated, DiseaseDetected |
| Analytics ingestion | NATS | subscribe to agrisense.> |
| Notification triggers | NATS | DiseaseDetected → alert farmer |

### Anti-patterns

❌ Using NATS for "get farmer profile" (synchronous query)
❌ Using gRPC for "notify all services about new harvest" (fan-out)
❌ Using MCP for service-to-service internal calls (MCP is for AI tools only)

## Consequences

- MCP is exclusively the AI capability protocol (brain → tools)
- gRPC is for service-to-service RPC (Farm ↔ Finance, Platform ↔ Farm)
- NATS is for domain event propagation and async workflows
- Clear boundaries prevent architectural confusion as team grows
