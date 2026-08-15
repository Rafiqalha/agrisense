# AgriSense — Domain Boundaries & Ownership Rules

## Principle

Each service **owns** its database schema. No service writes to another service's tables.

Cross-domain communication:
- **Synchronous query/command → gRPC or MCP**
- **Asynchronous domain propagation → NATS events**

## Ownership Map

| Schema | Owner Service | Tables |
|--------|--------------|--------|
| `identity` | platform-service | users, sessions |
| `farm` | farm-service | farmers, farms, crops, activities, harvests |
| `agronomy` | agronomy-service | diseases, detections, recommendations, pests |
| `finance` | finance-service | transactions, credit_scores, loan_applications |
| `ai` | brain-service + ai-service | conversations, messages, agent_runs, inbound_messages, workflow_runs |
| `analytics` | analytics-service | disease_trends, harvest_aggregates, input_usage, platform_metrics |
| `outbox` | all services (shared) | events |
| `audit` | platform-service (writes from all) | logs |

## Read Rules

| Scenario | Method |
|----------|--------|
| farm-service needs farmer profile | gRPC to platform-service |
| agronomy-service needs farm location | gRPC to farm-service |
| finance-service needs harvest yield | Event: HarvestRecorded (async) |
| analytics-service needs everything | Subscribe to agrisense.> (NATS) |
| brain-service needs farmer data | MCP tool: get_farmer_profile → platform-service |

## Anti-patterns

❌ `SELECT * FROM identity.users` inside farm-service
❌ `SELECT * FROM farm.harvests` inside analytics-service
❌ `JOIN farm.crops ON agronomy.detections` anywhere
❌ Using NATS to query real-time stock levels

## MCP vs gRPC Boundary

```
AI Agent → MCP → Domain Tool (brain-service controls this)
Service  → gRPC → Service (direct service-to-service)
```

MCP is the AI capability protocol. gRPC is the service mesh protocol.
They are NOT interchangeable.
