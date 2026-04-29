"""Tavily Crawl wrapper for Centinel's sitemap-builder skill.

Calls Tavily's /crawl endpoint and emits JSON the agent can parse line-by-line.
The agent is responsible for higher-level decisions (mode, paging across multiple
crawl calls for very large cities, classification of returned URLs).

Why this lives in the skill rather than as a Hermes built-in:
    Tavily is operator-supplied (not bundled), and the crawl call shape is
    Centinel-specific (regex filters from city-overlay/exclude-patterns.yaml,
    domain confinement to city.gov). Keeping it in the skill means the maintainer
    can tune crawl parameters without touching Hermes core.

Cost note:
    - 1 credit per 10 pages by default
    - 2 credits per 10 pages with --instructions
    - Free tier: 1000 credits/mo (enough for ~10K pages without instructions)
    - For Tampa-scale bootstrap, expect 500-1500 credits per full crawl.
    - Use --instructions sparingly (only for targeted re-crawls of small subtrees).

Usage:
    python3 crawl.py \\
        --url https://www.tampa.gov \\
        --max-depth 3 --max-breadth 50 --limit 500 \\
        --select-domains '^(www\\.)?tampa\\.gov$' \\
        --exclude-paths '/calendar/print' '/search\\\\?'

    # With instructions (costs 2x; only for focused passes):
    python3 crawl.py \\
        --url https://www.tampa.gov/finance \\
        --instructions "Find every procurement and contract page" \\
        --limit 100

Output:
    Stdout: one JSON line per result `{url, raw_content, favicon}` followed by a
    final `{"_meta": {"credits": N, "response_time": s, "request_id": "..."}}`.
    Errors go to stderr; exit code != 0 on failure.
"""
from __future__ import annotations

import argparse
import json
import os
import sys
from typing import Any

try:
    import requests
except ImportError:
    print("ERROR: `requests` not installed. pip install requests", file=sys.stderr)
    sys.exit(2)


TAVILY_ENDPOINT = "https://api.tavily.com/crawl"


def main() -> int:
    p = argparse.ArgumentParser(description="Tavily Crawl wrapper for Centinel.")
    p.add_argument("--url", required=True, help="Root URL to crawl")
    p.add_argument("--instructions", help="Natural-language guide (DOUBLES cost — use sparingly)")
    p.add_argument("--chunks-per-source", type=int, default=3,
                   help="1-5; only with --instructions")
    p.add_argument("--max-depth", type=int, default=3, help="1-5 (default 3 for Centinel)")
    p.add_argument("--max-breadth", type=int, default=50, help="1-500 (default 50)")
    p.add_argument("--limit", type=int, default=500,
                   help="Total pages cap (default 500; raise for full bootstrap)")
    p.add_argument("--select-paths", nargs="*", default=None,
                   help="Regex(es) to include URL paths")
    p.add_argument("--select-domains", nargs="*", default=None,
                   help="Regex(es) to confine to specific domains/subdomains")
    p.add_argument("--exclude-paths", nargs="*", default=None,
                   help="Regex(es) to exclude")
    p.add_argument("--exclude-domains", nargs="*", default=None)
    p.add_argument("--allow-external", action="store_true",
                   help="Allow crawling beyond the root domain (default: false)")
    p.add_argument("--include-images", action="store_true")
    p.add_argument("--extract-depth", choices=["basic", "advanced"], default="basic",
                   help="'advanced' retrieves tables/embedded content at higher latency")
    p.add_argument("--format", choices=["markdown", "text"], default="markdown")
    p.add_argument("--include-favicon", action="store_true")
    p.add_argument("--timeout", type=float, default=150.0)
    args = p.parse_args()

    api_key = os.environ.get("TAVILY_API_KEY")
    if not api_key:
        print("ERROR: TAVILY_API_KEY not set in environment.", file=sys.stderr)
        print("  Add it to ~/code/centinel/.env and re-source, or:", file=sys.stderr)
        print("  export TAVILY_API_KEY=tvly-...", file=sys.stderr)
        return 2

    body: dict[str, Any] = {
        "url": args.url,
        "max_depth": args.max_depth,
        "max_breadth": args.max_breadth,
        "limit": args.limit,
        "extract_depth": args.extract_depth,
        "format": args.format,
        "include_favicon": args.include_favicon,
        "include_images": args.include_images,
        "allow_external": args.allow_external,
        "timeout": args.timeout,
        "include_usage": True,
    }
    if args.instructions:
        body["instructions"] = args.instructions
        body["chunks_per_source"] = args.chunks_per_source
    for opt, key in [
        (args.select_paths, "select_paths"),
        (args.select_domains, "select_domains"),
        (args.exclude_paths, "exclude_paths"),
        (args.exclude_domains, "exclude_domains"),
    ]:
        if opt:
            body[key] = opt

    headers = {
        "Authorization": f"Bearer {api_key}",
        "Content-Type": "application/json",
    }

    try:
        r = requests.post(TAVILY_ENDPOINT, json=body, headers=headers, timeout=args.timeout + 30)
    except requests.RequestException as e:
        print(f"ERROR: Tavily request failed: {e}", file=sys.stderr)
        return 1

    if r.status_code == 401:
        print("ERROR: Tavily 401 unauthorized — check TAVILY_API_KEY", file=sys.stderr)
        return 1
    if r.status_code == 429:
        print("ERROR: Tavily 429 rate limit — retry later", file=sys.stderr)
        return 1
    if r.status_code >= 400:
        print(f"ERROR: Tavily {r.status_code}: {r.text[:500]}", file=sys.stderr)
        return 1

    payload = r.json()
    for entry in payload.get("results", []):
        print(json.dumps(entry, ensure_ascii=False))
    meta = {
        "_meta": {
            "base_url": payload.get("base_url"),
            "response_time": payload.get("response_time"),
            "credits": (payload.get("usage") or {}).get("credits"),
            "request_id": payload.get("request_id"),
            "result_count": len(payload.get("results", [])),
        }
    }
    print(json.dumps(meta, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main())
