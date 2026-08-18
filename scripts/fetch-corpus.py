# /// script
# requires-python = ">=3.11"
# dependencies = [
#     "httpx>=0.27",
#     "pypdfium2>=4.30",
# ]
# ///
"""scripts/fetch-corpus.py — eval/sources.toml の type=web エントリを取得し、
本文を eval/corpus/external/{id}.md に保存する。

coji/natural-japanese (MIT) corpus/fetch.py
@0f1cc1c5a4e2aa7590598c88a15c213a60d9545a を適応した。変更点は
入力のTOML化、取得結果のSHA-256を eval/corpus/external-lock.json へ
記録すること、出力先のみ。THIRD_PARTY_NOTICES.md を参照。

設計方針:
    - 著作権のある記事本文はコミットしない(.gitignore 済み)。ローカルの
      評価用コーパスとしてのみ使う。
    - lock には本文でなくSHA-256・文字数・抽出方式だけを記録し、コミットする。
      評価出力がどの取得版に基づくかを再現可能にする。
    - note / Zenn 用の構造化抽出 + 汎用フォールバック(<article> / <main>)。
    - レートリミット: リクエスト間に1秒スリープ。User-Agent を明示する。

使い方:
    uv run scripts/fetch-corpus.py               # web エントリを全件取得
    uv run scripts/fetch-corpus.py --id <id>     # 1件だけ取得(動作確認用)
    uv run scripts/fetch-corpus.py --limit 3     # 先頭3件だけ取得(試走用)
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
import time
import tomllib
from datetime import datetime, timezone
from pathlib import Path

import httpx

USER_AGENT = (
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 "
    "(KHTML, like Gecko) Chrome/122.0 Safari/537.36 "
    "suiko-corpus-fetch/0.1 (github.com/nwiizo/suiko; research/calibration use)"
)

REPO_ROOT = Path(__file__).resolve().parent.parent
SOURCES_PATH = REPO_ROOT / "eval" / "sources.toml"
OUT_DIR = REPO_ROOT / "eval" / "corpus" / "external"
LOCK_PATH = REPO_ROOT / "eval" / "corpus" / "external-lock.json"

RATE_LIMIT_SECONDS = 1.0


def load_sources() -> list[dict]:
    data = tomllib.loads(SOURCES_PATH.read_text(encoding="utf-8"))
    return [s for s in data.get("source", []) if s.get("type") == "web"]


def unescape_entities(text: str) -> str:
    text = text.replace("&nbsp;", " ").replace("&amp;", "&")
    text = text.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", '"')
    text = re.sub(r"&#(\d+);", lambda m: chr(int(m.group(1))), text)
    text = re.sub(r"&#x([0-9a-fA-F]+);", lambda m: chr(int(m.group(1), 16)), text)
    return text


def convert_headings(html: str) -> str:
    """本文 HTML 中の <h1>〜<h6> を Markdown 見出しに変換する(strip_tags の前に呼ぶ)。"""

    def repl(m: re.Match[str]) -> str:
        level = int(m.group(1))
        inner = re.sub(r"<[^>]+>", "", m.group(2))
        inner = unescape_entities(inner)
        text = " ".join(inner.split()).strip()
        if not text:
            return ""
        return "\n\n" + ("#" * level) + " " + text + "\n\n"

    return re.sub(r"<h([1-6])[^>]*>(.*?)</h\1>", repl, html, flags=re.S | re.I)


def strip_tags(html: str) -> str:
    html = re.sub(r"<(script|style|noscript)[^>]*>.*?</\1>", "", html, flags=re.S | re.I)
    html = convert_headings(html)
    html = re.sub(r"<br\s*/?>", "\n", html, flags=re.I)
    html = re.sub(r"</p>", "\n\n", html, flags=re.I)
    html = re.sub(r"<[^>]+>", "", html)
    html = unescape_entities(html)
    lines = [ln.strip() for ln in html.splitlines()]
    lines = [ln for ln in lines if ln]
    return "\n\n".join(lines)


def extract_note(html: str) -> str | None:
    # note.com: 本文は <div class="note-common-styles__textnote-body"> 配下
    m = re.search(
        r'<div class="note-common-styles__textnote-body"[^>]*>(.*?)</div>\s*</div>\s*</div>',
        html,
        re.S,
    )
    if not m:
        m = re.search(r"<article[^>]*>(.*?)</article>", html, re.S)
    if not m:
        return None
    return strip_tags(m.group(1))


def extract_zenn(html: str) -> str | None:
    # Zenn: 本文は <div class="znc"> (Zenn Notation Compiled) 配下
    m = re.search(r'<div class="znc"[^>]*>(.*?)</div>\s*</div>\s*</div>', html, re.S)
    if not m:
        m = re.search(r"<article[^>]*>(.*?)</article>", html, re.S)
    if not m:
        return None
    return strip_tags(m.group(1))


def extract_generic(html: str) -> str:
    m = re.search(r"<article[^>]*>(.*?)</article>", html, re.S)
    if not m:
        m = re.search(r"<main[^>]*>(.*?)</main>", html, re.S)
    body = m.group(1) if m else html
    return strip_tags(body)


def extract_pdf(content: bytes) -> str:
    """PDF からページ単位でテキストを抽出し、ページ番号・繰り返しヘッダ/フッタを除去する。"""
    import pypdfium2 as pdfium

    pdf = pdfium.PdfDocument(content)
    page_texts = []
    for page in pdf:
        textpage = page.get_textpage()
        page_texts.append(textpage.get_text_range())

    from collections import Counter

    line_counts: Counter[str] = Counter()
    for pt in page_texts:
        for ln in {ln.strip() for ln in pt.splitlines() if ln.strip()}:
            if len(ln) <= 40:
                line_counts[ln] += 1
    n_pages = max(len(page_texts), 1)
    repeated = {
        ln for ln, c in line_counts.items() if c >= max(3, n_pages // 2) and n_pages > 2
    }

    out_lines: list[str] = []
    for pt in page_texts:
        for ln in pt.splitlines():
            s = ln.strip()
            if not s:
                continue
            if s in repeated:
                continue
            if re.fullmatch(r"[-‐―ー0-9０-９ページPage/\s.]{1,10}", s):
                continue
            out_lines.append(s)
    return "\n".join(out_lines)


def extract_body(url: str, html: str) -> tuple[str, str]:
    """(抽出方式, 本文テキスト) を返す。"""
    if "note.com" in url:
        text = extract_note(html)
        if text and len(text) > 200:
            return "note", text
    if "zenn.dev" in url:
        text = extract_zenn(html)
        if text and len(text) > 200:
            return "zenn", text
    return "generic", extract_generic(html)


def decode_html(resp: httpx.Response) -> str:
    """HTTP ヘッダに charset が無い場合、meta charset / XML 宣言を読んでデコードし直す。"""
    content_type_header = resp.headers.get("content-type", "")
    if "charset=" in content_type_header.lower():
        return resp.text

    raw = resp.content
    head = raw[:2048].decode("ascii", errors="ignore")
    m = re.search(r'charset=["\']?([\w-]+)', head, re.I) or re.search(
        r'encoding=["\']([\w-]+)["\']', head, re.I
    )
    if m:
        declared = m.group(1).strip().lower()
        try:
            return raw.decode(declared)
        except (LookupError, UnicodeDecodeError):
            pass
    return resp.text


def load_lock() -> dict:
    if LOCK_PATH.exists():
        return json.loads(LOCK_PATH.read_text(encoding="utf-8"))
    return {"version": 1, "entries": {}}


def save_lock(lock: dict) -> None:
    LOCK_PATH.write_text(
        json.dumps(lock, ensure_ascii=False, indent=1, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def fetch_one(source: dict, client: httpx.Client, lock: dict) -> None:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    out_path = OUT_DIR / f"{source['id']}.md"
    url = source["url"]
    print(f"fetch: {source['id']} <- {url}", file=sys.stderr)
    resp = client.get(url, headers={"User-Agent": USER_AGENT}, timeout=60, follow_redirects=True)
    resp.raise_for_status()

    content_type = resp.headers.get("content-type", "").lower()
    is_pdf = "application/pdf" in content_type or url.lower().split("?")[0].endswith(".pdf")
    if is_pdf:
        method = "pdf"
        body = extract_pdf(resp.content)
        title = source.get("title") or ""
    else:
        html = decode_html(resp)
        method, body = extract_body(url, html)
        title = source.get("title") or ""

    frontmatter = (
        "---\n"
        f"id: {source['id']}\n"
        f"source_url: {url}\n"
        f"title: {json.dumps(title, ensure_ascii=False)}\n"
        f"author: {json.dumps(source.get('author') or '', ensure_ascii=False)}\n"
        f"genre: {source.get('genre', '')}\n"
        f"extract_method: {method}\n"
        f"chars: {len(body)}\n"
        "---\n\n"
    )
    payload = frontmatter + body + "\n"
    out_path.write_text(payload, encoding="utf-8")
    lock["entries"][source["id"]] = {
        "url": url,
        "sha256": hashlib.sha256(payload.encode("utf-8")).hexdigest(),
        "chars": len(body),
        "extract_method": method,
        "fetched_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    }
    print(f"  -> {out_path.name} ({method}, {len(body)} chars)", file=sys.stderr)


def main() -> int:
    parser = argparse.ArgumentParser(description="eval/sources.toml の web エントリを取得する")
    parser.add_argument("--id", help="この id のエントリだけ取得する")
    parser.add_argument("--limit", type=int, help="先頭 N 件だけ取得する(試走用)")
    args = parser.parse_args()

    sources = load_sources()
    if args.id:
        sources = [s for s in sources if s["id"] == args.id]
        if not sources:
            print(f"id not found: {args.id}", file=sys.stderr)
            return 1
    elif args.limit:
        sources = sources[: args.limit]

    lock = load_lock()
    failed = 0
    with httpx.Client() as client:
        for i, source in enumerate(sources):
            try:
                fetch_one(source, client, lock)
            except Exception as e:  # noqa: BLE001 — 1件失敗しても続行する
                failed += 1
                lock["entries"][source["id"]] = {
                    "url": source["url"],
                    "error": str(e),
                    "fetched_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
                }
                print(f"  ERROR: {source['id']}: {e}", file=sys.stderr)
            if i < len(sources) - 1:
                time.sleep(RATE_LIMIT_SECONDS)

    save_lock(lock)
    succeeded = len(sources) - failed
    print(
        f"summary: {succeeded} succeeded, {failed} failed (total {len(sources)}); lock -> {LOCK_PATH}",
        file=sys.stderr,
    )
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
