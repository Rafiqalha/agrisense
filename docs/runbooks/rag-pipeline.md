# Runbook: Pipeline RAG Offline

Pipeline ini berjalan **di luar runtime layanan** (tidak ada di
`docker-compose.yml`). Endpoint runtime `ai-service` (`/rag/query`,
`/embeddings/generate`) masih mengembalikan HTTP 501; pipeline ini menyiapkan
dan menguji korpus terlebih dahulu.

## Komponen

| Bagian | Lokasi | Fungsi |
|---|---|---|
| Crawler | `tools/rag-crawler/` | Ambil halaman web → `dataset/agrisense_rag_pool.jsonl` |
| Downloader | `scripts/download_badanpangan.py`, `download_datago.py`, `download_satudata.py` | Unduh dataset terbuka → `dataset/<sumber>/` |
| Ingest | `scripts/rag_ingest.py` | Ekstraksi (csv/xlsx/xls/json/pdf/xml) → chunk → embedding → `agronomy.rag_documents` |
| Pencarian | `scripts/rag_search.py` | Verifikasi retrieval via CLI |
| Sidecar | `scripts/rag_server.py` | HTTP API `:8000` untuk UI (`/api/rag/health`, `/api/rag/search`) |
| UI uji | `test-llm/` | Playground LLM lokal (Ollama/LM Studio/llama.cpp) + toggle RAG |

## Prasyarat

- PostgreSQL lokal dengan database utama sudah dimigrasikan — `rag_ingest.py`
  juga menerapkan `infrastructure/postgres/migrations/202609220001_rag_documents.sql`
  secara otomatis.
- [Ollama](https://ollama.com) berjalan dengan model embedding:
  `ollama pull nomic-embed-text` (768 dimensi).
- Python 3 dengan paket: `psycopg2-binary`, `requests`, `openpyxl`, `xlrd`, `pypdf`.
- Dijalankan dari root repo (path `dataset/` dan migrasi bersifat relatif).

Semua perintah di bawah diasumsikan dari root repo. Nilai koneksi diambil dari
`DATABASE_URL`/`--dsn` dan `OLLAMA_URL`/`--ollama-url`; tanpa itu dipakai
default lokal (`postgres://agrisense:secret@localhost:5432/agrisense`,
`http://127.0.0.1:11434`).

## Alur

```bash
# 1. Siapkan korpus (pilih salah satu atau keduanya)
python3 scripts/download_satudata.py            # dataset terbuka SatuData Pertanian
python3 scripts/download_badanpangan.py         # dataset terbuka Badan Pangan
python3 scripts/download_datago.py              # dataset pertanian data.go.id
cargo run -p rag-crawler                        # crawler web (contoh: 3 URL bawaan)

# 2. Uji tanpa menulis apa pun
python3 scripts/rag_ingest.py --dry-run --limit-files 5

# 3. Ingest ke pgvector (aman diulang; resume via ON CONFLICT DO NOTHING)
python3 scripts/rag_ingest.py
python3 scripts/rag_ingest.py --sources rag_pool --reembed   # paksa tulis ulang

# 4. Verifikasi retrieval
python3 scripts/rag_search.py "berapa produksi padi tahun 2025?" --top-k 3

# 5. Sidecar + UI
python3 scripts/rag_server.py                   # http://localhost:8000
cd test-llm && npm install && npm run dev       # http://localhost:5174
```

## Catatan

- `dataset/` dan `runtime/` **tidak di-commit** — keduanya dapat dibuat ulang
  dengan skrip di atas.
- `RAG_POOL_PATH` menimpa path output crawler (default
  `dataset/agrisense_rag_pool.jsonl`).
- Tabel `agronomy.rag_documents` menyimpan teks + embedding 768-dimensi dengan
  index HNSW. Migrasi memberi `SELECT` ke role runtime `agrisense_agronomy`;
  skrip dev memakai akun pemilik, jadi jangan dipakai sebagai pola produksi.
- Sidecar membuka CORS `*` dan tanpa autentikasi — hanya untuk pengembangan
  lokal, bukan untuk diekspos ke jaringan publik.
- Jika korpus `rag_pool` diperbarui, jalankan
  `python3 scripts/rag_ingest.py --sources rag_pool --reembed` agar chunk lama
  ikut diperbarui (tanpa `--reembed`, chunk yang sudah ada dilewati).
