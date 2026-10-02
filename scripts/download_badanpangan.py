#!/usr/bin/env python3
"""Unduh dataset TERBUKA Badan Pangan Nasional.

Sumber : https://data.badanpangan.go.id/datasetpublications (409 dataset, 10/halaman)
Filter : Sifat == Terbuka saja (Terbatas/Tertutup butuh SAPA/login -> dilewati)
Link   : detail -> /download/document/dataset/{id}/{file}.{ext}/{ext} (langsung, tanpa auth)
Output : dataset/badanpangan/<kode>__<slug>.<ext> + _manifest.jsonl

Jalankan dari root repo (WSL):
    python3 scripts/download_badanpangan.py
    python3 scripts/download_badanpangan.py --formats csv
    python3 scripts/download_badanpangan.py --overwrite --delay 0.3
"""
import argparse
import html
import json
import os
import re
import ssl
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

BASE = "https://data.badanpangan.go.id"
LIST_URL = BASE + "/datasetpublications"
UA = {"User-Agent": "AgrisenseDatasetBot/1.0 (+agrisense.id)"}

CARD_RE = re.compile(r'<a\s+href="([^"]*?/datasetpublications/([^"/]+)/([^"]+))"', re.I)
DL_RE = re.compile(r'href="([^"]*?/download/document/dataset/[^"]+)"', re.I)
FMT_RE = re.compile(r'\.(csv|xlsx?|json|pdf)(?:/|$)', re.I)
COUNT_RE = re.compile(r"(\d+)\s+Dataset ditemukan")
TAG_RE = re.compile(r"<[^>]+>")


def fetch(url, timeout=30, retries=3):
    last = "unknown"
    for i in range(retries + 1):
        try:
            req = urllib.request.Request(url, headers=UA)
            with urllib.request.urlopen(req, timeout=timeout) as r:
                return r.read().decode("utf-8", "replace")
        except urllib.error.HTTPError as e:
            last = f"HTTP {e.code} {e.reason}"
            if e.code in (400, 401, 403, 404, 410):
                break
        except Exception as e:
            last = f"{type(e).__name__}: {e}"
        time.sleep(1.5 * (i + 1))
    raise RuntimeError(f"fetch gagal [{last}] :: {url}")


def fetch_bytes(url, timeout=60, retries=3):
    url = urllib.parse.quote(url, safe=":/?&=#%+@,;")
    last = "unknown"
    unverified = False
    for i in range(retries + 1):
        try:
            req = urllib.request.Request(url, headers=UA)
            ctx = ssl._create_unverified_context() if unverified else None
            with urllib.request.urlopen(req, timeout=timeout, context=ctx) as r:
                if unverified:
                    print(f"  [TLS tak-diverifikasi] {url}", file=sys.stderr)
                return r.read()
        except urllib.error.HTTPError as e:
            last = f"HTTP {e.code} {e.reason}"
            if e.code in (400, 401, 403, 404, 410):
                break
        except Exception as e:
            msg = f"{type(e).__name__}: {e}"
            if "CERTIFICATE_VERIFY_FAILED" in msg and not unverified:
                unverified = True
                last = msg + " -> ulangi tanpa verifikasi TLS"
                continue
            last = msg
        time.sleep(1.5 * (i + 1))
    raise RuntimeError(f"download gagal [{last}] :: {url}")


def clean(s):
    s = TAG_RE.sub(" ", s or "")
    return re.sub(r"\s+", " ", html.unescape(s)).strip()


