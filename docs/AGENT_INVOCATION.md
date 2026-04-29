---
title: Centinel — Agent Invocation Model
status: 🔒 Locked v2
created: 2026-04-28
updated: 2026-04-28
parent: ABOUT.md
---

# Agent Invocation Model

How agents are launched and how they call each other. Locked v2 (2026-04-28).

## TL;DR

There is no `hermes session run <name>` primitive. A Hermes session is just
`(profile + skills + prompt)`. Centinel composes that into three distinct
**lanes** and a CLI wrapper for setup/operator-terminal use.

| Lane | Use for | Mechanism | Latency |
|---|---|---|---|
| **Sync delegation** | Editor needs depth on a question, mid-chat | `delegate_task(skill=...)` from Editor's tool calls | seconds |
| **Async inbox** | Run a real investigation in the background | Editor writes `<wiki>/_runtime/inbox/<role>/<task>.md`; cron-tick drains | minutes–hours |
| **Autonomous cron** | Scheduled scans, watches, lints, briefings | `hermes --profile <role> cron create '<sched>' --skill <skill> --name <n> "<prompt>"` | scheduled |

The `bin/centinel*` CLI is **only** for setup-time wiring and operator-terminal
ergonomics. It does **not** sit in the runtime loop.

## Profiles

Each non-Editor role is a Hermes profile under `~/.hermes/profiles/<role>/`
with its own config, skills, sessions, memory, and credential pool. **Profile
names are short**: `investigator`, `archivist`, `data-reporter`, `watch-runner`.
The Editor lives in the default profile.

Profiles are a **deployment** concern, not a runtime delegation concern. They
exist so:

