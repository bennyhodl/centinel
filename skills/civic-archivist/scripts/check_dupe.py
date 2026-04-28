#!/usr/bin/env python3
"""Look up a sha256 in the vault manifest. Stdlib only.

Usage:
    check_dupe.py <sha256> <manifest_path>

Output (stdout): single JSON object
    {
      "found": bool,
      "vault_path": "<rel path>" | null,
      "sidecar_path": "<rel path>" | null,
      "first_seen_at": "<iso8601>" | null,
      "seen_at_count": <int>
    }

Exit 0 always (caller distinguishes via `found`). Any read error -> exit 1
with a JSON error object on stderr.

Note: the manifest is append-only with `op: "vault"` and `op: "seen_at_append"`
lines. We only match `vault` lines for the originating vault_path/sidecar_path.
We count `seen_at_append` lines targeting the sha to populate seen_at_count.
"""

import json
import sys
from pathlib import Path


def main() -> int:
    if len(sys.argv) != 3:
        print(json.dumps({"error": "usage: check_dupe.py <sha256> <manifest_path>"}), file=sys.stderr)
        return 1

    sha = sys.argv[1].strip().lower()
    manifest = Path(sys.argv[2])

    result = {
        "found": False,
        "vault_path": None,
        "sidecar_path": None,
        "first_seen_at": None,
        "seen_at_count": 0,
    }

    if not manifest.exists():
        # An empty/missing manifest is normal on first run.
        print(json.dumps(result))
        return 0

    try:
        with manifest.open("r", encoding="utf-8") as f:
            for line_no, raw in enumerate(f, 1):
                raw = raw.strip()
                if not raw:
                    continue
                try:
                    e = json.loads(raw)
                except json.JSONDecodeError:
                    # Skip malformed lines but don't fail — operator can audit.
                    print(f"check_dupe.py: skipping malformed line {line_no}", file=sys.stderr)
                    continue
                op = e.get("op")
                if op == "vault" and e.get("sha256") == sha:
                    result["found"] = True
                    result["vault_path"] = e.get("vault_path")
                    result["sidecar_path"] = e.get("sidecar_path")
                    result["first_seen_at"] = e.get("fetched_at")
                    # initial seen_at array length
                    result["seen_at_count"] = len(e.get("seen_at", []))
                elif op == "seen_at_append" and e.get("target_sha256") == sha:
                    result["seen_at_count"] += 1
    except OSError as ex:
        print(json.dumps({"error": f"read error: {ex}"}), file=sys.stderr)
        return 1

    print(json.dumps(result))
    return 0


if __name__ == "__main__":
    sys.exit(main())
