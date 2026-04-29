"""Centinel dispatcher — `bin/centinel` invokes this.

Subcommands:
    bootstrap-sitemap <domain>      Build the initial sitemap (wizard Step 5).
    cron resume-all                 Activate paused cron jobs (wizard Step 7).
    cron pause-all                  Emergency stop — pause every Centinel cron job.
    cron list                       Show all Centinel-owned cron jobs.
    investigate register <slug>     Register per-investigation cron entry.
    setup-profiles                  Idempotently create role profiles (called by ./bootstrap).
    setup-cron                      Idempotently register paused recurring jobs (called by ./bootstrap).
    doctor                          Health check.

The dispatcher is the ONLY thing the Next.js web app shells out to. It in turn
shells out to `hermes` with the right `--profile` / `--skill` / etc.

See docs/AGENT_INVOCATION.md for the lane model.
"""
from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Iterable

from . import config as cfg


# ─────────────────────────────────────────────────────────────────────────────
# Helpers
# ─────────────────────────────────────────────────────────────────────────────

CRON_NAME_PREFIX = "centinel"  # all jobs we create are namespaced

# Per-role pre-cron preload scripts. Hermes runs these before the cron tick
# and injects their stdout into the prompt as context — so the agent sees
# its inbox + last-run status immediately, no list-dir round-trip needed.
# A role with no entry here gets no preload script (defaults to bare prompt).
REPO_ROOT = Path(__file__).resolve().parent.parent
PRELOAD_SCRIPTS: dict[str, Path] = {
    "investigator":  REPO_ROOT / "scripts" / "cron" / "preload_investigator.py",
    "watch-runner":  REPO_ROOT / "scripts" / "cron" / "preload_watch_runner.py",
    "data-reporter": REPO_ROOT / "scripts" / "cron" / "preload_data_reporter.py",
    "archivist":     REPO_ROOT / "scripts" / "cron" / "preload_archivist.py",
}


def _preload_script_for(profile: str | None) -> str | None:
    """Return absolute path to the preload script for `profile`, or None.

    Returns None for the default profile (no role-specific inbox) and for
    any role we haven't built a script for yet.
    """
    if not profile:
        return None
    p = PRELOAD_SCRIPTS.get(profile)
    if not p:
        return None
    if not p.exists():
        return None
    return str(p)


def _err(msg: str) -> None:
    print(f"❌ {msg}", file=sys.stderr)


def _ok(msg: str) -> None:
    print(f"✅ {msg}")


def _info(msg: str) -> None:
    print(f"→ {msg}")


def _run(cmd: list[str], *, check: bool = True, capture: bool = False) -> subprocess.CompletedProcess:
    """Run a subprocess, echoing the command for transparency."""
    pretty = " ".join(_shell_quote(c) for c in cmd)
    _info(pretty)
    return subprocess.run(
        cmd,
        check=check,
        text=True,
        capture_output=capture,
    )


def _shell_quote(s: str) -> str:
    if not s or any(c in s for c in " '\"\t\n*?{}$()&|;<>"):
        return "'" + s.replace("'", "'\\''") + "'"
    return s


def _hermes_bin() -> str:
    bin_path = shutil.which("hermes")
    if not bin_path:
        _err("`hermes` not on PATH. Install Hermes Agent first.")
        raise SystemExit(2)
    return bin_path


def _job_name(suffix: str, *, slug: str | None = None) -> str:
    base = f"{CRON_NAME_PREFIX}-{suffix}"
    return f"{base}-{slug}" if slug else base


