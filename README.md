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
# Edit .env: isi kredensial provider dan empat token internal yang berbeda.
# Contoh generator: openssl rand -hex 32

# 3. Build, migrasikan database, dan jalankan seluruh local stack
make local-up

# 4. Cek readiness
curl --fail http://localhost:3001/ready  # WhatsApp Gateway + PostgreSQL
curl --fail http://localhost:3002/ready  # Brain + PostgreSQL + Redis + NATS
set -a; source .env; set +a
curl -H "Authorization: Bearer $MCP_REGISTRATION_TOKEN" \
  http://localhost:3002/mcp/tools  # MCP Tool Registry
```

Untuk menjalankan binary langsung dari host selama development, gunakan
`make infra-up`, `make db-migrate`, kemudian target `make dev-*` di terminal
terpisah. Dalam container, Compose mengganti URL `localhost` dengan DNS service
internal secara otomatis.

Internal APIs fail closed. Gateway → Brain uses `GATEWAY_BRAIN_TOKEN`, Brain →
AI uses `BRAIN_AI_TOKEN`, Brain → Farm uses `BRAIN_FARM_TOKEN`, and MCP
registration/discovery uses `MCP_REGISTRATION_TOKEN`. Use different random
values of at least 32 bytes.
`POST /mcp/call` accepts a user JWT and derives ownership from its signed `sub`
claim; caller-provided farmer identifiers are not trusted for authorization.

WhatsApp outbound calls use `WHATSAPP_GRAPH_API_VERSION` (default `v25.0`) so
the Meta Graph API version can be upgraded without recompiling the gateway.
The durable inbound worker stores the generated AI reply before delivery. If
Meta delivery fails, retries reuse that exact reply instead of calling the AI
provider again. Successful delivery records the outbound WhatsApp message ID.

Voice notes use ElevenLabs Speech-to-Text before the transcript is sent to
Brain/Gemini. When the inbound message is audio, the final AI response is
converted with ElevenLabs Text-to-Speech, uploaded to Meta, and returned as a
WhatsApp audio message. The transcript, generated answer, and uploaded Meta
media ID are durable checkpoints, so retries do not repeat STT, AI generation,
or TTS. Configure the gateway in `.env`:

```dotenv
ELEVENLABS_API_KEY=isi-api-key-anda
ELEVENLABS_VOICE_ID=isi-voice-id-anda
ELEVENLABS_STT_MODEL=scribe_v2
ELEVENLABS_TTS_MODEL=eleven_multilingual_v2
ELEVENLABS_LANGUAGE_CODE=id
ELEVENLABS_TTS_OUTPUT_FORMAT=mp3_44100_128
```

If both `ELEVENLABS_API_KEY` and `ELEVENLABS_VOICE_ID` are empty, text and
image messages remain available but voice-note processing is disabled. A
partial configuration makes the gateway fail at startup instead of silently
using the wrong provider. Never commit the real API key.

Brain menyimpan maksimum 8 pesan terakhir per percakapan di Redis selama 24
jam (dapat diatur melalui `CONVERSATION_MEMORY_*`). Riwayat ini tersedia juga
untuk nomor WhatsApp yang belum terdaftar. Bytes/base64 gambar dan ID media Meta
tidak disimpan; pesan lanjutan menggunakan teks percakapan serta ringkasan
analisis sebelumnya. Prompt diagnosis memisahkan observasi dari dugaan,
meminta pembanding/verifikasi, dan melarang rekomendasi pestisida spesifik
sebelum tanaman serta penyebab cukup terkonfirmasi.

Nomor WhatsApp yang belum memiliki tanaman aktif masuk ke onboarding
deterministik: persetujuan penyimpanan data → nama → kebun → tanaman → varietas
→ tanggal tanam → luas → rangkuman → konfirmasi. Balasan `BATAL` menghentikan
alur tanpa membuat profil; data domain baru ditulis setelah balasan `YA` pada
rangkuman. Setiap transisi memakai UUID pesan inbound sehingga retry tidak
memajukan state dua kali. Gunakan `DAFTAR TANAMAN` untuk menambah tanaman pada
profil yang sudah ada. Melon disimpan sebagai `crop_type=melon`; jika pengguna
menulis jumlah greenhouse dan luas per greenhouse, jumlah unit, luas per unit,
dan luas total disimpan terpisah serta total dihitung secara deterministik.

Setelah onboarding selesai, aktivitas tanaman aktif dapat dicatat secara
deterministik dari WhatsApp. Model AI tidak diberi kewenangan menulis data.
Brain hanya menyusun draft terbatas, meminta persetujuan eksplisit, lalu
farm-service menyelesaikan pemilik, kebun, dan tanaman aktif dari identitas
nomor telepon yang telah diautentikasi. Contoh alurnya:

```text
User: CATAT hari ini menyiram semua greenhouse
Bot:  ... Balas YA untuk menyimpan, UBAH untuk memperbaiki, atau BATAL.
User: YA
Bot:  Aktivitas berhasil disimpan.

