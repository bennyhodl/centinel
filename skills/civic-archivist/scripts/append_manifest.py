#!/usr/bin/env python3
"""Atomic append to vault manifest.jsonl with flock. Stdlib only.

Usage:
    cat entry.json | append_manifest.py <manifest_path>

Reads ONE JSON object from stdin (may be pretty-printed across lines), validates
it has the required keys for its `op`, and atomically appends a single-line
canonical-JSON serialization + newline to the manifest file. Uses fcntl.flock
on the manifest itself for cross-process exclusion, and fsync after write.

Exits 0 on success. On any error, exits 1 with a JSON error object on stderr
and does NOT modify the manifest.

Idempotency: this script does NOT dedupe. Callers must run check_dupe.py first.
"""

import fcntl
import json
import os
import sys
from pathlib import Path

REQUIRED_VAULT_KEYS = {"op", "vault_path", "sha256", "size_bytes", "mime_type",
                       "fetched_at", "source_url", "extractor"}
REQUIRED_SEEN_KEYS = {"op", "target_sha256", "at", "url"}


def fail(msg: str) -> int:
    print(json.dumps({"error": msg}), file=sys.stderr)
    return 1


def main() -> int:
    if len(sys.argv) != 2:
        return fail("usage: append_manifest.py <manifest_path>")
    manifest = Path(sys.argv[1])
    manifest.parent.mkdir(parents=True, exist_ok=True)

    raw = sys.stdin.read()
    if not raw.strip():
        return fail("empty stdin")

    try:
        entry = json.loads(raw)
    except json.JSONDecodeError as e:
        return fail(f"invalid JSON on stdin: {e}")

    if not isinstance(entry, dict):
        return fail("entry must be a JSON object")

    op = entry.get("op")
    if op == "vault":
        missing = REQUIRED_VAULT_KEYS - entry.keys()
        if missing:
            return fail(f"vault entry missing keys: {sorted(missing)}")
    elif op == "seen_at_append":
        missing = REQUIRED_SEEN_KEYS - entry.keys()
        if missing:
            return fail(f"seen_at_append entry missing keys: {sorted(missing)}")
    else:
        return fail(f"unknown op: {op!r} (expected 'vault' or 'seen_at_append')")

    # Canonicalize: sorted keys, no extra whitespace, single line.
    line = json.dumps(entry, sort_keys=True, separators=(",", ":"), ensure_ascii=False) + "\n"

    # Open with O_APPEND so the kernel atomically appends each write; flock for
    # cross-process serialization (so two Archivist instances can't interleave).
    fd = os.open(manifest, os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o644)
    try:
        fcntl.flock(fd, fcntl.LOCK_EX)
        try:
            n = os.write(fd, line.encode("utf-8"))
            if n != len(line.encode("utf-8")):
                return fail(f"short write: {n} of {len(line)} bytes")
            os.fsync(fd)
        finally:
            fcntl.flock(fd, fcntl.LOCK_UN)
    finally:
        os.close(fd)

    print(json.dumps({"ok": True, "bytes_appended": len(line)}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