def _cron_jobs() -> list[dict]:
    """Return Centinel-owned cron jobs as dicts via `hermes cron list`."""
    # hermes cron list doesn't expose --json today; parse the text table.
    # Fall back gracefully if the format ever changes.
    try:
        out = subprocess.run(
            [_hermes_bin(), "cron", "list", "--all"],
            check=True,
            text=True,
            capture_output=True,
        ).stdout
    except subprocess.CalledProcessError as e:
        _err(f"hermes cron list failed: {e.stderr or e}")
        return []

    jobs: list[dict] = []
    for line in out.splitlines():
        line = line.strip()
        if not line or line.startswith("#") or "═" in line or "─" in line:
            continue
        # Heuristic: any line containing a Centinel-prefixed name.
        if CRON_NAME_PREFIX + "-" in line:
            jobs.append({"raw": line})
    return jobs


# ─────────────────────────────────────────────────────────────────────────────
# Subcommands
# ─────────────────────────────────────────────────────────────────────────────


def cmd_bootstrap_sitemap(args: argparse.Namespace) -> int:
    """Wizard Step 5. Run the sitemap-builder skill in bootstrap mode.

    Sync, log-streaming. The web app tails the log file via SSE.
    """
    config = cfg.load()
    domain = args.domain or config.city.domain
    wiki = config.wiki_path

    log_path = config.runtime_dir / "logs" / "bootstrap-sitemap.log"
    log_path.parent.mkdir(parents=True, exist_ok=True)

    prompt = (
        f"Bootstrap mode: build the full sitemap for {domain}. "
        f"Write outputs to {wiki}/Sitemap/ (index.md + sitemap.json). "
        f"Resolve the wiki via $CENTINEL_WIKI_PATH={wiki}."
    )
    cmd = [
        _hermes_bin(),
        "chat",
        "--quiet",
        "--skills", "sitemap-builder",
        "--query", prompt,
    ]
    env = os.environ.copy()
    env["CENTINEL_WIKI_PATH"] = str(wiki)

    _info(f"Logging to {log_path}")
    with log_path.open("w") as logf:
        proc = subprocess.run(cmd, env=env, stdout=logf, stderr=subprocess.STDOUT)
    if proc.returncode != 0:
        _err(f"sitemap-builder exited with {proc.returncode}; see {log_path}")
        return proc.returncode
    _ok(f"Sitemap bootstrap complete for {domain}")
    return 0


def cmd_setup_profiles(args: argparse.Namespace) -> int:
    """Idempotently create one Hermes profile per non-Editor role."""
    hermes = _hermes_bin()
    existing = subprocess.run(
        [hermes, "profile", "list"], check=False, text=True, capture_output=True
    ).stdout

    for role in cfg.PROFILE_ROLES:
        if role in existing.split():
            _info(f"profile {role} already exists, skipping")
            continue
        flags = ["--clone"]
        if args.no_alias:
            flags.append("--no-alias")
        _run([hermes, "profile", "create", *flags, role], check=False)
    _ok("Profiles ready")
    return 0


