#!/usr/bin/env python3
"""Pre-cron preload wrapper for the data-reporter profile.

Hermes runs this via `--script` before each data-reporter cron tick and
injects stdout as context. See `lib/inbox_preload.py` for shared logic.
"""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent.parent))

from lib.inbox_preload import preload  # noqa: E402

if __name__ == "__main__":
    preload("data-reporter")
