# ADR 004: NATS JetStream Over Kafka

**Date:** 2026-08-15
**Status:** Accepted

## Context
AgriSense needs an event bus for inter-service communication (DiseaseDetected → analytics,
HarvestRecorded → finance, etc.).

## Decision
Use **NATS JetStream** as the event bus.

## Rationale
| Factor | NATS JetStream | Kafka |
|--------|----------------|-------|
| Operational complexity | Low (single binary) | High (Zookeeper/KRaft) |
| Latency | < 1ms | 5-10ms |
| Rust client quality | async-nats (excellent) | rdkafka (C bindings) |
| Storage | File/Memory | File (high throughput) |
| At-scale (>100M events/day) | Limitations | Strong |
| GKE resource cost | Very low | High |

## Migration Path
When AgriSense reaches > 100M events/day:
```
NATS JetStream → Google Pub/Sub or Kafka
```
The EventEnvelope struct and NatsSubject builder abstract this.
Migration = change publisher/consumer, not business logic.

## Consequences
Low infra cost in early stage.
NATS server runs in a single pod on GKE.
All event payloads are JSON (+ protobuf schemas for language-agnostic contracts).
