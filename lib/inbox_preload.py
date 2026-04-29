"""Pre-cron inbox + status injection for Centinel role profiles.

Hermes runs `--script <path>` before each cron job and injects the script's
stdout into the prompt as context. We use that to pre-load:

  1. The role's inbox files (`_runtime/inbox/<role>/*.md`) — full content,
     so the agent doesn't burn tool calls listing and reading them.
  2. The role's last-run status file (`_runtime/status/<role>.md`) — for
     continuity across ticks.

Each role gets a 3-line wrapper at `scripts/cron/preload_<role>.py` that
just imports this module and calls `preload(role)`.

Caps:
  - MAX_FILES messages shown
  - MAX_TOTAL_BYTES across all message bodies

Anything beyond the cap is listed by path so the agent knows there's more
to drain — the next tick (or a re-run) will surface them.
"""
from __future__ import annotations

import os
import sys
from pathlib import Path

MAX_FILES = 50
MAX_TOTAL_BYTES = 200_000
MAX_STATUS_BYTES = 4_000


def _wiki_path() -> Path:
    """Resolve the wiki root.

    Order:
      1. CENTINEL_WIKI_PATH env (expanded)
      2. lib.config.load().wiki_path (parses doge.config.yaml)

    The cron script runs in the user's environment so env-var path is the
    common case. Falls back to the dispatcher's config loader so behavior
    stays identical to `bin/centinel`.
    """
    raw = os.environ.get("CENTINEL_WIKI_PATH")
    if raw:
        return Path(os.path.expandvars(os.path.expanduser(raw))).resolve()

    # Lazy import to avoid pulling PyYAML unless we need it.
    repo_root = Path(__file__).resolve().parent.parent
    sys.path.insert(0, str(repo_root))
    from lib import config as cfg  # noqa: E402

    return cfg.load().wiki_path


def preload(role: str) -> None:
    """Emit pre-cron context for `role` to stdout."""
    wiki = _wiki_path()
    inbox_dir = wiki / "_runtime" / "inbox" / role
    status_file = wiki / "_runtime" / "status" / f"{role}.md"

    print(f"# Pre-cron context — {role}")
    print()
    print(
        "_The following is **already loaded** for you. Do NOT list-dir or "
        "read these files again — you already have them. Use file tools "
        "only to **write** outbox replies, **move** processed inbox messages, "
        "and **update** queue items / status._"
    )
    print()

    # ── Status snapshot ─────────────────────────────────────────────────
    if status_file.exists():
        try:
            text = status_file.read_text(errors="replace")
        except OSError:
            text = ""
        if len(text) > MAX_STATUS_BYTES:
            text = text[:MAX_STATUS_BYTES].rstrip() + "\n\n... (truncated)"
        rel = status_file.relative_to(wiki)
        print(f"## Last-run status — `{rel}`")
        print()
        print("```markdown")
        print(text.rstrip())
        print("```")
        print()

    # ── Inbox ───────────────────────────────────────────────────────────
    if not inbox_dir.exists():
        print(f"## Inbox — `_runtime/inbox/{role}/`")
        print()
        print(
            f"Directory does not exist yet. No messages possible. If your "
            f"prompt asks you to drain the inbox, there is nothing to do this tick."
        )
        return

    try:
        files = sorted(
            (p for p in inbox_dir.glob("*.md") if p.is_file()),
            key=lambda p: p.stat().st_mtime,  # oldest first
        )
    except OSError as e:
        print(f"## Inbox — error reading `{inbox_dir}`: {e}")
        return

    if not files:
        print(f"## Inbox — EMPTY")
        print()
        print(
            "No pending messages. If your prompt asks you to drain the "
            "inbox, there is nothing to do this tick — update the status "
            "file if appropriate and exit clean."
        )
        return

    print(f"## Inbox — {len(files)} pending message(s)")
    print()
    print(
        "Each file's full content is included below. Process them in order "
        "(oldest first). After processing each: write your reply to "
        f"`_runtime/outbox/{role}/<YYYY-MM>/`, then move the original from "
        f"`_runtime/inbox/{role}/` to `_runtime/outbox/<sender>/<YYYY-MM>/` "
        "with `status: done`."
    )
    print()

    total_bytes = 0
    shown = 0
    truncated: list[Path] = []

    for f in files:
        try:
            content = f.read_text(errors="replace")
        except OSError as e:
            content = f"_(error reading: {e})_"
        size = len(content.encode("utf-8"))

        if shown >= MAX_FILES or total_bytes + size > MAX_TOTAL_BYTES:
            truncated.append(f)
            continue

        rel = f.relative_to(wiki)
        print(f"### `{rel}`")
        print()
        print("```markdown")
        print(content.rstrip())
        print("```")
        print()
        total_bytes += size
        shown += 1

    if truncated:
        print(f"### Note — {len(truncated)} additional message(s) not shown")
        print()
        print(
            "Process the messages above first. The next cron tick (or a "
            "manual re-run) will surface the rest. Files pending:"
        )
        for t in truncated[:10]:
            print(f"- `{t.relative_to(wiki)}`")
        if len(truncated) > 10:
            print(f"- ... and {len(truncated) - 10} more")


def main(argv: list[str] | None = None) -> int:
    args = argv if argv is not None else sys.argv[1:]
    if len(args) != 1:
        print("usage: inbox_preload.py <role>", file=sys.stderr)
        return 2
    preload(args[0])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
