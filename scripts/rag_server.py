#!/usr/bin/env python3
"""Sidecar API RAG untuk UI test-llm (stdlib saja, tanpa flask).

  POST /api/rag/search {"query": "...", "top_k": 5}
    -> {"results": [{"source","file","title","cuplikan","skor"}]}
  GET  /api/rag/health -> {"ok": true, "rows": N}

Contoh (WSL, dari root repo):
    source ~/.venvs/rag/bin/activate
    python3 scripts/rag_server.py
    python3 scripts/rag_server.py --port 8000
"""
import argparse
import json
import os
import subprocess
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import psycopg2
import requests

EMBED_DIM = 768


def get_dsn(cli):
    if cli:
        return cli
    env = os.environ.get("DATABASE_URL")
    if env:
        return env
    return "postgres://agrisense:secret@localhost:5432/agrisense"


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


class Handler(BaseHTTPRequestHandler):
    server_version = "RAGSidecar/1.0"

    def log_message(self, *a):
        pass

    def _cors(self):
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")

    def _json(self, obj, code=200):
        body = json.dumps(obj, ensure_ascii=False).encode("utf-8")
        self.send_response(code)
        self._cors()
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_OPTIONS(self):
        self.send_response(204)
        self._cors()
        self.end_headers()

    def do_GET(self):
        if self.path == "/api/rag/health":
            try:
                with self.server.db() as conn:
                    with conn.cursor() as cur:
                        cur.execute("SELECT COUNT(*) FROM agronomy.rag_documents")
                        rows = (cur.fetchone() or [0])[0]
                self._json({"ok": True, "rows": rows})
            except Exception as e:
                self._json({"ok": False, "error": str(e)}, 500)
        else:
            self._json({"error": "not found"}, 404)

    def do_POST(self):
        if self.path != "/api/rag/search":
            self._json({"error": "not found"}, 404)
            return
        try:
            n = int(self.headers.get("Content-Length") or 0)
            payload = json.loads(self.rfile.read(n) or b"{}")
        except Exception:
            self._json({"error": "body JSON tak valid"}, 400)
            return
        query = str(payload.get("query", "")).strip()
        try:
            top_k = max(1, min(int(payload.get("top_k", 5)), 20))
        except Exception:
            top_k = 5
        if not query:
            self._json({"error": "query kosong"}, 400)
            return
        try:
            r = requests.post(self.server.ollama + "/api/embed",
                              json={"model": self.server.model, "input": query},
                              timeout=120)
            r.raise_for_status()
            vec = r.json()["embeddings"][0]
            if len(vec) != EMBED_DIM:
                raise RuntimeError(f"dimensi embedding {len(vec)} != {EMBED_DIM}")
            lit = "[" + ",".join(f"{x:.6f}" for x in vec) + "]"
            with self.server.db() as conn:
                with conn.cursor() as cur:
                    cur.execute(
                        """SELECT source, file_name, title,
                                  LEFT(content, 600) AS cuplikan,
                                  1 - (embedding <=> %s::vector) AS skor
                           FROM agronomy.rag_documents
                           ORDER BY embedding <=> %s::vector
                           LIMIT %s""", (lit, lit, top_k))
                    rows = cur.fetchall()
            self._json({"results": [
                {"source": s, "file": f, "title": t,
                 "cuplikan": c, "skor": round(float(sk), 4)}
                for s, f, t, c, sk in rows]})
        except Exception as e:
            self._json({"error": str(e)}, 500)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=8000)
    ap.add_argument("--host", default="0.0.0.0")
    ap.add_argument("--dsn", default="")
    ap.add_argument("--ollama-url", default="")
    ap.add_argument("--embed-model", default="nomic-embed-text")
    a = ap.parse_args()

    srv = ThreadingHTTPServer((a.host, a.port), Handler)
    srv.db = lambda: psycopg2.connect(get_dsn(a.dsn))
    srv.ollama = resolve_ollama(a.ollama_url)
    srv.model = a.embed_model
    with srv.db() as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT COUNT(*) FROM agronomy.rag_documents")
            print(f"RAG sidecar :8000 rows={(cur.fetchone() or [0])[0]} ollama={srv.ollama}",
                  flush=True)
    try:
        srv.serve_forever()
    except KeyboardInterrupt:
        pass
    return 0


if __name__ == "__main__":
    sys.exit(main())
