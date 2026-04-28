#!/usr/bin/env python3
"""
parse_investigation_yaml.py — Parse and validate an investigation file's frontmatter.

Reads <path-to-investigation.md>, extracts the YAML frontmatter, validates the
required fields per the civic-investigator skill, and emits the parsed
frontmatter as JSON to stdout.

Exit codes:
  0  — frontmatter valid; JSON emitted on stdout
  1  — file not found / unreadable
  2  — no frontmatter found
  3  — YAML parse error
  4  — required field missing or invalid

Dependencies:
  - PyYAML (pip install pyyaml). Standard in most agent environments.

Usage:
  python parse_investigation_yaml.py <path>
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

try:
    import yaml  # type: ignore
except ImportError:
    sys.stderr.write(
        "ERROR: PyYAML not installed. Install with: pip install pyyaml\n"
    )
    sys.exit(1)


REQUIRED_FIELDS = ("title", "goal", "seeds", "status", "depth", "schedule")
VALID_STATUS = {"active", "paused", "done", "archived"}
VALID_SCHEDULE = {"daily", "weekly", "monthly", "manual"}
MAX_DEPTH = 5


def split_frontmatter(text: str) -> str | None:
    """Return the YAML frontmatter block (between leading '---' fences) or None."""
    if not text.startswith("---"):
        return None
    # Find the closing '---' on its own line.
    lines = text.splitlines()
    if not lines or lines[0].strip() != "---":
        return None
    for i in range(1, len(lines)):
        if lines[i].strip() == "---":
            return "\n".join(lines[1:i])
    return None


def fail(code: int, msg: str) -> None:
    sys.stderr.write(f"ERROR: {msg}\n")
    sys.exit(code)


def main(argv: list[str]) -> None:
    if len(argv) != 2:
        fail(1, f"usage: {argv[0]} <path-to-investigation.md>")

    path = Path(argv[1]).expanduser()
    if not path.is_file():
        fail(1, f"file not found: {path}")

    try:
        text = path.read_text(encoding="utf-8")
    except OSError as e:
        fail(1, f"cannot read {path}: {e}")

    fm_text = split_frontmatter(text)
    if fm_text is None:
        fail(2, "no YAML frontmatter found (expected leading '---' fences)")

    try:
        data = yaml.safe_load(fm_text)
    except yaml.YAMLError as e:
        fail(3, f"YAML parse error: {e}")

    if not isinstance(data, dict):
        fail(3, "frontmatter is not a YAML mapping")

    # Required field validation.
    missing = [f for f in REQUIRED_FIELDS if f not in data or data[f] in (None, "")]
    if missing:
        fail(4, f"missing required field(s): {', '.join(missing)}")

    if data["status"] not in VALID_STATUS:
        fail(4, f"invalid status {data['status']!r}; must be one of {sorted(VALID_STATUS)}")

    if data["schedule"] not in VALID_SCHEDULE:
        fail(4, f"invalid schedule {data['schedule']!r}; must be one of {sorted(VALID_SCHEDULE)}")

    seeds = data["seeds"]
    if not isinstance(seeds, list) or not seeds:
        fail(4, "seeds must be a non-empty list of URLs")

    depth = data["depth"]
    if not isinstance(depth, int) or depth < 1 or depth > MAX_DEPTH:
        fail(4, f"depth must be an int 1..{MAX_DEPTH}; got {depth!r}")

    # Optional fields with defaults.
    data.setdefault("focus_entities", [])
    data.setdefault("exclude_urls", [])
    data.setdefault("auto_complete", False)
    data.setdefault("confidential", False)
    data.setdefault("date_range", {"from": None, "to": None})

    json.dump(data, sys.stdout, indent=2, default=str, sort_keys=True)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main(sys.argv)
