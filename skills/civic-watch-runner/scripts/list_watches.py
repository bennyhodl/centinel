#!/usr/bin/env python3
"""list_watches.py — enumerate and validate watch YAMLs in <wiki>/Watches/.

Prints a JSON array of {id, title, type, paused, severity, path, errors} to stdout.
Usage:  python3 list_watches.py <watches_dir>

Requires PyYAML (the wider Centinel toolchain depends on it; this script
avoids stdlib YAML parsing because the watch files are user-authored and may
include block scalars / multi-line strings).
"""
from __future__ import annotations

import json
import os
import sys
from pathlib import Path

try:
    import yaml  # type: ignore
except ImportError:  # pragma: no cover
    print(
        "list_watches.py requires PyYAML. Install with: pip install pyyaml",
        file=sys.stderr,
    )
    sys.exit(2)


REQUIRED_FIELDS = ["id", "title", "type", "match", "rule"]
ALLOWED_TYPES = {"data", "narrative"}
ALLOWED_SEVERITY = {"low", "medium", "high"}


def validate(doc: dict, path: Path) -> list[str]:
    errors: list[str] = []
    if not isinstance(doc, dict):
        return [f"top-level YAML is not a mapping (got {type(doc).__name__})"]

    for field in REQUIRED_FIELDS:
        if field not in doc:
            errors.append(f"missing required field: {field}")

    t = doc.get("type")
    if t is not None and t not in ALLOWED_TYPES:
        errors.append(f"type must be one of {sorted(ALLOWED_TYPES)} (got {t!r})")

    sev = doc.get("severity")
    if sev is not None and sev not in ALLOWED_SEVERITY:
        errors.append(f"severity must be one of {sorted(ALLOWED_SEVERITY)} (got {sev!r})")

    # Filename stem must equal id.
    if "id" in doc and path.stem != doc["id"]:
        errors.append(f"filename stem {path.stem!r} != id {doc['id']!r}")

    # Narrative watches with auto_publish:true is a misconfig (runner enforces draft anyway).
    if doc.get("type") == "narrative" and doc.get("auto_publish") is True:
        errors.append(
            "narrative watch has auto_publish:true — runner will override to draft; fix YAML"
        )

    # match must be a mapping
    if "match" in doc and not isinstance(doc["match"], dict):
        errors.append("match must be a mapping")

    return errors


def list_watches(watches_dir: Path) -> list[dict]:
    out: list[dict] = []
    if not watches_dir.exists():
        return out

    for path in sorted(watches_dir.glob("*.yaml")):
        # Skip presets directory — it's read-only reference, not active watches.
        if "_presets" in path.parts:
            continue
        try:
            doc = yaml.safe_load(path.read_text())
        except yaml.YAMLError as exc:
            out.append(
                {
                    "id": path.stem,
                    "path": str(path),
                    "errors": [f"YAML parse error: {exc}"],
                    "valid": False,
                }
            )
            continue

        errors = validate(doc or {}, path)
        out.append(
            {
                "id": (doc or {}).get("id", path.stem),
                "title": (doc or {}).get("title"),
                "type": (doc or {}).get("type"),
                "severity": (doc or {}).get("severity"),
                "paused": bool((doc or {}).get("paused", False)),
                "auto_publish": bool((doc or {}).get("auto_publish", False)),
                "max_hits_per_run": (doc or {}).get("max_hits_per_run", 25),
                "path": str(path),
                "errors": errors,
                "valid": not errors,
            }
        )

    # Also check _presets/ exists and warn (informational, not fatal).
    return out


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: list_watches.py <watches_dir>", file=sys.stderr)
        return 2

    watches_dir = Path(sys.argv[1]).expanduser()
    watches = list_watches(watches_dir)
    json.dump(watches, sys.stdout, indent=2, default=str)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
