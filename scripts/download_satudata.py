#!/usr/bin/env python3
"""Unduh semua dataset gratis SatuData Pertanian.

Sumber : https://satudata.pertanian.go.id/datasets
Pola   : list -> detail_data/{id} -> [UNDUH DOKUMEN](.../assets/docs/...)
Output : dataset/satudata/<nama-file-asli> + _manifest.jsonl

Jalankan dari root repo (WSL):
    python3 scripts/download_satudata.py
    python3 scripts/download_satudata.py --overwrite
    python3 scripts/download_satudata.py --out dataset/satudata --delay 0.3
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

BASE = "https://satudata.pertanian.go.id"
LIST_URL = BASE + "/datasets"
UA = {"User-Agent": "AgrisenseDatasetBot/1.0 (+agrisense.id)"}

ID_RE = re.compile(r"/datasets/detail_data/(\d+)")
DL_RE = re.compile(r'href="([^"]*assets/docs/[^"]+)"', re.I)
TITLE_RE = re.compile(r"###\s*(.+?)\s*\n+---", re.S)
H_TITLE_RE = re.compile(r"<h[13][^>]*>(.*?)</h[13]>", re.S | re.I)
TAG_RE = re.compile(r"<[^>]+>")


def fetch(url, timeout=30):
    req = urllib.request.Request(url, headers=UA)
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return r.read().decode("utf-8", "replace")


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
    s = html.unescape(s)
    return re.sub(r"\s+", " ", s).strip()


def list_ids():
    page = fetch(LIST_URL)
    ids = ID_RE.findall(page)
    seen, out = set(), []
    for i in ids:
        if i not in seen:
            seen.add(i)
            out.append(i)
    return out


def detail(did):
    url = f"{BASE}/datasets/detail_data/{did}"
    try:
        page = fetch(url)
    except Exception as e:
        return {"id": did, "detail_url": url, "error": f"detail fetch: {e}"}
    m = DL_RE.search(page)
    dl = html.unescape(m.group(1)).strip() if m else ""
    if dl:
        dl = urllib.parse.urljoin(BASE + "/", dl)
    title = ""
    for h in H_TITLE_RE.findall(page):
        t = clean(h)
        if not t:
            continue
        low = t.lower()
        if "portal satu data" in low or "kembali ke home" in low:
            continue
        if len(t) > len(title):
            title = t
    return {"id": did, "detail_url": url, "title": title, "download_url": dl}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default="dataset/satudata")
    ap.add_argument("--delay", type=float, default=0.5)
    ap.add_argument("--overwrite", action="store_true")
    ap.add_argument("--timeout", type=int, default=60)
    a = ap.parse_args()

    os.makedirs(a.out, exist_ok=True)
    manifest = os.path.join(a.out, "_manifest.jsonl")

    try:
        ids = list_ids()
    except Exception as e:
        print(f"Gagal ambil daftar datasets: {e}", file=sys.stderr)
        return 1
    print(f"Total dataset di daftar: {len(ids)}")

    ok, fail, skip = 0, 0, 0
    with open(manifest, "a", encoding="utf-8") as mf:
        for n, did in enumerate(ids, 1):
            info = detail(did)
            dl = info.get("download_url", "")
            if not dl:
                info["status"] = "no-download-link"
                mf.write(json.dumps(info, ensure_ascii=False) + "\n")
                mf.flush()
                fail += 1
                print(f"[{n}/{len(ids)}] id={did} TANPA LINK UNDUH :: {info.get('title','')}")
                time.sleep(a.delay)
                continue
            fname = os.path.basename(urllib.parse.urlparse(dl).path) or f"{did}.bin"
            fname = urllib.parse.unquote(fname)
            dest = os.path.join(a.out, fname)
            info["file"] = fname
            if os.path.exists(dest) and not a.overwrite:
                info["status"] = "exists-skip"
                mf.write(json.dumps(info, ensure_ascii=False) + "\n")
                mf.flush()
                skip += 1
                print(f"[{n}/{len(ids)}] id={did} SKIP (ada) :: {fname}")
                time.sleep(a.delay)
                continue
            try:
                data = fetch_bytes(dl, timeout=a.timeout)
                with open(dest, "wb") as f:
                    f.write(data)
                info["status"] = "ok"
                info["bytes"] = len(data)
                ok += 1
                print(f"[{n}/{len(ids)}] id={did} OK {len(data)}B :: {fname}")
            except Exception as e:
                info["status"] = "error"
                info["error"] = str(e)
                fail += 1
                print(f"[{n}/{len(ids)}] id={did} GAGAL :: {e}")
            mf.write(json.dumps(info, ensure_ascii=False) + "\n")
            mf.flush()
            time.sleep(a.delay)

    print(f"Selesai. ok={ok} skip={skip} gagal={fail} manifest={manifest}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