def cmd_setup_cron(args: argparse.Namespace) -> int:
    """Idempotently register paused recurring jobs.

    Per-role jobs live in that role's profile (registered with `hermes --profile <role> cron create`).
    Editor-side jobs live in the default profile.
    """
    config = cfg.load()
    hermes = _hermes_bin()

    def register(name: str, schedule: str, profile: str | None, skill: str, prompt: str) -> None:
        # Skip if a job with this name already exists.
        existing = subprocess.run(
            [hermes, *(["--profile", profile] if profile else []), "cron", "list", "--all"],
            check=False, text=True, capture_output=True,
        ).stdout
        if name in existing:
            _info(f"cron {name} already registered, skipping")
            return
        cmd = [hermes]
        if profile:
            cmd += ["--profile", profile]
        cmd += [
            "cron", "create",
            "--name", name,
            "--skill", skill,
            "--deliver", "local",
        ]
        preload = _preload_script_for(profile)
        if preload:
            cmd += ["--script", preload]
        cmd += [schedule, prompt]
        _run(cmd, check=False)
        # New jobs default to active; pause them so the wizard's Step 7 can activate.
        # We resolve the id by re-listing — simplest reliable approach.
        listing = subprocess.run(
            [hermes, *(["--profile", profile] if profile else []), "cron", "list"],
            check=False, text=True, capture_output=True,
        ).stdout
        for line in listing.splitlines():
            if name in line:
                # Job IDs in `hermes cron list` are the first whitespace-delimited token.
                job_id = line.strip().split()[0]
                _run(
                    [hermes, *(["--profile", profile] if profile else []), "cron", "pause", job_id],
                    check=False,
                )
                break

    s = config.cron
    register(_job_name("sitemap-lint"),    s.sitemap_lint,   None,            "sitemap-builder",     f"Lint sitemap at {config.wiki_path}/Sitemap/.")
    register(_job_name("briefings"),       s.briefings,      None,            "humanized-writing",   f"Draft weekly briefing from {config.wiki_path}/Findings + outbox.")
    register(_job_name("huddle-rollup"),   s.huddle_rollup,  None,            "civic-doge-editor",   f"Roll up daily huddle into {config.wiki_path}/_runtime/operator-queue/.")
    register(_job_name("watch-runner"),    s.watch_runner,   "watch-runner",  "civic-watch-runner",  "Run watches over latest sitemap diffs and new wiki pages.")
    register(_job_name("data-reporter"),   s.data_reporter,  "data-reporter", "civic-data-reporter", "Refresh entity DB and methodology log.")
    register(_job_name("vault-manifest"),  s.vault_manifest, "archivist",     "civic-archivist",     "Drain archivist inbox; rebuild vault manifest if stale.")
    register(_job_name("investigator-tick"), s.investigator_tick, "investigator", "civic-investigator", "Drain investigator inbox; run pending tasks.")
    # Snooze sweep is a dispatcher subcommand, not a Hermes session — runs as
    # plain shell out via cron. Daily at 06:00 local.
    _register_dispatcher_cron(name=_job_name("snooze-sweep"), schedule="0 6 * * *",
                              dispatcher_args=["queue", "sweep-snoozed"])

    _ok("Cron jobs registered (paused). Run `centinel cron resume-all` to activate.")
    return 0


def _register_dispatcher_cron(*, name: str, schedule: str, dispatcher_args: list[str]) -> None:
    """Register a cron job that shells out to `bin/centinel <args>`.

    Used for maintenance tasks that don't need an LLM session — Hermes still
    owns the schedule, but the prompt is just a shell directive the agent
    follows literally (`run the listed shell command and exit`).

    We use Hermes cron rather than system cron so it shows up in `cron list`,
    can be paused/resumed via the same controls, and the audit trail lands
    in the same place.
    """
    hermes = _hermes_bin()
    bin_centinel = REPO_ROOT / "bin" / "centinel"
    args_str = " ".join(dispatcher_args)
    existing = subprocess.run(
        [hermes, "cron", "list", "--all"],
        check=False, text=True, capture_output=True,
    ).stdout
    if name in existing:
        _info(f"cron {name} already registered, skipping")
        return
    prompt = (
        f"Run this exact shell command and exit, reporting only its stdout/stderr:\n"
        f"\n"
        f"```bash\n{bin_centinel} {args_str}\n```\n"
    )
    cmd = [
        hermes,
        "cron", "create",
        "--name", name,
        "--deliver", "local",
        schedule,
        prompt,
    ]
    _run(cmd, check=False)
    job_id = _find_cron_job_id(None, name)
    if job_id:
        _run([hermes, "cron", "pause", job_id], check=False)


def cmd_cron_resume_all(args: argparse.Namespace) -> int:
    """Wizard Step 7. Resume every paused Centinel-owned cron job."""
    return _bulk_cron_state(action="resume")


def cmd_cron_pause_all(args: argparse.Namespace) -> int:
    """Emergency stop — pause every Centinel-owned cron job."""
    return _bulk_cron_state(action="pause")


