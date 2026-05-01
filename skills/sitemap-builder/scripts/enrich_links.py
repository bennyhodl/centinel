"""Enrich sitemap.json entries with outgoing `links[]`.

Fetches each URL with urllib, parses anchor tags from the HTML, classifies
each link by kind, and writes the enriched entries back to sitemap.json.

Stdlib-only — no extra deps. Skips entries already enriched unless --force.

Usage:
    python3 enrich_links.py \
        --sitemap "$CENTINEL_WIKI_PATH/Sitemap/sitemap.json" \
        [--limit 50] [--force] [--host www.tampa.gov]

Output: progress to stderr, final stats line to stdout.
"""
from __future__ import annotations

import argparse
import json
import re
import sys
import time
from html.parser import HTMLParser
from pathlib import Path
from typing import Any
from urllib.parse import urldefrag, urljoin, urlparse
from urllib.request import Request, urlopen

USER_AGENT = "Centinel-Cartographer/0.1 (+civic-investigation)"
DOC_EXT_RE = re.compile(
    r"\.(pdf|docx?|xlsx?|pptx?|csv|tsv|odt|ods|odp|rtf|zip)(\?|$)", re.I
)


class AnchorExtractor(HTMLParser):
    """Pulls (href, anchor_text) pairs from <a href> tags."""

    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.links: list[dict[str, str]] = []
        self._href: str | None = None
        self._buf: list[str] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        if tag.lower() != "a":
            return
        for name, val in attrs:
            if name.lower() == "href" and val:
                self._href = val.strip()
                self._buf = []
                return

    def handle_data(self, data: str) -> None:
        if self._href is not None:
            self._buf.append(data)

    def handle_endtag(self, tag: str) -> None:
        if tag.lower() != "a" or self._href is None:
            return
        anchor = " ".join("".join(self._buf).split()).strip()
        self.links.append({"href": self._href, "anchor": anchor[:200]})
        self._href = None
        self._buf = []


def classify(href: str, page_url: str, sitemap_urls: set[str]) -> tuple[str, str]:
    """Return (resolved_absolute_href, kind)."""
    href = href.strip()
    if not href:
        return ("", "anchor")
    if href.startswith("mailto:"):
        return (href, "mailto")
    if href.startswith("tel:"):
        return (href, "tel")
    if href.startswith("#"):
        return (href, "anchor")
    if href.lower().startswith("javascript:"):
        return ("", "anchor")

    abs_href, _ = urldefrag(urljoin(page_url, href))
    if not abs_href:
        return ("", "anchor")

    if abs_href in sitemap_urls:
        return (abs_href, "sitemap")
    if DOC_EXT_RE.search(abs_href):
        return (abs_href, "document")

    page_host = urlparse(page_url).netloc
    href_host = urlparse(abs_href).netloc
    if href_host and href_host == page_host:
        return (abs_href, "internal")
    return (abs_href, "external")


def fetch(url: str, timeout: float = 20.0) -> str:
    req = Request(
        url,
        headers={
            "User-Agent": USER_AGENT,
            "Accept": "text/html,application/xhtml+xml",
        },
    )
    with urlopen(req, timeout=timeout) as resp:
        ct = resp.headers.get("content-type", "")
        if "html" not in ct.lower() and "xml" not in ct.lower():
            return ""  # not parseable; skip silently
        raw = resp.read(4_000_000)  # cap at 4MB per page
    # decode best-effort
    charset = resp.headers.get_content_charset() or "utf-8"
    try:
        return raw.decode(charset, errors="replace")
    except LookupError:
        return raw.decode("utf-8", errors="replace")


def extract_links(html: str, page_url: str, sitemap_urls: set[str]) -> list[dict[str, Any]]:
    parser = AnchorExtractor()
    try:
        parser.feed(html)
    except Exception:  # html.parser is forgiving but we still bail safely
        return []

    seen: set[tuple[str, str]] = set()
    out: list[dict[str, Any]] = []
    for raw_link in parser.links:
        abs_href, kind = classify(raw_link["href"], page_url, sitemap_urls)
        if not abs_href:
            continue
        key = (abs_href, raw_link["anchor"])
        if key in seen:
            continue
        seen.add(key)
        out.append({"href": abs_href, "anchor": raw_link["anchor"], "kind": kind})
    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--sitemap", required=True, help="Path to sitemap.json")
    ap.add_argument("--limit", type=int, default=0, help="Max entries to process (0 = all)")
    ap.add_argument("--force", action="store_true", help="Re-enrich even if links already present")
    ap.add_argument("--host", default="", help="Only enrich entries on this host")
    ap.add_argument("--delay", type=float, default=0.5, help="Sleep between fetches (seconds)")
    args = ap.parse_args()

    sitemap_path = Path(args.sitemap).expanduser()
    if not sitemap_path.exists():
        print(f"ERROR: sitemap not found at {sitemap_path}", file=sys.stderr)
        return 2

    doc = json.loads(sitemap_path.read_text())
    if isinstance(doc, list):
        doc = {"domain": "", "generated_at": "", "entries": doc}

    entries: list[dict[str, Any]] = doc["entries"]
    sitemap_urls: set[str] = {e["url"] for e in entries}

    # Pick targets
    targets: list[int] = []
    for i, e in enumerate(entries):
        if args.host and urlparse(e["url"]).netloc != args.host:
            continue
        existing = e.get("links") or []
        if existing and not args.force:
            continue
        targets.append(i)
    if args.limit > 0:
        targets = targets[: args.limit]

    print(f"enriching {len(targets)} of {len(entries)} entries", file=sys.stderr)

    n_ok = 0
    n_fail = 0
    n_links = 0
    started = time.time()

    for idx, i in enumerate(targets, 1):
        e = entries[i]
        url = e["url"]
        try:
            html = fetch(url)
            if not html:
                e["links"] = []
                n_fail += 1
                print(f"[{idx}/{len(targets)}] skip (non-html) {url}", file=sys.stderr)
            else:
                links = extract_links(html, url, sitemap_urls)
                e["links"] = links
                n_ok += 1
                n_links += len(links)
                print(
                    f"[{idx}/{len(targets)}] {len(links):3d} links  {url}",
                    file=sys.stderr,
                )
        except Exception as exc:  # noqa: BLE001 — we want every error captured
            e["links"] = []
            n_fail += 1
            print(f"[{idx}/{len(targets)}] FAIL {url} :: {exc}", file=sys.stderr)

        # Persist incrementally every 25 entries so a crash doesn't lose work
        if idx % 25 == 0:
            sitemap_path.write_text(json.dumps(doc, indent=2))

        time.sleep(args.delay)

    sitemap_path.write_text(json.dumps(doc, indent=2))
    elapsed = time.time() - started
    summary = {
        "ok": n_ok,
        "failed": n_fail,
        "links_total": n_links,
        "elapsed_sec": round(elapsed, 1),
    }
    print(json.dumps(summary))
    return 0


if __name__ == "__main__":
    sys.exit(main())
