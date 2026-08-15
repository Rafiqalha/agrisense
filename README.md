<div align="center">

# 🌾 AgriSense

**Operating System untuk Aktivitas Ekonomi Pertanian Berbasis Percakapan**

[![Rust](https://img.shields.io/badge/Rust-1.80+-orange.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/License-UNLICENSED-red.svg)]()
[![Architecture](https://img.shields.io/badge/Architecture-DDD%20%2B%20Event--Driven%20%2B%20MCP-blue.svg)]()

</div>

---

AgriSense bukan chatbot pertanian.

AgriSense adalah **Operating System untuk seluruh aktivitas ekonomi pertanian** — dimulai dari WhatsApp, berkembang menjadi platform untuk jutaan petani Indonesia.

## Architecture Principles

> **Event-Driven + Domain-Driven + MCP-Centric + AI-as-a-Service**

AI bukan pusat sistem. **Domain pertanian adalah pusat sistem.** AI adalah mesin yang membantu domain tersebut.

```
Monorepo → Domain-Driven Design → Event-Driven → Modular Services → Microservices Bertahap
```

## Quick Start

```bash
# 1. Clone dan setup
git clone https://github.com/agrisense/agrisense
cd agrisense
make setup

# 2. Konfigurasi environment
cp .env.example .env
# Edit .env: isi GEMINI_API_KEY, WHATSAPP_ACCESS_TOKEN, dst.

# 3. Jalankan infrastruktur
make infra-up

# 4. Jalankan services (development mode)
make dev-brain    # Terminal 1
make dev-farm     # Terminal 2
make dev-gateway  # Terminal 3

# 5. Cek semua berjalan
curl http://localhost:3001/health  # WhatsApp Gateway
curl http://localhost:3002/health  # Brain Service
curl http://localhost:3002/mcp/tools  # MCP Tool Registry
```

## Repository Structure

```
agrisense/
│
├── apps/
│   └── whatsapp-gateway/     Menerima webhook Meta WhatsApp Business API
│
├── services/
│   ├── brain-service/        Orchestrator + MCP Registry (jantung sistem)
│   ├── farm-service/         Operasional pertanian (lahan, tanaman, panen)
│   ├── agronomy-service/     Knowledge base penyakit, hama, rekomendasi
│   ├── finance-service/      Transaksi, cashflow, kredit scoring
│   ├── marketplace-service/  Produk pertanian, kios, supplier
│   ├── analytics-service/    Tren penyakit, yield, insight regional
│   ├── ai-service/           Gemini Vision, Whisper STT, RAG, pgvector
│   └── platform-service/     Identity, notifikasi, media
│
├── packages/
│   ├── shared-types/         Domain types (FarmerId, CropType, Money)
│   ├── shared-events/        Event catalog + protobuf schemas
│   ├── shared-mcp/           MCP protocol (McpTool, McpToolHandler trait)
│   ├── shared-db/            PostgreSQL pool + sqlx
│   ├── shared-cache/         Redis client
│   ├── shared-auth/          JWT + session management
│   └── shared-observability/ OpenTelemetry + Prometheus + tracing
│
├── contracts/
│   ├── grpc/                 Protobuf service definitions
│   ├── openapi/              REST API specs
│   └── events/               Event schema catalog
│
├── infrastructure/
│   ├── postgres/schemas/     6 domain schemas + pgvector
│   ├── redis/                Redis config
│   ├── nats/                 NATS JetStream config
│   └── monitoring/           Prometheus, Grafana, Loki, Alertmanager
│
├── deployments/
│   ├── k8s/base/             Kubernetes base manifests per service
│   ├── k8s/staging/          Kustomize overlay — staging
│   └── k8s/production/       Kustomize overlay — production (HA)
│
└── docs/
    ├── architecture/         Overview, DDD boundaries, event catalog
    └── architecture/adr/     Architecture Decision Records
```

## Services & Ports

| Service | Port | Domain |
|---------|------|--------|
| whatsapp-gateway | 3001 | WhatsApp Business API |
| brain-service | 3002 | Orchestration + MCP Registry |
| farm-service | 3003 | Farm operations |
| agronomy-service | 3004 | Agronomy knowledge |
| finance-service | 3005 | Finance & credit |
| marketplace-service | 3006 | Products & commerce |
| analytics-service | 3007 | Data insights |
| ai-service | 3008 | AI providers |
| platform-service | 3009 | Identity, notification, media |

## MCP Tool Registry

brain-service menjadi pusat registrasi semua tools yang dapat dipanggil AI agent.

Setiap service mendaftarkan toolsnya saat startup:

```
get_farmer_profile        → platform-service
get_farm_status           → farm-service
get_crop_status           → farm-service
detect_disease            → ai-service (Gemini Vision)
recommend_fertilizer      → agronomy-service
check_weather             → external (BMKG)
record_expense            → finance-service
check_credit_score        → finance-service
request_kur               → finance-service
list_marketplace_products → marketplace-service
```

Agent tidak perlu tahu implementasi. Cukup panggil tool dari registry.

## AI Provider Abstraction

Swap model tanpa mengubah satu baris agent code:

```rust
// services/ai-service/src/providers/mod.rs
#[async_trait]
pub trait AiProvider {
    fn name(&self) -> &str;
    async fn generate(&self, request: GenerateRequest) -> anyhow::Result<GenerateResponse>;
    async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>>;
    async fn classify_intent(&self, message: &str, context: &str) -> anyhow::Result<String>;
    async fn analyze_image(&self, image_url: &str, prompt: &str) -> anyhow::Result<String>;
}

// Implementations: GeminiProvider, OpenAiProvider, AnthropicProvider, LocalProvider
// Switch via: AI_PROVIDER=gemini|openai|anthropic|local
```

## Database Design

Satu PostgreSQL cluster, schema terisolasi per domain:

```sql
identity    -- platform-service (users, sessions)
farm        -- farm-service (farmers, farms, crops, harvests)
agronomy    -- agronomy-service (diseases + pgvector embeddings)
finance     -- finance-service (transactions, credit scores, loans)
ai          -- brain + ai-service (conversations, agent runs)
analytics   -- analytics-service (trends, aggregates — future B2B)
```

Cross-domain communication = events, **bukan JOINs**.

## Event Bus (NATS JetStream)

Stream: `AGRISENSE` | Subjects: `agrisense.<domain>.<event>`

```
agrisense.agronomy.disease_detected
agrisense.finance.loan_requested
agrisense.farm.harvest_recorded
agrisense.ai.agent_run_completed
```

→ analytics-service subscribe ke `agrisense.>` untuk aggregasi semua events.

## Monitoring

```bash
make monitoring-up

# Grafana:    http://localhost:3000 (admin/admin)
# Prometheus: http://localhost:9090
# Loki:       http://localhost:3100
```

## Developer Commands

```bash
make help            # Lihat semua commands
make setup           # Initial setup
make infra-up        # Start Postgres + Redis + NATS
make build           # Build semua Rust services
make test            # Run semua tests
make lint            # Clippy + fmt check
make db-migrate      # Run database migrations
make proto-gen       # Generate code dari .proto files
make k8s-apply-staging  # Deploy ke staging
```

## Architecture Decision Records

| ADR | Keputusan |
|-----|-----------|
| [001](docs/architecture/adr/001-rust-over-go.md) | Rust over Go |
| [002](docs/architecture/adr/002-mcp-protocol.md) | MCP as integration protocol |
| [003](docs/architecture/adr/003-whatsapp-first.md) | WhatsApp First |
| [004](docs/architecture/adr/004-nats-over-kafka.md) | NATS JetStream over Kafka |

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Language | Rust (Axum, tokio, sqlx) |
| AI | Gemini Vision, Whisper, pgvector RAG |
| Protocol | MCP (Model Context Protocol) |
| Database | PostgreSQL + pgvector |
| Cache | Redis |
| Events | NATS JetStream |
| Container | Docker |
| Orchestration | GKE (Kustomize) |
| Storage | Google Cloud Storage |
| Monitoring | Prometheus + Grafana + Loki |

---

*AgriSense — Dibangun untuk 1 juta petani Indonesia.*