def _bulk_cron_state(*, action: str) -> int:
    hermes = _hermes_bin()
    profiles_to_check: list[str | None] = [None] + cfg.PROFILE_ROLES
    touched = 0
    for profile in profiles_to_check:
        cmd = [hermes]
        if profile:
            cmd += ["--profile", profile]
        cmd += ["cron", "list", "--all"]
        listing = subprocess.run(cmd, check=False, text=True, capture_output=True).stdout
        for line in listing.splitlines():
            if CRON_NAME_PREFIX + "-" not in line:
                continue
            tokens = line.strip().split()
            if not tokens:
                continue
            job_id = tokens[0]
            cmd2 = [hermes]
            if profile:
                cmd2 += ["--profile", profile]
            cmd2 += ["cron", action, job_id]
            _run(cmd2, check=False)
            touched += 1
    _ok(f"{action}d {touched} Centinel cron jobs")
    return 0


def cmd_cron_list(args: argparse.Namespace) -> int:
    """List all Centinel-owned cron jobs across profiles."""
    hermes = _hermes_bin()
    for profile in [None] + cfg.PROFILE_ROLES:
        label = profile or "default"
        print(f"\n# profile: {label}")
        cmd = [hermes]
        if profile:
            cmd += ["--profile", profile]
        cmd += ["cron", "list", "--all"]
        out = subprocess.run(cmd, check=False, text=True, capture_output=True).stdout
        # Filter to centinel-owned rows only.
        for line in out.splitlines():
            if CRON_NAME_PREFIX + "-" in line or line.startswith(("ID", "═", "─")):
                print(line)
    return 0


# Map the skill's friendly schedule words to actual cron expressions.
# `manual` means no cron at all — operator triggers via inbox only.
SCHEDULE_WORD_TO_CRON: dict[str, str] = {
    "daily":   "0 2 * * *",    # 02:00 every day
    "weekly":  "0 2 * * 1",    # Monday 02:00
    "monthly": "0 2 1 * *",    # 1st of month 02:00
}


def _resolve_schedule(raw: str | None) -> str | None:
    """Translate `daily|weekly|monthly|manual` to a cron expression.

    Returns:
        cron expression string, or None if `manual` (no cron should be registered),
        or the raw value if it looks like a cron expression already (contains spaces).
    """
    if not raw:
        return SCHEDULE_WORD_TO_CRON["daily"]
    word = raw.strip().lower()
    if word == "manual":
        return None
    if word in SCHEDULE_WORD_TO_CRON:
        return SCHEDULE_WORD_TO_CRON[word]
    # Assume it's already a cron expression (e.g. "0 4 * * *").
    if " " in raw:
        return raw
    # Unknown word — fall back to daily and warn.
    _info(f"unknown schedule word '{raw}', defaulting to daily (0 2 * * *)")
    return SCHEDULE_WORD_TO_CRON["daily"]


def cmd_investigate_register(args: argparse.Namespace) -> int:
    """Register a per-investigation cron job in the investigator profile.

    Reads `<wiki>/Investigations/<slug>.md` frontmatter for `schedule:` field.
    Schedule words (`daily | weekly | monthly | manual`) translate to cron
    expressions; `manual` skips registration entirely.
    """
    config = cfg.load()
    hermes = _hermes_bin()
    slug = args.slug

    inv_path = config.wiki_path / "Investigations" / f"{slug}.md"
    if not inv_path.exists():
        _err(f"Investigation file not found: {inv_path}")
        return 1

    raw_schedule = _parse_frontmatter_field(inv_path, "schedule")
    schedule = _resolve_schedule(raw_schedule)
    if schedule is None:
        _ok(f"Schedule is 'manual' for {slug} — no cron registered (operator triggers only)")
        return 0

    name = _job_name("investigation", slug=slug)
    prompt = (
        f"Run investigation {slug}. Read {inv_path}, resume from its last "
        f"`## Run log` entry, fan out from seeds, append findings, and update "
        f"the run log with timestamp + summary."
    )
    cmd = [
        hermes, "--profile", "investigator",
        "cron", "create",
        "--name", name,
        "--skill", "civic-investigator",
        "--deliver", "local",
    ]
    preload = _preload_script_for("investigator")
    if preload:
        cmd += ["--script", preload]
    cmd += [schedule, prompt]
    _run(cmd, check=False)
    _ok(f"Registered cron {name} ({schedule}) for {slug}")
    return 0


