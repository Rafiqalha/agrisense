-- RAG document store for ingested open-data datasets.
-- Embeddings are 768-dim (nomic-embed-text, local Ollama) to match
-- agronomy.diseases.embedding. Ingest uses the schema-owner account;
-- runtime services get read-only access following least privilege.

BEGIN;

CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE IF NOT EXISTS agronomy.rag_documents (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    doc_id          TEXT NOT NULL,
    source          TEXT NOT NULL CHECK (source IN (
                        'satudata', 'datago', 'badanpangan', 'rag_pool'
                    )),
    file_name       TEXT NOT NULL,
    title           TEXT NOT NULL DEFAULT '',
    chunk_index     INTEGER NOT NULL DEFAULT 0 CHECK (chunk_index >= 0),
    content         TEXT NOT NULL CHECK (
                        char_length(content) BETWEEN 1 AND 20000
                    ),
    embedding       vector(768) NOT NULL,
    metadata        JSONB NOT NULL DEFAULT '{}'
                    CHECK (jsonb_typeof(metadata) = 'object'),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (doc_id, chunk_index)
);

CREATE INDEX IF NOT EXISTS idx_rag_documents_source
    ON agronomy.rag_documents (source);

CREATE INDEX IF NOT EXISTS idx_rag_documents_embedding
    ON agronomy.rag_documents USING hnsw (embedding vector_cosine_ops);

REVOKE ALL ON TABLE agronomy.rag_documents FROM PUBLIC;
GRANT USAGE ON SCHEMA agronomy TO agrisense_agronomy;
GRANT SELECT ON TABLE agronomy.rag_documents TO agrisense_agronomy;

COMMIT;
