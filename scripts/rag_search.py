#!/usr/bin/env python3
"""Verifikasi retrieval RAG: pertanyaan -> embedding -> top-k pgvector.

Contoh (WSL, dari root repo):
    source ~/.venvs/rag/bin/activate
    python3 scripts/rag_search.py "berapa produksi padi tahun 2025?"
    python3 scripts/rag_search.py "harga beras konsumen" --top-k 3
"""
import argparse
import os
import subprocess
import sys

import requests


def resolve_ollama(cli):
    cands = []
    if cli:
        cands.append(cli)
    env = os.environ.get("OLLAMA_URL")
    if env:
        cands.append(env)
    cands.append("http://127.0.0.1:11434")
    try:
        gw = subprocess.check_output(["ip", "route", "show", "default"],
                                     text=True, timeout=5).split()
        if "default" in gw:
            cands.append(f"http://{gw[gw.index('default') + 2]}:11434")
    except Exception:
        pass
    for u in dict.fromkeys(u.rstrip("/") for u in cands):
        try:
            if requests.get(u + "/api/version", timeout=5).ok:
                return u
        except Exception:
            pass
    raise RuntimeError("Ollama tak terjangkau (coba --ollama-url)")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("query")
    ap.add_argument("--top-k", type=int, default=5)
    ap.add_argument("--dsn", default="")
    ap.add_argument("--ollama-url", default="")
    ap.add_argument("--embed-model", default="nomic-embed-text")
    a = ap.parse_args()

    import psycopg2

    ollama = resolve_ollama(a.ollama_url)
    r = requests.post(ollama + "/api/embed",
                      json={"model": a.embed_model, "input": a.query},
                      timeout=120)
    r.raise_for_status()
    vec = r.json()["embeddings"][0]
    print(f"dimensi query: {len(vec)}")

    dsn = a.dsn or os.environ.get("DATABASE_URL") or \
        "postgres://agrisense:secret@localhost:5432/agrisense"
    conn = psycopg2.connect(dsn)
    with conn.cursor() as cur:
        cur.execute(
            """SELECT source, file_name, title,
                      LEFT(content, 300) AS cuplikan,
                      1 - (embedding <=> %s::vector) AS skor
               FROM agronomy.rag_documents
               ORDER BY embedding <=> %s::vector
               LIMIT %s""",
            ("[" + ",".join(f"{x:.6f}" for x in vec) + "]",) * 2 + (a.top_k,))
        for i, (src, fn, title, cuplikan, skor) in enumerate(cur.fetchall(), 1):
            print(f"\n[{i}] skor={skor:.4f} sumber={src} file={fn}")
            print(f"    judul: {title}")
            print(f"    {cuplikan!s:.300s}")
    conn.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