# ─────────────────────────────────────────────────────────────────────────────
# Per-investigation lifecycle: pause / resume / trigger
# ─────────────────────────────────────────────────────────────────────────────


def _find_cron_job_id(profile: str | None, name: str) -> str | None:
    """Return the cron job id (first whitespace-delimited token) matching `name`.

    Searches `hermes --profile <p> cron list --all`.
    """
    hermes = _hermes_bin()
    cmd = [hermes]
    if profile:
        cmd += ["--profile", profile]
    cmd += ["cron", "list", "--all"]
    listing = subprocess.run(cmd, check=False, text=True, capture_output=True).stdout
    for line in listing.splitlines():
        if name in line:
            tokens = line.strip().split()
            if tokens:
                return tokens[0]
    return None


def _patch_frontmatter_field(path: Path, field: str, value: str) -> bool:
    """Atomically set a single top-level frontmatter field on `path`.

    Returns True if the field was updated or added, False if the file has
    no frontmatter to patch. Body is preserved verbatim.
    """
    try:
        text = path.read_text()
    except OSError as e:
        _err(f"cannot read {path}: {e}")
        return False
    if not text.startswith("---"):
        return False
    end = text.find("\n---", 3)
    if end < 0:
        return False
    block = text[3:end]
    body = text[end + 4 :]  # skip the closing fence + newline
    lines = block.splitlines()
    new_line = f"{field}: {value}"
    found = False
    for i, line in enumerate(lines):
        # Match top-level keys only (no leading whitespace).
        if line and not line[0].isspace() and ":" in line:
            key, _, _ = line.partition(":")
            if key.strip() == field:
                lines[i] = new_line
                found = True
                break
    if not found:
        # Append before closing fence.
        lines.append(new_line)
    new_text = "---\n" + "\n".join(lines).rstrip("\n") + "\n---" + body
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(new_text)
    tmp.replace(path)
    return True


def cmd_investigate_pause(args: argparse.Namespace) -> int:
    """Pause an investigation: flip its file frontmatter + pause its cron job."""
    config = cfg.load()
    hermes = _hermes_bin()
    slug = args.slug
    inv_path = config.wiki_path / "Investigations" / f"{slug}.md"
    if not inv_path.exists():
        _err(f"Investigation file not found: {inv_path}")
        return 1

    if not _patch_frontmatter_field(inv_path, "status", "paused"):
        _err(f"Could not patch frontmatter on {inv_path}")
        return 1
    _ok(f"Set status: paused on {inv_path}")

    name = _job_name("investigation", slug=slug)
    job_id = _find_cron_job_id("investigator", name)
    if not job_id:
        _info(f"No cron job named {name} (manual schedule, or never registered)")
        return 0
    _run([hermes, "--profile", "investigator", "cron", "pause", job_id], check=False)
    _ok(f"Paused cron {job_id} ({name})")
    return 0


def cmd_investigate_resume(args: argparse.Namespace) -> int:
    """Resume an investigation: flip its file frontmatter + resume its cron job."""
    config = cfg.load()
    hermes = _hermes_bin()
    slug = args.slug
    inv_path = config.wiki_path / "Investigations" / f"{slug}.md"
    if not inv_path.exists():
        _err(f"Investigation file not found: {inv_path}")
        return 1

    if not _patch_frontmatter_field(inv_path, "status", "active"):
        _err(f"Could not patch frontmatter on {inv_path}")
        return 1
    _ok(f"Set status: active on {inv_path}")

    name = _job_name("investigation", slug=slug)
    job_id = _find_cron_job_id("investigator", name)
    if not job_id:
        # Schedule may be 'manual' — try to register, which is idempotent.
        _info(f"No cron job for {slug}, attempting to register from frontmatter schedule")
        return cmd_investigate_register(argparse.Namespace(slug=slug))
    _run([hermes, "--profile", "investigator", "cron", "resume", job_id], check=False)
    _ok(f"Resumed cron {job_id} ({name})")
    return 0


