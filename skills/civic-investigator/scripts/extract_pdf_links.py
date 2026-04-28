#!/usr/bin/env python3
"""
extract_pdf_links.py — Enumerate PDF/document links in a markdown file.

Scans the given markdown file for links that look like documents (PDF, DOCX,
XLSX, etc.) and emits one JSON object per line on stdout, suitable for piping
into Archivist-bound vault-request emission.

Output format (one JSON object per line; NDJSON):
  {"url": "...", "link_text": "...", "found_in": "<input-path>"}

Exit codes:
  0  — done (zero or more links emitted)
  1  — file not found / unreadable

Standard library only.

Usage:
  python extract_pdf_links.py <path-to-markdown-file>
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path
from urllib.parse import urlparse

# Match markdown links: [text](url) and bare <url>.
MD_LINK_RE = re.compile(r"\[([^\]]+)\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)")
BARE_URL_RE = re.compile(r"<(https?://[^>\s]+)>")
RAW_URL_RE = re.compile(r"(?<![\(\[\<])\bhttps?://\S+")

DOC_EXTS = (
    ".pdf", ".doc", ".docx", ".xls", ".xlsx", ".ppt", ".pptx",
    ".csv", ".tsv", ".rtf", ".odt", ".ods", ".odp",
)


def looks_like_doc(url: str) -> bool:
    try:
        path = urlparse(url).path.lower()
    except Exception:
        return False
    return any(path.endswith(ext) for ext in DOC_EXTS)


def emit(url: str, text: str, found_in: str) -> None:
    json.dump(
        {"url": url, "link_text": text, "found_in": found_in},
        sys.stdout,
        ensure_ascii=False,
    )
    sys.stdout.write("\n")


def main(argv: list[str]) -> None:
    if len(argv) != 2:
        sys.stderr.write(f"usage: {argv[0]} <path-to-markdown-file>\n")
        sys.exit(1)

    path = Path(argv[1]).expanduser()
    if not path.is_file():
        sys.stderr.write(f"ERROR: file not found: {path}\n")
        sys.exit(1)

    try:
        text = path.read_text(encoding="utf-8", errors="replace")
    except OSError as e:
        sys.stderr.write(f"ERROR: cannot read {path}: {e}\n")
        sys.exit(1)

    found_in = str(path)
    seen: set[str] = set()

    # 1. Markdown [text](url) links.
    for m in MD_LINK_RE.finditer(text):
        link_text, url = m.group(1).strip(), m.group(2).strip()
        if looks_like_doc(url) and url not in seen:
            seen.add(url)
            emit(url, link_text, found_in)

    # 2. Bare <url> links.
    for m in BARE_URL_RE.finditer(text):
        url = m.group(1).strip()
        if looks_like_doc(url) and url not in seen:
            seen.add(url)
            emit(url, "", found_in)

    # 3. Raw http(s) URLs not already wrapped.
    for m in RAW_URL_RE.finditer(text):
        url = m.group(0).rstrip(".,);]>")
        if looks_like_doc(url) and url not in seen:
            seen.add(url)
            emit(url, "", found_in)


if __name__ == "__main__":
    main(sys.argv)
