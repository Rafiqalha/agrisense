#!/usr/bin/env python3
"""Ingest dataset terbuka ke database utama sebagai RAG (pgvector).

Alur: dataset/{satudata,datago,badanpangan} + agrisense_rag_pool.jsonl
  -> ekstrak teks (csv/xlsx/xls/json/pdf/xml)
  -> chunk ~1200 char + overlap
  -> embedding lokal Ollama (nomic-embed-text, 768 dim)
  -> upsert agronomy.rag_documents (ON CONFLICT DO NOTHING = resume aman)

Contoh (WSL, dari root repo):
    source ~/.venvs/rag/bin/activate
    python3 scripts/rag_ingest.py --dry-run --limit-files 5
    python3 scripts/rag_ingest.py --limit-files 5
    python3 scripts/rag_ingest.py
    python3 scripts/rag_ingest.py --sources satudata --formats csv,xlsx
"""
import argparse
import csv
import hashlib
import json
import os
import re
import subprocess
import sys
import time

MIGRATION = "infrastructure/postgres/migrations/202609220001_rag_documents.sql"
SOURCES = ("satudata", "datago", "badanpangan")
SKIP_NAMES = {"_manifest.jsonl"}
EMBED_DIM = 768
FMT_PRIORITY = {"xlsx": 0, "csv": 1, "xls": 2, "json": 3, "pdf": 4,
                "xml": 5, "jsonl": 6, "txt": 7, "md": 8}


def mask_dsn(dsn):
    return re.sub(r"(://[^:/@]+:)[^@]+(@)", r"\1***\2", dsn or "")


def get_dsn(cli):
    if cli:
        return cli
    env = os.environ.get("DATABASE_URL")
    if env:
        return env
    return "postgres://agrisense:secret@localhost:5432/agrisense"


def resolve_ollama(cli):
    import requests
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
    seen = []
    for u in cands:
        u = u.rstrip("/")
        if u not in seen:
            seen.append(u)
    for u in seen:
        try:
            r = requests.get(u + "/api/version", timeout=5)
            if r.ok:
                return u
        except Exception:
            pass
    raise RuntimeError("Ollama tak terjangkau (coba --ollama-url). "
                       "Di Windows: setx OLLAMA_HOST \"0.0.0.0\" lalu restart Ollama.")


def doc_id(source, rel):
    return hashlib.md5(f"{source}::{rel}".encode("utf-8")).hexdigest()


def rows_to_text(rows):
    out = []
    for r in rows:
        cells = [f"{k}: {v}".strip() for k, v in r if str(v).strip() != ""]
        if cells:
            out.append(" | ".join(cells))
    return "\n".join(out)


def read_csv(path):
    text = None
    for enc in ("utf-8-sig", "cp1252"):
        try:
            with open(path, encoding=enc) as f:
                text = f.read(5_000_000)
            break
        except (UnicodeDecodeError, UnicodeError):
            continue
    if text is None:
        with open(path, encoding="utf-8", errors="replace") as f:
            text = f.read(5_000_000)
    import io
    buf = io.StringIO(text)
    try:
        dialect = csv.Sniffer().sniff(buf.read(65536), delimiters=";,|\t")
    except Exception:
        dialect = csv.excel
    buf.seek(0)
    reader = csv.reader(buf, dialect)
    try:
        header = next(reader)
    except StopIteration:
        return []
    header = [h.strip() or f"kol{i}" for i, h in enumerate(header)][:64]
    rows = []
    for i, row in enumerate(reader):
        if i >= 20000:
            break
        rows.append([(h, (row[j] if j < len(row) else "").strip())
                     for j, h in enumerate(header)])
    return [rows_to_text(rows)] if rows else []


def read_xlsx(path):
    from openpyxl import load_workbook
    wb = load_workbook(path, read_only=True, data_only=True)
    blocks = []
    for ws in wb.worksheets:
        header, rows, started = [], [], False
        for i, row in enumerate(ws.iter_rows(values_only=True)):
            if i > 20000:
                break
            vals = [(str(v).strip() if v is not None else "") for v in (row or [])[:64]]
            if not any(vals):
                continue
            if not started:
                header = [v or f"kol{j}" for j, v in enumerate(vals)]
                started = True
                continue
            rows.append([(header[j] if j < len(header) else f"kol{j}", vals[j])
                         for j in range(len(vals))])
        text = rows_to_text(rows)
        if text:
            blocks.append(f"[Sheet: {ws.title}]\n{text}")
    return blocks


def read_xls(path):
    import xlrd
    book = xlrd.open_workbook(path)
    blocks = []
    for s in book.sheets():
        if s.nrows == 0:
            continue
        header = [str(s.cell_value(0, c)).strip() or f"kol{c}"
                  for c in range(min(s.ncols, 64))]
        rows = []
        for r in range(1, min(s.nrows, 20001)):
            rows.append([(header[c], str(s.cell_value(r, c)).strip())
                         for c in range(min(s.ncols, 64))])
        text = rows_to_text(rows)
        if text:
            blocks.append(f"[Sheet: {s.name}]\n{text}")
    return blocks