def cmd_investigate_trigger(args: argparse.Namespace) -> int:
    """Trigger an investigation now by dropping a request into the investigator inbox.

    The investigator's preloaded inbox will surface this on its next tick, which
    is bounded by the `investigator-tick` cron (defaults to every 4 hours). For
    foreground 'run right now' use `centinel-investigator -q "<prompt>"` from
    the terminal — this dispatcher command does NOT spawn a session.
    """
    config = cfg.load()
    slug = args.slug
    inv_path = config.wiki_path / "Investigations" / f"{slug}.md"
    if not inv_path.exists():
        _err(f"Investigation file not found: {inv_path}")
        return 1

    return _drop_inbox_request(
        role="investigator",
        sender="operator",
        request_body=(
            f"# Operator trigger: re-run investigation `{slug}`\n\n"
            f"Operator requested an out-of-schedule run. Read "
            f"`Investigations/{slug}.md`, resume from its last `## Run log` "
            f"entry, fan out from seeds, append findings.\n\n"
            f"## Reply\nUpdate the investigation's run log and write a "
            f"completion notice to `_runtime/outbox/investigator/<YYYY-MM>/`."
        ),
        slug_hint=f"investigation-{slug}",
    )


def cmd_watch_trigger(args: argparse.Namespace) -> int:
    """Trigger a watch (or all watches) now by dropping a request into the watch-runner inbox."""
    watch_id = args.watch_id
    body_target = (
        f"watch `{watch_id}`" if watch_id else "all watches"
    )
    return _drop_inbox_request(
        role="watch-runner",
        sender="operator",
        request_body=(
            f"# Operator trigger: run {body_target}\n\n"
            f"Operator requested an out-of-schedule watch run. "
            f"{'Run only the named watch.' if watch_id else 'Run all configured watches.'}\n\n"
            f"## Reply\nWrite the run summary to "
            f"`_runtime/outbox/watch-runner/<YYYY-MM>/`."
        ),
        slug_hint=f"watch-{watch_id or 'all'}",
        extra_fm={"watch_id": watch_id} if watch_id else None,
    )


def _drop_inbox_request(
    *,
    role: str,
    sender: str,
    request_body: str,
    slug_hint: str,
    extra_fm: dict | None = None,
) -> int:
    """Drop a `type: request` message into `_runtime/inbox/<role>/<ts>-<sender>-<slug>.md`."""
    import hashlib
    from datetime import datetime

    config = cfg.load()
    inbox_dir = config.inbox_dir / role
    inbox_dir.mkdir(parents=True, exist_ok=True)

    now = datetime.now()
    ts = now.strftime("%Y-%m-%d-%H%M")
    short = hashlib.sha256(f"{role}|{sender}|{slug_hint}|{ts}".encode()).hexdigest()[:8]
    filename = f"{ts}-{sender}-{slug_hint}-{short}.md"
    out_path = inbox_dir / filename

    fm_lines = [
        f"id: {ts}-{short}",
        f"from: {sender}",
        f"to: {role}",
        "type: request",
        "priority: normal",
        f"created: {now.isoformat()}",
    ]
    if extra_fm:
        for k, v in extra_fm.items():
            if v is not None:
                fm_lines.append(f"{k}: {v}")

    text = "---\n" + "\n".join(fm_lines) + "\n---\n\n" + request_body.rstrip() + "\n"
    tmp = out_path.with_suffix(out_path.suffix + ".tmp")
    tmp.write_text(text)
    tmp.replace(out_path)
    _ok(f"Dropped trigger at {out_path.relative_to(config.wiki_path)}")
    _info(f"{role} will pick this up on its next cron tick.")
    return 0


