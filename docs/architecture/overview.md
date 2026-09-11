# AgriSense Architecture Overview

> AgriSense is not a farming chatbot.
> It is an Operating System for agricultural economic activity, powered by conversation.

## Core Principles

1. **Domain-Driven** — Domain boundaries, not technology layers
2. **Event-Driven** — Services communicate via events, not direct calls
3. **MCP-Centric** — AI agents interact with the world via MCP tools
4. **AI-as-a-Service** — AI is a capability layer, not the center of the system

## Service Map

```
WhatsApp Business API
        ↓
whatsapp-gateway (Axum)
        ↓
brain-service (Orchestrator + MCP Registry)
    ↓           ↓           ↓           ↓
farm-service  agronomy   finance   platform
                ↓
             ai-service (TextGeneration + VisionService)
               ├── DeepSeek V4 Flash Vision (experimental)
               └── Speech / embeddings / RAG: unavailable (HTTP 501)
                        ↓
              analytics-service (event consumer)
```

## Data Flow: Disease Report

```
Farmer sends photo on WhatsApp
        ↓
whatsapp-gateway receives webhook
        ↓
brain-service: detect intent = REPORT_DISEASE
        ↓
supervisor → agronomist_agent
        ↓
MCP tool: detect_disease (ai-service)
        ↓
DeepSeek Vision analyzes image
        ↓
MCP tool: recommend_fertilizer (agronomy-service)
        ↓
MCP tool: list_products (marketplace-service)
        ↓
Response sent back → WhatsApp
        ↓
Event: DiseaseDetected → analytics-service (async)
```

The gateway persists the generated reply before calling the Meta Graph API.
Delivery retries therefore reuse the same reply and do not repeat AI inference.
The Graph API version is supplied through `WHATSAPP_GRAPH_API_VERSION`.
Inbound WhatsApp audio is downloaded by the gateway and transcribed with
ElevenLabs before entering Brain. Audio input receives an ElevenLabs-generated
audio response; STT output and the uploaded Meta media ID are stored as durable
retry checkpoints. Raw audio is not added to Brain conversation memory.

Unknown WhatsApp identities are handled by a deterministic, durable onboarding
state machine before the LLM loop. It collects one bounded field at a time and
requires both privacy consent and a final summary confirmation. The final
identity, farmer, farm, and crop writes happen in one transaction serialized by
the normalized WhatsApp owner. Inbound request UUIDs make every transition
idempotent. Catalogued crops such as melon use first-class enum values;
cultivation system, unit count, area per unit, and authoritative total area are
separate fields. Per-unit totals are calculated deterministically before the
database write rather than inferred later by the language model.

Authenticated farmers record bounded operational activities through a second
deterministic state machine. `CATAT` creates a durable draft, but no domain
write occurs until the user replies `YA`; `UBAH` returns to data entry and
`BATAL` closes the session without a write. Brain calls farm-service with the
authenticated phone scope and never supplies a farm or crop ID. Farm-service
resolves the newest active crop inside a narrow database function, commits the
activity and transactional outbox atomically, and publishes
`agrisense.farm.activity_recorded` only after a JetStream acknowledgement.
Inbound UUIDs make the confirmation, domain write, and resulting event
idempotent across retries.

Brain keeps a bounded, expiring multi-turn session in Redis using a hashed
conversation key and owner digest. It stores user/assistant text plus a marker
that an image existed, never raw image bytes, base64 data, WhatsApp media IDs,
or image URLs. The latest domain facts are rebuilt from PostgreSQL on every
turn, so conversation history cannot override the source of truth. Redis
failure degrades to a stateless answer rather than blocking message delivery.

## Database Isolation

One PostgreSQL cluster, isolated schemas:
- `identity` — platform-service
- `farm` — farm-service
- `agronomy` — agronomy-service (+ pgvector)
- `finance` — finance-service
- `ai` — brain-service, ai-service
- `analytics` — analytics-service (read-heavy)

Each service owns its schema. Cross-domain = events, not JOINs.

Every runtime service that accesses PostgreSQL connects through a distinct `NOSUPERUSER`,
`NOCREATEROLE`, `NOCREATEDB`, `NOINHERIT`, `NOBYPASSRLS` login role; only the
migration job uses the schema-owner account. A service without a database
repository receives no database credential. Gateway access is restricted to
its durable inbound queue. Brain reads identity/farm context and writes
onboarding and confirmation state only through narrowly scoped
`SECURITY DEFINER` functions with fixed search paths; audit tables use
transaction-local tenant identity and forced RLS. Farm-service has no direct
tenant-table privileges: its activity API can execute only owner-scoped
record/history and activity-outbox functions. Placeholder domain services
receive database `CONNECT` only and no
object privileges until their repository operations are implemented. Tenant
policies traverse the ownership chain `identity.users → farm.farmers →
farm.farms → crops/activities/harvests`, so a caller cannot select or mutate
another farmer's rows merely by knowing IDs.