User: CATAT kemarin memupuk NPK 25 kg
User: YA
User: Kapan terakhir saya memupuk?
User: RIWAYAT AKTIVITAS
```

Aktivitas yang didukung adalah penyiraman, pemupukan, penyemprotan,
pemangkasan, dan inspeksi. UUID pesan pada setiap transisi pencatatan menjadi
kunci idempotensi: retry konfirmasi tidak menggandakan aktivitas atau event.
Penulisan aktivitas dan outbox NATS terjadi dalam satu transaksi; event baru
ditandai terpublikasi hanya setelah JetStream mengirim acknowledgement.

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
│   ├── ai-service/           Gemini text + vision
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

## Gemini Text + Vision

Seluruh panggilan AI aktif memakai Gemini API. Isi konfigurasi berikut di
`.env` (jangan masukkan API key ke Git atau log):

```dotenv
AI_PROVIDER=gemini
GEMINI_API_KEY=isi-api-key-anda
GEMINI_MODEL=gemini-3.6-flash
```

Integrasi mengikuti
[Gemini GenerateContent](https://ai.google.dev/api/generate-content) melalui
header `x-goog-api-key`. Thinking memakai level `minimal` agar respons tetap
berada dalam batas waktu delivery WhatsApp. API key wajib diisi sebelum AI
service bisa start.

`/generate` menerima `image_urls` opsional; Brain meneruskan media gambar WhatsApp
bersama konteks domain. `/vision/analyze` mendukung JPEG, PNG, GIF, WebP berupa
base64 data URI atau URL HTTP(S) publik. Batas lokal: body 16 MiB, maksimal
4 gambar per request, base64 maksimal 12 MiB per gambar. Path file lokal dan
audio ditolak. Nilai `confidence: 0.0` berarti confidence tidak tersedia,
bukan probabilitas diagnosis.

Speech transcription tetap ditangani ElevenLabs di gateway. Embedding,
moderation khusus, dan RAG belum diimplementasikan untuk deployment Gemini ini;
endpoint terkait mengembalikan HTTP 501. Tidak ada fallback ke DeepSeek,
OpenAI, Anthropic, atau Ollama. Source provider lama hanya disimpan sebagai arsip
dan tidak dikompilasi.

Setelah mengisi key, terapkan perubahan binary Brain dan AI:

```bash
docker compose --profile services up -d --build --wait
```

Tes mock HTTP dijalankan melalui `cargo test -p ai-service`; tes tersebut tidak
menghubungi Gemini atau membuktikan bahwa API key memiliki akses model vision.

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

Setiap service runtime yang mengakses database memakai login PostgreSQL
tersendiri, bukan pemilik schema. Service AI yang belum memiliki repository
database tidak menerima kredensial database sama sekali. Gateway hanya diberi
`SELECT`, `INSERT`, dan `UPDATE` pada antrean
inbound; Brain hanya mendapat tabel audit yang dilindungi RLS dan fungsi
onboarding/status konfirmasi yang sempit. Farm-service tidak memiliki akses
langsung ke tabel tenant; ia hanya mendapat `EXECUTE` pada fungsi aktivitas
yang mengambil scope nomor telepon dari transaksi. Service domain lain yang
endpoint-nya masih placeholder
hanya mendapat hak `CONNECT`—tanpa akses schema, tabel, sequence, atau fungsi—
sampai operasi datanya benar-benar diimplementasikan dan dapat diberi grant
spesifik. Data tenant menggunakan `FORCE ROW LEVEL SECURITY`, foreign key
kepemilikan, serta transaksi atomik dengan lock per nomor WhatsApp.

Sebelum production, ganti seluruh `*_DB_PASSWORD` dengan nilai acak, berbeda,
minimal 32 byte, dan URL-safe. Akun pemilik dari `POSTGRES_USER` hanya untuk
migrasi, tidak boleh dipakai container aplikasi. Startup production Brain dan
Gateway juga menolak password lokal bawaannya.

## Event Bus (NATS JetStream)

Stream: `AGRISENSE` | Subjects: `agrisense.<domain>.<event>`

```
agrisense.agronomy.disease_detected
agrisense.finance.loan_requested
agrisense.farm.harvest_recorded
agrisense.farm.activity_recorded
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
| AI | Gemini 3.6 Flash text + vision; ElevenLabs voice; RAG deferred |
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