# ─────────────────────────────────────────────────────────────────────────────
# Operator-queue snooze sweep
# ─────────────────────────────────────────────────────────────────────────────


def cmd_queue_sweep_snoozed(args: argparse.Namespace) -> int:
    """Re-open queue items where snooze_until <= today.

    Walks `<wiki>/_runtime/operator-queue/<bucket>/` for `*.md` files with
    `status: snoozed`. If `snooze_until` is past, flips status back to `open`
    and stamps `unsnoozed_at`. Atomic write per file.
    """
    from datetime import date

    config = cfg.load()
    queue_root = config.wiki_path / "_runtime" / "operator-queue"
    if not queue_root.exists():
        _info("No operator-queue directory — nothing to sweep")
        return 0

    today = date.today()
    swept = 0
    skipped = 0

    for bucket_dir in sorted(queue_root.iterdir()):
        if not bucket_dir.is_dir():
            continue
        for f in sorted(bucket_dir.glob("*.md")):
            status = _parse_frontmatter_field(f, "status")
            if status != "snoozed":
                continue
            until = _parse_frontmatter_field(f, "snooze_until")
            if not until:
                continue
            try:
                # Accept YYYY-MM-DD; ignore any time component.
                until_date = date.fromisoformat(until.split("T", 1)[0])
            except ValueError:
                _info(f"Skipping {f.relative_to(config.wiki_path)} — bad snooze_until: {until!r}")
                skipped += 1
                continue
            if until_date > today:
                continue
            _patch_frontmatter_field(f, "status", "open")
            _patch_frontmatter_field(f, "unsnoozed_at", _iso_now())
            _ok(f"Re-opened {f.relative_to(config.wiki_path)} (snoozed until {until_date})")
            swept += 1

    _ok(f"Snooze sweep complete: {swept} re-opened, {skipped} skipped")
    return 0


def _iso_now() -> str:
    from datetime import datetime, timezone
    return datetime.now(timezone.utc).isoformat(timespec="seconds")


def _parse_frontmatter_field(path: Path, field: str) -> str | None:
    """Tiny YAML frontmatter parser — pulls `field: value` from the first block."""
    try:
        text = path.read_text()
    except OSError:
        return None
    if not text.startswith("---"):
        return None
    end = text.find("\n---", 3)
    if end < 0:
        return None
    block = text[3:end]
    for line in block.splitlines():
        if ":" in line:
            key, _, value = line.partition(":")
            if key.strip() == field:
                return value.strip().strip('"\'')
    return None


def cmd_doctor(args: argparse.Namespace) -> int:
    """Health check. Exit 0 = green, 1 = yellow, 2 = red."""
    issues: list[str] = []
    warnings: list[str] = []
    hermes = shutil.which("hermes")
    if not hermes:
        issues.append("hermes binary not on PATH")
    else:
        _ok(f"hermes found at {hermes}")

    try:
        config = cfg.load()
    except SystemExit:
        issues.append("doge.config.yaml missing or invalid")
        config = None  # type: ignore[assignment]

    if config:
        _ok(f"city: {config.city.name} ({config.city.domain})")
        if not config.wiki_path.exists():
            warnings.append(f"wiki path not found: {config.wiki_path}")
        else:
            _ok(f"wiki: {config.wiki_path}")
            for sub in ("Sitemap", "Investigations", "Findings", "Vault", "_runtime", "_data"):
                p = config.wiki_path / sub
                if not p.exists():
                    warnings.append(f"missing wiki dir: {p}")
        if not config.db_path.exists():
            warnings.append(f"DB not initialized: {config.db_path}")
        else:
            _ok(f"db: {config.db_path}")

    if hermes:
        existing = subprocess.run(
            [hermes, "profile", "list"], check=False, text=True, capture_output=True
        ).stdout
        for role in cfg.PROFILE_ROLES:
            if role in existing.split():
                _ok(f"profile: {role}")
            else:
                warnings.append(f"profile not created: {role}")

    print()
    if issues:
        for i in issues:
            _err(i)
        return 2
    if warnings:
        for w in warnings:
            print(f"⚠️  {w}")
        return 1
    _ok("All checks passed")
    return 0


