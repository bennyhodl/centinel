#!/usr/bin/env python3
"""Compute SHA256 of a file. Stdlib only.

Usage:
    sha256_file.py <path>          # path as argument
    echo <path> | sha256_file.py   # path on stdin (one line)

Output: 64-char lowercase hex SHA256, followed by a newline. Nothing else.
Exit 0 on success, 1 on any error (with message on stderr).

The Archivist standardizes on this single tool rather than `sha256sum` so
behavior is identical across Linux/macOS/containers and so the script can
evolve (e.g., progress reporting for huge files) without changing callers.
"""

import hashlib
import sys
from pathlib import Path


def sha256_of(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def main() -> int:
    if len(sys.argv) >= 2:
        target = sys.argv[1]
    else:
        target = sys.stdin.readline().strip()
    if not target:
        print("sha256_file.py: no path provided (arg or stdin)", file=sys.stderr)
        return 1
    p = Path(target)
    if not p.is_file():
        print(f"sha256_file.py: not a file: {target}", file=sys.stderr)
        return 1
    try:
        print(sha256_of(p))
    except OSError as e:
        print(f"sha256_file.py: read error: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
