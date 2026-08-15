# ADR 006: Transactional Outbox Pattern

**Date:** 2026-08-15
**Status:** Accepted

## Context

AgriSense is event-driven. When a service writes to its database AND publishes
an event to NATS, there's a dual-write problem:

```
BEGIN TX
  INSERT INTO farm.harvests (...)    ← DB write
COMMIT

nats.publish("agrisense.farm.harvest_recorded", ...)  ← can fail!
```

If NATS publish fails after DB commit, the event is lost.
Analytics never knows about the harvest. Finance never updates revenue.

## Decision

Use the **Transactional Outbox Pattern**.

```
BEGIN TX
  INSERT INTO farm.harvests (...)
  INSERT INTO outbox.events (
    aggregate_type = 'harvest',
    aggregate_id   = harvest_id,
    event_type     = 'harvest_recorded',
    nats_subject   = 'agrisense.farm.harvest_recorded',
    payload        = { ... }
  )
COMMIT
```

A separate outbox publisher process polls `outbox.events` for unpublished rows
and sends them to NATS JetStream.

## Publisher Design

```
loop {
  SELECT * FROM outbox.events
    WHERE status = 'pending'
    ORDER BY created_at
    LIMIT 100;

  for event in batch {
    nats.publish(event.nats_subject, event.payload);
    UPDATE outbox.events SET status = 'published', published_at = NOW()
      WHERE id = event.id;
  }

  sleep(500ms);
}
```

## Idempotency

- Each outbox entry has a unique `idempotency_key`
- NATS consumers must be idempotent (dedup by event_id)
- This combination guarantees at-least-once delivery

## Dead Letter

- After `max_retries` (default: 5), events move to `dead_letter` status
- Alertmanager fires on dead letter count > 0
- Ops can manually replay dead letters

## Consequences

- Zero event loss: if DB commit succeeds, event WILL be published
- Small latency increase (poll interval, default 500ms)
- Outbox table needs cleanup (cron job to delete published events > 7 days)
- Each service runs its own outbox publisher goroutine