# ─────────────────────────────────────────────────────────────────────────────
# Argparse wiring
# ─────────────────────────────────────────────────────────────────────────────


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog="centinel",
        description="Centinel dispatcher — setup, cron management, investigation registration.",
    )
    sub = p.add_subparsers(dest="command", required=True)

    sp = sub.add_parser("bootstrap-sitemap", help="Run sitemap-builder in bootstrap mode (wizard Step 5)")
    sp.add_argument("domain", nargs="?", default=None, help="Override city domain (default: from doge.config.yaml)")
    sp.set_defaults(func=cmd_bootstrap_sitemap)

    sp = sub.add_parser("setup-profiles", help="Idempotently create Hermes profiles for each role")
    sp.add_argument("--no-alias", action="store_true", help="Skip Hermes' wrapper alias creation")
    sp.set_defaults(func=cmd_setup_profiles)

    sp = sub.add_parser("setup-cron", help="Idempotently register paused recurring cron jobs")
    sp.set_defaults(func=cmd_setup_cron)

    cron = sub.add_parser("cron", help="Manage Centinel-owned cron jobs")
    cron_sub = cron.add_subparsers(dest="cron_action", required=True)
    cron_sub.add_parser("resume-all", help="Wizard Step 7: activate every paused Centinel job").set_defaults(func=cmd_cron_resume_all)
    cron_sub.add_parser("pause-all", help="Emergency stop — pause every Centinel job").set_defaults(func=cmd_cron_pause_all)
    cron_sub.add_parser("list", help="List all Centinel-owned cron jobs").set_defaults(func=cmd_cron_list)

    inv = sub.add_parser("investigate", help="Investigation lifecycle commands")
    inv_sub = inv.add_subparsers(dest="inv_action", required=True)
    inv_reg = inv_sub.add_parser("register", help="Register a per-investigation cron entry")
    inv_reg.add_argument("slug", help="Investigation slug (matches Investigations/<slug>.md)")
    inv_reg.set_defaults(func=cmd_investigate_register)
    inv_pause = inv_sub.add_parser("pause", help="Pause an investigation (frontmatter + cron)")
    inv_pause.add_argument("slug")
    inv_pause.set_defaults(func=cmd_investigate_pause)
    inv_resume = inv_sub.add_parser("resume", help="Resume an investigation (frontmatter + cron)")
    inv_resume.add_argument("slug")
    inv_resume.set_defaults(func=cmd_investigate_resume)
    inv_trig = inv_sub.add_parser("trigger", help="Drop an inbox request — runs on next investigator tick")
    inv_trig.add_argument("slug")
    inv_trig.set_defaults(func=cmd_investigate_trigger)

    watch = sub.add_parser("watch", help="Watch lifecycle commands")
    watch_sub = watch.add_subparsers(dest="watch_action", required=True)
    watch_trig = watch_sub.add_parser("trigger", help="Drop an inbox request — runs on next watch-runner tick")
    watch_trig.add_argument("watch_id", nargs="?", default=None, help="Watch id (omit to run all)")
    watch_trig.set_defaults(func=cmd_watch_trigger)

    queue = sub.add_parser("queue", help="Operator-queue maintenance")
    queue_sub = queue.add_subparsers(dest="queue_action", required=True)
    queue_sub.add_parser(
        "sweep-snoozed",
        help="Re-open queue items where snooze_until <= today",
    ).set_defaults(func=cmd_queue_sweep_snoozed)

    sp = sub.add_parser("doctor", help="Health check")
    sp.set_defaults(func=cmd_doctor)

    return p


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