- Cron-driven runs have isolated session history (won't pollute Editor chat).
- Operator's terminal access (`bin/centinel-investigator`) opens a clean
  session that doesn't leak into Editor's context.
- Each role can have a different model/credential pool if ever needed.

**All profiles share the same access** (same wiki, same DB, same vault) — they
differ only in role/skills/session history. Knowledge lives in the wiki and
DB, not in profile memory. Profile memory is a deployment convenience, never
the source of truth.

## Lane 1 — Sync delegation (Editor → specialist, in-chat)

When the Editor is mid-chat and needs Investigator-shaped analysis, it calls
the Hermes `delegate_task` tool with the specialist skill loaded:

```
delegate_task(
  goal="analyze whether ACME, BlueRock, and CityWide share principals based on existing wiki/DB material",
  toolsets=["file", "qmd-search", "code_execution"],
  context="<paste relevant wiki/DB excerpts>"
)
```

The subagent runs in the **same process** with the specified skills loaded
into its system prompt. It does NOT cross profile boundaries — it borrows the
default profile's process — but that's fine, because:

- The specialist's durable knowledge lives in wiki/DB, not in profile memory.
- The subagent has full read access to the same wiki/DB the standalone
  specialist would.
- Latency is seconds, not minutes — keeps chat UX snappy.

Use sync delegation when:
- Question is answerable from existing structured material with light analysis.
- A quick depth-pass is enough — no fresh ingest needed.
- Editor wants a summary back, not a wiki-page-shaped artifact.

**Do not** use sync delegation for: fresh crawls, multi-hour investigations,
or anything that should produce a durable wiki/DB write the operator needs to
review later. Those go to Lane 2.

## Lane 2 — Async inbox (Editor → specialist, durable)

When a question warrants a real investigation, the Editor writes a request
file to the specialist's inbox:

```
<wiki>/_runtime/inbox/investigator/2026-04-28-acme-relationships.md
```

with frontmatter (per `RUNTIME_PROTOCOL.md`) and tells the human in chat:

> "I've queued a deeper look into ACME's relationships with the Investigator.
> First results expected in tomorrow's huddle. Preliminary answer based on
> what I have now: ..."

The Investigator's cron tick (autonomous, Lane 3) drains its inbox, runs each
task, and writes results to:

- `<wiki>/Investigations/<slug>.md` — investigation page (append-mode)
- `<wiki>/Findings/raw/` — atomic findings if any
- `<wiki>/_runtime/outbox/investigator/<task>-result.md` — completion notice

The next time the human chats with the Editor, it sees the outbox entry and
surfaces the result.

Use async inbox when:
- Fresh crawl/ingest is required.
- The result should be a durable wiki/DB artifact.
- The operator will review/cite it later (i.e., it's "real journalism work").

## Lane 3 — Autonomous cron

Scheduled, profile-isolated runs registered at bootstrap and updated as
operator launches investigations.

```bash
hermes --profile investigator cron create '0 4 * * *' \
    --skill civic-investigator \
    --name "centinel-investigator-tick" \
    "drain $WIKI/_runtime/inbox/investigator/, run each pending task, write results"
```

Per-investigation crons get registered dynamically when the operator creates
an investigation:

```bash
hermes --profile investigator cron create "$SCHED" \
    --skill civic-investigator \
    --name "centinel-investigation-${SLUG}" \
    "run investigation $SLUG: read $WIKI/Investigations/$SLUG.md and append results"
```

> **Note:** `hermes cron create` does NOT take a `--profile` flag. Profile
> selection is the global `hermes --profile <name>` flag, placed BEFORE the
> subcommand. The flag for skills is `--skill` (singular, repeatable), not
> `--skills`. The dispatcher (`bin/centinel`) handles this correctly.

## Lane 4 (the CLI) — `bin/centinel*`

Setup wiring + operator terminal ergonomics. **Not in the runtime loop.**

```
bin/
├── centinel                       # main dispatcher (Python; lib/cli.py)
├── centinel-investigator          # exec hermes --profile investigator "$@"
├── centinel-archivist             # exec hermes --profile archivist "$@"
├── centinel-data-reporter         # exec hermes --profile data-reporter "$@"
└── centinel-watch-runner          # exec hermes --profile watch-runner "$@"
```

Centralizing into the dispatcher means there is **no** separate `bin/centinel-bootstrap` or `bin/centinel-doctor` — those are subcommands of `bin/centinel`. The role shims are the only standalone wrappers.

### What `bin/centinel` (the dispatcher) does

```
centinel bootstrap-sitemap <domain>     # called by web wizard Step 5
centinel cron resume-all                # called by web wizard Step 7
centinel investigate register <slug>    # called by /investigations server action
centinel doctor                         # health check, called post-bootstrap & on-demand
centinel setup-profiles                 # idempotent profile creation
```

The dispatcher knows the wiki layout, resolves `$WIKI` from `doge.config.yaml`,
parses investigation YAML frontmatter, and constructs the right `hermes`
invocation. The web app calls these subcommands; it never shells out to raw
`hermes`.

### What `bin/centinel-<role>` (the role shims) do

One-line wrappers giving the operator a friendly terminal entry into each
profile:

```bash
# bin/centinel-investigator
#!/usr/bin/env bash
exec hermes --profile investigator "$@"
```

```bash
# Operator usage:
centinel-investigator                   # interactive session
centinel-investigator -q "look into X"  # one-shot
centinel-investigator --continue        # resume last session in that profile
```

These are **not** called by the web app or cron. They exist so the operator
can `cd` to a terminal and talk directly to a role without remembering
`--profile` flags.

## How setup wires this together

Two layers, both idempotent:

### `./bootstrap` (shell script, one-time per install)

Per `REPO_AND_DISTRIBUTION.md`:

1. Sanity checks.
2. Read/generate `doge.config.yaml` and `.env`.
3. Create wiki structure.
4. Initialize SQLite DB.
5. Symlink skills into `~/.hermes/skills/centinel/`.
6. **Create profiles** (one per non-Editor role):
   ```bash
   for role in investigator archivist data-reporter watch-runner; do
     hermes profile create "$role" --clone || true   # idempotent
   done
   ```
7. **Generate the `bin/centinel*` wrappers** (chmod +x).
8. **Register paused cron jobs** via `register_cron` helper (see `REPO_AND_DISTRIBUTION.md`).
9. Bring up Docker (web + Datasette).

### Web wizard at `/setup` (per-city, in browser)

Steps 1–4: collect city domain, project name, presets, notification channel.

**Step 5 (Start Bootstrap):** server action calls
`bin/centinel bootstrap-sitemap <domain>` and tails the log via SSE. The
dispatcher invokes:

```bash
hermes -s sitemap-builder chat -q "bootstrap mode: build full sitemap for <domain>, write to $WIKI/Sitemap/"
```

Step 6: operator reviews the sitemap.

**Step 7 (Activate cron):** server action calls
`bin/centinel cron resume-all`. The dispatcher loops over the paused jobs and
runs `hermes cron resume <id>` on each.

After Step 7, the wizard marks setup complete in
`<wiki>/_runtime/setup-state.json` and the rest of the app unlocks.

## Acceptance criteria

- ✅ No reference to `hermes session run X` anywhere in the codebase or docs.
- ✅ Profile names are short (no `centinel-` prefix).
- ✅ Sync Editor→specialist uses `delegate_task`, not shell-out.
- ✅ Async Editor→specialist uses inbox files, drained by cron tick.
- ✅ `bin/centinel` is the only thing the web app shells out to.
- ✅ `bin/centinel-<role>` wrappers are 1-line shims for operator terminal use.
- ✅ Bootstrap creates all profiles and wrappers idempotently.
- ✅ Wizard Step 5 calls `centinel bootstrap-sitemap`, Step 7 calls `centinel cron resume-all`.
- ✅ Per-investigation cron registration goes through `centinel investigate register <slug>`.