def list_open_pages(max_pages=100):
    found = {}
    total = None
    page = 1
    while page <= max_pages:
        url = LIST_URL if page == 1 else f"{LIST_URL}?page={page}"
        try:
            body = fetch(url)
        except Exception as e:
            print(f"Halaman {page} gagal diambil: {e}", file=sys.stderr)
            break
        if total is None:
            m = COUNT_RE.search(clean(body))
            if m:
                total = int(m.group(1))
                print(f"Total di katalog: {total} dataset")
        cards = CARD_RE.findall(body)
        if not cards:
            break
        new = 0
        for href, code, slug in cards:
            href = html.unescape(href).strip()
            slug = slug.split("?")[0].strip()
            key = (code.strip(), slug)
            if key in found:
                continue
            end = body.find("</a>", body.find(href))
            block = body[body.find(href):end if end > 0 else len(body)]
            sifat = "?"
            for s in ("Terbuka", "Terbatas", "Tertutup"):
                if f">{s}<" in block:
                    sifat = s
                    break
            found[key] = {"code": key[0], "slug": key[1],
                          "detail_url": urllib.parse.urljoin(BASE, href),
                          "sifat": sifat}
            new += 1
        print(f"Halaman {page}: {len(cards)} kartu ({new} baru)")
        if new == 0:
            break
        page += 1
        time.sleep(0.3)
    return found, total


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default="dataset/badanpangan")
    ap.add_argument("--formats", default="csv,xlsx,json",
                    help="format yang diunduh, koma-dipisah (csv,xlsx,json)")
    ap.add_argument("--delay", type=float, default=0.5)
    ap.add_argument("--overwrite", action="store_true")
    ap.add_argument("--max-pages", type=int, default=100)
    a = ap.parse_args()
    wants = {f.strip().lower() for f in a.formats.split(",") if f.strip()}

    os.makedirs(a.out, exist_ok=True)
    manifest = os.path.join(a.out, "_manifest.jsonl")

    found, _ = list_open_pages(a.max_pages)
    opens = [v for v in found.values() if v["sifat"] == "Terbuka"]
    print(f"Kartu unik: {len(found)}, TERBUKA: {len(opens)} "
          f"(Terbatas/Tertutup dilewati, butuh SAPA)")

    ok, fail, skip = 0, 0, 0
    with open(manifest, "a", encoding="utf-8") as mf:
        for n, item in enumerate(opens, 1):
            rec = dict(item)
            try:
                detail = fetch(item["detail_url"])
            except Exception as e:
                rec["status"] = "detail-error"
                rec["error"] = str(e)
                mf.write(json.dumps(rec, ensure_ascii=False) + "\n")
                mf.flush()
                fail += 1
                print(f"[{n}/{len(opens)}] {item['slug']} GAGAL detail: {e}")
                time.sleep(a.delay)
                continue
            links = []
            for m in DL_RE.finditer(detail):
                u = urllib.parse.urljoin(BASE, html.unescape(m.group(1)).strip())
                links.append(u)
            seen, uniq = set(), []
            for u in links:
                if u not in seen:
                    seen.add(u)
                    uniq.append(u)
            rec["download_links"] = uniq
            got = []
            for u in uniq:
                path = urllib.parse.urlparse(u).path
                fm = FMT_RE.search(path)
                ext = fm.group(1).lower() if fm else ""
                if ext == "xls":
                    ext = "xlsx"
                if ext not in wants:
                    continue
                fname = f"{item['code']}__{item['slug']}.{ext}"
                dest = os.path.join(a.out, fname)
                if os.path.exists(dest) and not a.overwrite:
                    got.append({"file": fname, "status": "exists-skip"})
                    skip += 1
                    continue
                try:
                    data = fetch_bytes(u, timeout=60)
                    with open(dest, "wb") as f:
                        f.write(data)
                    got.append({"file": fname, "status": "ok", "bytes": len(data)})
                    ok += 1
                except Exception as e:
                    got.append({"file": fname, "status": "error", "error": str(e)})
                    fail += 1
            rec["files"] = got
            rec["status"] = "ok" if got and all(g.get("status") in ("ok", "exists-skip") for g in got) else "partial"
            mf.write(json.dumps(rec, ensure_ascii=False) + "\n")
            mf.flush()
            print(f"[{n}/{len(opens)}] {item['slug']}: "
                  + ", ".join(f"{g['file']}({g['status']})" for g in got))
            time.sleep(a.delay)

    print(f"Selesai. file-ok={ok} skip={skip} gagal={fail} manifest={manifest}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
