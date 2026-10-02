#!/usr/bin/env python3
"""Unduh dataset bagian PERTANIAN dari Satu Data Indonesia (data.go.id).

Sumber : https://data.go.id/dataset?q=pertanian (10.541 hasil, 20/halaman)
Pola   : list -> /dataset/dataset/{slug} -> JSON tertanam
         resources[] {url, format} (file asli di portal walidata/daerah)
         badge Terlihat + extras accesslevel -> Tertutup/Terbatas dilewati
         (public/terbuka/open, atau tanpa info eksplisit -> dicoba unduh)
Output : dataset/datago/<slug>__<namafile> + _manifest.jsonl

Contoh (WSL, dari root repo):
    python3 scripts/download_datago.py --max-pages 5
    python3 scripts/download_datago.py --formats csv,xlsx
    python3 scripts/download_datago.py --query "padi" --limit 50
    python3 scripts/download_datago.py --include-restricted
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

BASE = "https://data.go.id"
UA = {"User-Agent": "AgrisenseDatasetBot/1.0 (+agrisense.id)"}

SLUG_RE = re.compile(r'href="(/dataset/dataset/([^"/]+))"', re.I)
COUNT_RE = re.compile(r"([\d.,]+)\s*Datasets Found")
RES_RE = re.compile(r'\{[^{}]*?"url"\s*:\s*"(https?://[^"]+)"[^{}]*?\}')
FMT_RE = re.compile(r'"format"\s*:\s*"([^"]*)"')
LVL_RE = re.compile(r'"key"\s*:\s*"accesslevel"\s*,\s*"value"\s*:\s*"([^"]*)"')
BADGE_RE = re.compile(r'>(Terbuka|Tertutup|Terbatas)<')
NONFILE_FORMATS = {"html", "htm"}
PUBLIC_LVLS = {"public", "terbuka", "open", "1", "true", "yes"}
CLOSED_LVLS = {"tertutup", "terbatas", "restricted", "private", "closed", "limited", "0", "false"}
TITLE_RE = re.compile(r'"title"\s*:\s*"([^"]{4,300})"')
ORG_RE = re.compile(r'"organization"\s*:\s*\{[^{}]*?"title"\s*:\s*"([^"]+)"')


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


_TLS_FALLBACK = set()


def fetch_bytes(url, timeout=30, retries=2, max_mb=200):
    url = urllib.parse.quote(url, safe=":/?&=#%+@,;")
    last = "unknown"
    unverified = False
    limit = int(max_mb * 1024 * 1024)
    for i in range(retries + 1):
        try:
            req = urllib.request.Request(url, headers=UA)
            ctx = ssl._create_unverified_context() if unverified else None
            with urllib.request.urlopen(req, timeout=timeout, context=ctx) as r:
                if unverified:
                    _TLS_FALLBACK.add(url)
                    print(f"  [TLS tak-diverifikasi] {url}", file=sys.stderr)
                ln = r.headers.get("Content-Length")
                if ln and ln.isdigit() and int(ln) > limit:
                    raise RuntimeError(f"too large ({ln}B > {max_mb}MB)")
                chunks, total = [], 0
                while True:
                    b = r.read(1 << 20)
                    if not b:
                        break
                    chunks.append(b)
                    total += len(b)
                    if total > limit:
                        raise RuntimeError(f"too large (>{max_mb}MB)")
                return b"".join(chunks)
        except urllib.error.HTTPError as e:
            last = f"HTTP {e.code} {e.reason}"
            if e.code in (400, 401, 403, 404, 410):
                break
        except Exception as e:
            msg = f"{type(e).__name__}: {e}"
            if "too large" in msg:
                raise RuntimeError(f"{msg} :: {url}")
            if "CERTIFICATE_VERIFY_FAILED" in msg and not unverified:
                unverified = True
                last = msg + " -> ulangi tanpa verifikasi TLS"
                continue
            last = msg
        time.sleep(1.5 * (i + 1))
    raise RuntimeError(f"download gagal [{last}] :: {url}")


def clean(s):
    return re.sub(r"\s+", " ", html.unescape(s or "")).strip()


def norm_page(body):
    return body.replace('\\"', '"').replace("\\/", "/")


def list_slugs(query, max_pages):
    q = urllib.parse.quote(query)
    slugs, total, page = [], None, 1
    while page <= max_pages:
        url = f"{BASE}/dataset?q={q}" if page == 1 else f"{BASE}/dataset?q={q}&page={page}"
        try:
            body = fetch(url)
        except Exception as e:
            print(f"Halaman {page} gagal: {e}", file=sys.stderr)
            break
        if total is None:
            m = COUNT_RE.search(clean(body))
            if m:
                total = m.group(1)
                print(f"Total hasil {query!r}: {total} dataset")
        found = SLUG_RE.findall(body)
        if not found:
            break
        new = 0
        for _, slug in found:
            slug = slug.split("?")[0]
            if slug not in slugs:
                slugs.append(slug)
                new += 1
        print(f"Halaman {page}: {len(found)} kartu ({new} slug baru)")
        if new == 0:
            break
        page += 1
        time.sleep(0.3)
    return slugs


def detail(slug):
    url = f"{BASE}/dataset/dataset/{slug}"
    try:
        raw = fetch(url)
    except Exception as e:
        return {"slug": slug, "detail_url": url, "error": f"detail: {e}"}
    body = norm_page(raw)
    m = LVL_RE.search(body)
    level = m.group(1).strip().lower() if m else "?"
    bm = BADGE_RE.search(raw)
    badge = bm.group(1).lower() if bm else "?"
    t = TITLE_RE.search(body)
    o = ORG_RE.search(body)
    res = []
    for rm in RES_RE.finditer(body):
        obj = rm.group(0)
        um = re.search(r'"url"\s*:\s*"(https?://[^"]+)"', obj)
        if not um:
            continue
        fm = FMT_RE.search(obj)
        res.append({"url": html.unescape(um.group(1)),
                    "format": (fm.group(1) if fm else "").strip().lower()})
    seen, uniq = set(), []
    for r in res:
        if r["url"] not in seen:
            seen.add(r["url"])
            uniq.append(r)
    return {"slug": slug, "detail_url": url,
            "title": clean(t.group(1)) if t else slug,
            "organization": clean(o.group(1)) if o else "",
            "accesslevel": level, "badge": badge, "resources": uniq}


def safe_name(s):
    s = urllib.parse.unquote(s)
    return re.sub(r'[<>:"/\\|?*\x00-\x1f]', "_", s).strip()[:180] or "file"


def load_done(manifest):
    done = set()
    try:
        with open(manifest, encoding="utf-8") as f:
            for line in f:
                try:
                    rec = json.loads(line)
                except Exception:
                    continue
                if rec.get("slug"):
                    if rec.get("status") == "ok":
                        done.add(rec["slug"])
                    else:
                        done.discard(rec["slug"])
    except FileNotFoundError:
        pass
    return done


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--query", default="pertanian")
    ap.add_argument("--out", default="dataset/datago")
    ap.add_argument("--formats", default="",
                    help="filter format, koma-dipisah (csv,xlsx,json,pdf); kosong = semua")
    ap.add_argument("--delay", type=float, default=0.5)
    ap.add_argument("--timeout", type=int, default=30,
                    help="timeout detik per percobaan unduh")
    ap.add_argument("--retries", type=int, default=2,
                    help="pengulangan untuk error jaringan sementara")
    ap.add_argument("--max-mb", type=float, default=200,
                    help="lewati file lebih besar dari ini (MB)")
    ap.add_argument("--overwrite", action="store_true")
    ap.add_argument("--max-pages", type=int, default=528)
    ap.add_argument("--limit", type=int, default=0,
                    help="batasi jumlah dataset (0 = tanpa batas)")
    ap.add_argument("--include-restricted", action="store_true",
                    help="ikutkan accesslevel non-public")
    a = ap.parse_args()
    wants = {f.strip().lower() for f in a.formats.split(",") if f.strip()}

    os.makedirs(a.out, exist_ok=True)
    manifest = os.path.join(a.out, "_manifest.jsonl")

    slugs = list_slugs(a.query, a.max_pages)
    if a.limit > 0:
        slugs = slugs[:a.limit]
    if not a.overwrite:
        already = load_done(manifest)
        before = len(slugs)
        slugs = [s for s in slugs if s not in already]
        if before - len(slugs):
            print(f"Resume: {before - len(slugs)} slug sudah ok dilewati")
    print(f"Slug pertanian terkumpul: {len(slugs)}")

    ok, fail, skip = 0, 0, 0
    with open(manifest, "a", encoding="utf-8") as mf:
        try:
            for n, slug in enumerate(slugs, 1):
                info = detail(slug)
                if "error" in info and not info.get("resources"):
                    info["status"] = "detail-error"
                    mf.write(json.dumps(info, ensure_ascii=False) + "\n")
                    mf.flush()
                    fail += 1
                    print(f"[{n}/{len(slugs)}] {slug} GAGAL detail")
                    time.sleep(a.delay)
                    continue
                lvl = info.get("accesslevel", "?")
                bdg = info.get("badge", "?")
                closed = lvl in CLOSED_LVLS or bdg in ("tertutup", "terbatas")
                if closed and not a.include_restricted:
                    info["status"] = "restricted-skip"
                    mf.write(json.dumps(info, ensure_ascii=False) + "\n")
                    mf.flush()
                    skip += 1
                    print(f"[{n}/{len(slugs)}] {slug} SKIP (level={lvl}/badge={bdg})")
                    time.sleep(a.delay)
                    continue
                got, skipped = [], []
                for r in info.get("resources", []):
                    ext = r["format"] or r["url"].rsplit(".", 1)[-1].lower()
                    if r["format"] in NONFILE_FORMATS and r["format"] not in wants:
                        skipped.append({"status": "viewer-skip", "format": r["format"],
                                        "url": r["url"]})
                        continue
                    if wants and ext not in wants and r["format"] not in wants:
                        continue
                    base = os.path.basename(urllib.parse.urlparse(r["url"]).path) or "res"
                    if "." not in base and ext:
                        base = f"{base}.{ext}"
                    fname = safe_name(f"{slug}__{base}")
                    dest = os.path.join(a.out, fname)
                    if os.path.exists(dest) and not a.overwrite:
                        got.append({"file": fname, "status": "exists-skip",
                                    "format": r["format"], "url": r["url"]})
                        skip += 1
                        continue
                    try:
                        data = fetch_bytes(r["url"], timeout=a.timeout,
                                           retries=a.retries, max_mb=a.max_mb)
                        with open(dest, "wb") as f:
                            f.write(data)
                        got.append({"file": fname, "status": "ok",
                                    "bytes": len(data), "format": r["format"], "url": r["url"],
                                    "tls": ("unverified" if r["url"] in _TLS_FALLBACK else "verified")})
                        ok += 1
                    except Exception as e:
                        got.append({"file": fname, "status": "error",
                                    "error": str(e), "format": r["format"], "url": r["url"]})
                        fail += 1
                info["files"] = got + skipped
                if got and all(g["status"] in ("ok", "exists-skip") for g in got):
                    info["status"] = "ok"
                elif not got and skipped:
                    info["status"] = "viewer-only"
                elif not got:
                    info["status"] = "no-resource"
                else:
                    info["status"] = "partial"
                mf.write(json.dumps(info, ensure_ascii=False) + "\n")
                mf.flush()
                print(f"[{n}/{len(slugs)}] {slug} ({info.get('accesslevel')}): "
                      + (", ".join(f"{g.get('file', g.get('format', '?'))}({g['status']})" for g in got) or "tanpa resource"))
                time.sleep(a.delay)
        except KeyboardInterrupt:
            print(f"\nDihentikan manual. Progres aman (ok={ok} skip={skip} gagal={fail}). "
                  f"Jalankan lagi untuk lanjut dari manifest.", file=sys.stderr)
            print(f"Progres: file-ok={ok} skip={skip} gagal={fail} manifest={manifest}")
            return 130

    print(f"Selesai. file-ok={ok} skip={skip} gagal={fail} manifest={manifest}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