def read_json(path):
    with open(path, encoding="utf-8") as f:
        data = json.load(f)
    if isinstance(data, list):
        return [json.dumps(x, ensure_ascii=False)[:4000] for x in data[:5000]
                if x is not None]
    if isinstance(data, dict):
        return [json.dumps(data, ensure_ascii=False)[:20000]]
    return [str(data)[:20000]]


def read_pdf(path):
    from pypdf import PdfReader
    out = []
    reader = PdfReader(str(path))
    for i, page in enumerate(reader.pages[:200]):
        try:
            t = (page.extract_text() or "").strip()
        except Exception:
            t = ""
        if t:
            out.append(f"[Halaman {i + 1}]\n{t}")
    return out


def read_xml(path):
    with open(path, encoding="utf-8", errors="replace") as f:
        raw = f.read(2_000_000)
    text = re.sub(r"<[^>]+>", " ", raw)
    return [re.sub(r"\s+", " ", text).strip()]


def read_rag_pool(path):
    blocks = []
    with open(path, encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                rec = json.loads(line)
            except Exception:
                continue
            if isinstance(rec, dict) and rec.get("text_context"):
                blocks.append((rec.get("doc_id"), str(rec["text_context"])))
    return blocks


def extract(source, path):
    ext = path.suffix.lower()
    if source == "rag_pool" or (ext == ".jsonl" and path.name != "_manifest.jsonl"):
        return read_rag_pool(path), True
    if ext == ".csv":
        return [(None, b) for b in read_csv(path)], False
    if ext == ".xlsx":
        return [(None, b) for b in read_xlsx(path)], False
    if ext == ".xls":
        return [(None, b) for b in read_xls(path)], False
    if ext == ".json":
        return [(None, b) for b in read_json(path)], False
    if ext == ".pdf":
        return [(None, b) for b in read_pdf(path)], False
    if ext == ".xml":
        return [(None, b) for b in read_xml(path)], False
    if ext in (".txt", ".md"):
        return [(None, path.read_text(encoding="utf-8", errors="replace")[:200000])], False
    return [], False


def chunk_text(text, size, overlap):
    text = re.sub(r"[ \t]+", " ", text).strip()
    if not text:
        return []
    if len(text) <= size:
        return [text]
    out, start = [], 0
    while start < len(text):
        end = min(start + size, len(text))
        cut = text.rfind("\n", start + size // 2, end)
        if cut > start:
            end = cut
        out.append(text[start:end].strip())
        if end >= len(text):
            break
        start = max(end - overlap, start + 1)
    return [c for c in out if c]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dsn", default="")
    ap.add_argument("--ollama-url", default="")
    ap.add_argument("--embed-model", default="nomic-embed-text")
    ap.add_argument("--sources", default="satudata,datago,badanpangan,rag_pool")
    ap.add_argument("--formats", default="")
    ap.add_argument("--limit-files", type=int, default=0)
    ap.add_argument("--chunk-size", type=int, default=1200)
    ap.add_argument("--overlap", type=int, default=150)
    ap.add_argument("--embed-batch", type=int, default=32)
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--reembed", action="store_true",
                    help="tulis ulang embedding yang sudah ada")
    ap.add_argument("--keep-all-formats", action="store_true",
                    help="jangan deduplikasi basename yang sama beda format")
    a = ap.parse_args()

    import requests
    try:
        import psycopg2
        PSYCOPG_MISS = False
    except ImportError:
        PSYCOPG_MISS = True

    wants_fmt = {f.strip().lower().lstrip(".") for f in a.formats.split(",") if f.strip()}
    sources = [s.strip() for s in a.sources.split(",") if s.strip()]

    files = []
    for src in sources:
        base = os.path.join("dataset", src) if src != "rag_pool" else "dataset"
        if src == "rag_pool":
            p = os.path.join("dataset", "agrisense_rag_pool.jsonl")
            if os.path.exists(p):
                files.append((src, p))
            continue
        if not os.path.isdir(base):
            print(f"lewati sumber {src}: folder {base} tak ada")
            continue
        for root, _, names in os.walk(base):
            for n in sorted(names):
                if n in SKIP_NAMES or n.startswith("."):
                    continue
                ext = os.path.splitext(n)[1].lower().lstrip(".")
                if wants_fmt and ext not in wants_fmt:
                    continue
                files.append((src, os.path.join(root, n)))
    files.sort(key=lambda x: x[1])
    if not a.keep_all_formats:
        best, dup = {}, 0
        for src, path in files:
            stem = os.path.splitext(os.path.basename(path))[0].lower()
            key = (src, stem)
            pri = FMT_PRIORITY.get(os.path.splitext(path)[1].lower().lstrip("."), 99)
            if key not in best or pri < best[key][0]:
                if key in best:
                    dup += 1
                best[key] = (pri, src, path)
            else:
                dup += 1
        files = [(s, p) for _, s, p in sorted(best.values())]
        if dup:
            print(f"Deduplikasi format: {dup} file dilewati (format prioritas dipakai)")
    if a.limit_files > 0:
        files = files[:a.limit_files]
    print(f"File kandidat: {len(files)}")

    ollama = "" if a.dry_run else resolve_ollama(a.ollama_url)
    if ollama:
        print(f"Ollama: {ollama} model={a.embed_model}")

    dsn = get_dsn(a.dsn)
    conn = None if a.dry_run or PSYCOPG_MISS else psycopg2.connect(dsn)
    if PSYCOPG_MISS and not a.dry_run:
        print("psycopg2 tak ada: pip install psycopg2-binary", file=sys.stderr)
        return 2
    if conn:
        conn.autocommit = True
        with open(MIGRATION, encoding="utf-8") as f:
            with conn.cursor() as cur:
                cur.execute(f.read())
        print(f"DDL {MIGRATION} teraplikasi di {mask_dsn(dsn)}")

    n_files, n_chunks, n_new, n_err = 0, 0, 0, 0
    for src, path in files:
        from pathlib import Path
        p = Path(path)
        rel = os.path.relpath(path, os.path.join("dataset", src) if src != "rag_pool" else "dataset")
        if not a.dry_run and not a.reembed and conn is not None and src != "rag_pool":
            with conn.cursor() as cur:
                cur.execute("SELECT COUNT(*) FROM agronomy.rag_documents WHERE doc_id=%s",
                            (doc_id(src, rel),))
                already = (cur.fetchone() or [0])[0]
            if already > 0:
                print(f"  = {rel}: {already} chunk sudah di DB, lewati")
                continue
        try:
            blocks, has_ids = extract(src, p)
        except Exception as e:
            print(f"  ! {rel}: ekstrak gagal: {e}")
            n_err += 1
            continue
        items = []
        for bid, b in blocks:
            for ci, ch in enumerate(chunk_text(b, a.chunk_size, a.overlap)):
                did = bid if has_ids and bid else doc_id(src, rel)
                ctx = ch if has_ids else f"Sumber: {src} | File: {p.name}\n{ch}"
                items.append((did, ci if not has_ids else 0, ctx))
        if not items:
            continue
        if not a.keep_all_formats:
            uniq, seen_h = [], set()
            for did, _, ctx in items:
                h = hashlib.md5(re.sub(r"\s+", " ", ctx).encode()).hexdigest()
                if h not in seen_h:
                    seen_h.add(h)
                    uniq.append((did, ctx))
            items = [(did, i, ctx) for i, (did, ctx) in enumerate(uniq)]
            if not items:
                continue
        n_files += 1
        n_chunks += len(items)
        if a.dry_run:
            print(f"  . {rel}: {len(blocks)} blok -> {len(items)} chunk")
            continue
        vecs = []
        try:
            for i in range(0, len(items), a.embed_batch):
                batch = [c for _, _, c in items[i:i + a.embed_batch]]
                last_err: Exception | None = None
                for attempt in range(6):
                    try:
                        r = requests.post(ollama + "/api/embed",
                                          json={"model": a.embed_model, "input": batch},
                                          timeout=300)
                        r.raise_for_status()
                        break
                    except Exception as e:
                        last_err = e
                        time.sleep(5 * (attempt + 1))
                else:
                    raise last_err  # type: ignore[misc]
                vecs.extend(r.json()["embeddings"])
        except Exception as e:
            print(f"  ! {rel}: embed gagal setelah retry, lewati file: {e}")
            n_err += 1
            continue
        if any(len(v) != EMBED_DIM for v in vecs):
            print(f"  ! {rel}: dimensi embedding bukan {EMBED_DIM}, abort")
            return 3
        rows = [(did, src, p.name, p.stem.replace("_", " ")[:200], ci, ctx,
                 "[" + ",".join(f"{x:.6f}" for x in v) + "]",
                 json.dumps({"rel": rel}, ensure_ascii=False))
                for (did, ci, ctx), v in zip(items, vecs)]
        with conn.cursor() as cur:
            if a.reembed:
                cur.executemany(
                    """INSERT INTO agronomy.rag_documents
                       (doc_id, source, file_name, title, chunk_index, content, embedding, metadata)
                       VALUES (%s,%s,%s,%s,%s,%s,%s::vector,%s::jsonb)
                       ON CONFLICT (doc_id, chunk_index) DO UPDATE SET
                         content=EXCLUDED.content, embedding=EXCLUDED.embedding,
                         metadata=EXCLUDED.metadata""", rows)
            else:
                cur.executemany(
                    """INSERT INTO agronomy.rag_documents
                       (doc_id, source, file_name, title, chunk_index, content, embedding, metadata)
                       VALUES (%s,%s,%s,%s,%s,%s,%s::vector,%s::jsonb)
                       ON CONFLICT (doc_id, chunk_index) DO NOTHING""", rows)
            n_new += cur.rowcount
        print(f"  + {rel}: {len(items)} chunk, {cur.rowcount} baru")
    if conn:
        conn.close()
    print(f"Selesai. file={n_files} chunk={n_chunks} baru={n_new} gagal_ekstrak={n_err}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
