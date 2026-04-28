#!/usr/bin/env python3
"""dedup_hits.py — filter hit-hashes against the seen log.

Reads hit-hashes (one per line) from stdin. Prints novel hashes to stdout.
Appends novel hashes to <seen_file> (JSONL, one object per line).

Usage:  python3 dedup_hits.py <seen_file> [--watch <id>]

Stdlib only.
"""
from __future__ import annotations

import argparse
import datetime as dt
import json
import sys
from pathlib import Path


def load_seen(seen_file: Path) -> set[str]:
    seen: set[str] = set()
    if not seen_file.exists():
        return seen
    for line in seen_file.read_text().splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            obj = json.loads(line)
            h = obj.get("hit_hash")
            if h:
                seen.add(h)
        except json.JSONDecodeError:
            # Tolerant: ignore corrupt lines, don't crash the run.
            continue
    return seen


def main() -> int:
    p = argparse.ArgumentParser(description="Filter hit-hashes against seen log.")
    p.add_argument("seen_file", help="path to watch-runner-seen.jsonl")
    p.add_argument("--watch", default="", help="watch id (recorded with each new hash)")
    p.add_argument(
        "--lane",
        default="",
        help="lane (raw|draft|discarded) — recorded with each new hash",
    )
    p.add_argument(
        "--no-write",
        action="store_true",
        help="don't append to seen file (smoke-test mode)",
    )
    args = p.parse_args()

    seen_file = Path(args.seen_file).expanduser()
    seen_file.parent.mkdir(parents=True, exist_ok=True)

    seen = load_seen(seen_file)
    novel: list[str] = []

    for line in sys.stdin:
        h = line.strip()
        if not h:
            continue
        if h in seen:
            continue
        seen.add(h)
        novel.append(h)
        print(h)

    if novel and not args.no_write:
        ts = dt.datetime.now(dt.timezone.utc).isoformat()
        with seen_file.open("a") as fp:
            for h in novel:
                fp.write(
                    json.dumps(
                        {
                            "ts": ts,
                            "watch_id": args.watch,
                            "hit_hash": h,
                            "lane": args.lane,
                        }
                    )
                    + "\n"
                )

    return 0


if __name__ == "__main__":
    sys.exit(main())
