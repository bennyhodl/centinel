---
title: Centinel — Installation & First Investigation
status: 📝 living doc (current state)
created: 2026-04-28
updated: 2026-05-21
parent: README.md
---

# Installation & First Investigation

The honest, current-state path from `git clone` to "I have an investigation
running." Some pieces are fully wired; others are spec-only. This doc calls out
what works **today** vs. what's still a stub, so you don't burn an hour
debugging something that was never built.

> Status legend: ✅ wired, 🟡 partially wired (works for the happy path),
> 🚧 spec-only, ❌ not yet started.

## Prerequisites

| Tool | Version | Why |
|---|---|---|
| Node.js | ≥ 20 | centinel-server (TS) + web app |
| `pnpm` | ≥ 9 | Workspace package manager |
| Python | ≥ 3.11 | Skill helper scripts under `skills/*/scripts/` |
| Anthropic API key | — | centinel-server drives roles via pi-agent → Anthropic |
| Docker (optional) | latest | For Datasette + production deploy |
| `sqlite3` CLI (optional) | any | Bootstrap DB init falls back gracefully if missing |

Centinel runs roles inside `centinel-server` (a TypeScript service built on
`@mariozechner/pi-coding-agent`). There is no separate agent runtime to
install — `./bootstrap` builds and wires everything from this repo.

## Step 1 — Clone

```bash
git clone https://github.com/bennyhodl/tampa-doge centinel
cd centinel
```

The repo path is still `tampa-doge` on GitHub for now; the project itself is
**Centinel**.

## Step 2 — Run `./bootstrap` ✅

```bash
./bootstrap
```

What it does (idempotent — safe to re-run after `git pull`):

1. Checks `node`, `pnpm`, `python3`, `docker` are installed.
2. Auto-installs `pyyaml` if missing (used by skill helper scripts).
3. Copies `doge.config.yaml.example` → `doge.config.yaml` and opens it in
   `$EDITOR` so you can fill in:
   - `city.name` (e.g. `Tampa`)
   - `city.slug` (e.g. `tampa` — used as DB filename)
   - `city.domain` (e.g. `tampa.gov`)
   - `wiki.path` (defaults `~/wiki/Tampa`)
4. Copies `.env.example` → `.env` (or creates a stub). You fill:
   - `CENTINEL_PASSWORD` — basic-auth password for the web app
   - `ANTHROPIC_API_KEY` — used by centinel-server's role runtime
5. `pnpm install` + `pnpm --filter centinel-server build`.
6. Creates the entire wiki tree at `wiki.path`.
7. Initializes an empty SQLite DB at `<wiki>/_data/<slug>.db`.
8. Seeds the canonical cron jobs into `.runtime/cron.json` in the
   **paused** state (so nothing fires until the wizard's Step 7).

**Verify:**

```bash
./bin/centinel doctor
```

Should show ✅ for node / city / wiki / db / server build. Yellow warnings
are OK at this stage.

## Step 3 — Start the runtime ✅

Open two terminals.

```bash
# Terminal A — centinel-server (cron + HTTP for roles)
./bin/centinel-server
```

```bash
# Terminal B — web app
pnpm --filter centinel dev
```

Open `http://localhost:3000`. Browser prompts for basic auth — leave username
blank, password is `CENTINEL_PASSWORD` from `.env`.

Until the setup wizard reports complete, every route redirects to `/setup`.

## Step 4 — Walk the setup wizard

The wizard at `/setup` has 7 steps:

| Step | Status | What happens |
|---|---|---|
| 1 — City domain | ✅ | Validates and persists |
| 2 — Project name | ✅ | Defaults to "Centinel" |
| 3 — Watch presets | ✅ | Persists checked IDs |
| 4 — Briefing channel | ✅ | Optional Discord/Telegram |
| 5 — Start bootstrap | 🟡 | Asks centinel-server to run the `editor` role with `bootstrap-sitemap <domain>` |
| 6 — Review sitemap | ✅ | SSE log tail at `/api/setup/bootstrap-log` |
| 7 — Activate cron | ✅ | Calls `./bin/centinel cron resume-all` |

**🟡 Step 5 caveat:** The dispatcher *will* successfully spawn the
`sitemap-builder` skill inside the `editor` role. But the skill body itself is
still spec-shaped — it describes the procedure but several pi-agent tools
(`web_fetch`, `qmd_search`, `db_query`, `vault_put`) are still **stubs**.
Expect a partial sitemap on first run; it improves as the tool implementations
land. The wizard advances either way.

**🟡 Step 7 caveat:** `cron resume-all` flips paused → active. The cron
**scheduler** (inside centinel-server) runs whatever prompt was registered, in
the right role. Whether that prompt does useful work depends on the underlying
tool implementations (see the skill matrix below).

## Step 5 — Beginning an investigation

Here's where the gap between design and implementation is most visible.

### What the spec promises

You open `/chat` and tell the Editor:

> "Start an investigation tracking parks contractors since 2021."

The Editor uses the `delegate` tool with `target: 'investigator'` and a
brief, which the role runtime turns into a registered investigation + cron
entry.

### What works today (manual path)

```bash
# 1. Register the investigation (writes YAML + seeds a cron entry, paused)
./bin/centinel investigate register parks-contractors \
  --cron "0 2 * * *"

# 2. Resume the investigator tick so cron actually fires
./bin/centinel cron resume investigator-tick
```

The `investigator` role inside centinel-server picks it up on the next tick,
reads its `civic-investigator` skill, and runs.

**🚧 What's not built that the spec promises:**

| Piece | Status | Workaround |
|---|---|---|
| Editor's `delegate`-driven `register_investigation` flow | 🚧 | Hand-write the YAML + run dispatcher |
| `/investigations/new` web form | ❌ | Same |
| `docker-compose.yml` | ❌ | Run web app via `pnpm dev` instead |

## Skill implementation matrix

What you can expect each role + skill to actually do today:

| Role / Skill | Spec | Implementation | What runs |
|---|---|---|---|
| `editor` / `sitemap-builder` | ✅ | 🟡 | Crawls and writes partial sitemap; classification works, descriptions partial |
| `investigator` / `civic-investigator` | ✅ | 🚧 | Loads and starts; produces partial output until `web_fetch`/`qmd_search` land |
| `archivist` / `civic-archivist` | ✅ | 🚧 | Same — needs `vault_*` tools to fully function |
| `data-reporter` / `civic-data-reporter` | ✅ | 🚧 | Needs `db_query` tool to land |
| `watch-runner` / `civic-watch-runner` | ✅ | 🚧 | Needs Match DSL evaluator + sitemap diff source |

Each `SKILL.md` opens with the locked spec and the QMD-mandatory rule. The
**procedure** sections are the working parts; the **tools** sections describe
what the role expects to be available, which depends on tool implementation.

## Common operations

### Check health

```bash
./bin/centinel doctor
```

### Run a role interactively

```bash
./bin/centinel role investigator --interactive
./bin/centinel role archivist -q "drain inbox now"
```

### See all Centinel cron jobs

```bash
./bin/centinel cron list
```

### Emergency stop

```bash
./bin/centinel cron pause-all
```

### Re-activate after a stop

```bash
./bin/centinel cron resume-all
```

### Trigger a single job now (debugging)

```bash
./bin/centinel cron list                     # find job name
./bin/centinel cron run <job-name>           # fires immediately
```

### Re-run bootstrap after a `git pull`

```bash
./bootstrap                # idempotent — picks up new skills/presets/migrations
./bin/centinel doctor      # confirm no regressions
```

## Troubleshooting

**`./bin/centinel doctor` says server build missing** — re-run
`pnpm --filter centinel-server build`. The bootstrap step is idempotent.

**Step 5 spawns but log shows nothing** — open
`<wiki>/_runtime/logs/bootstrap-sitemap.log` directly. The SSE endpoint
streams that file; if centinel-server crashed at startup, the log will show
why.

**Step 7 errors with "cron job not found"** — the paused jobs were never
seeded. Run `./bin/centinel cron seed-paused` from a terminal and try
Step 7 again.

**The web app loads but `/chat` returns 500** — centinel-server isn't
running (Terminal A above) or `ANTHROPIC_API_KEY` is missing/invalid. Check
the terminal output where you launched `./bin/centinel-server`.

**`PromiseWithResolvers` TypeScript errors** — pre-existing tsconfig issue,
unrelated to functionality. Build still works via Next's bundler.

## What to read next

- **`docs/PI_MIGRATION_PLAN.md`** — the current architecture: how roles run
  inside centinel-server, how the `delegate` tool dispatches, and which
  tools are still stubs.
- **`docs/EDITOR_ANSWER_SOURCES.md`** — the locked priority order
  (DB → Vault → Findings → Investigations → Entities → QMD-always) and the
  rule that QMD runs on every freeform question.
- **`docs/AGENT_ROSTER.md`** — Spotlight model mapping. Which role owns
  which skill, who writes where.
- **`docs/RUNTIME_PROTOCOL.md`** — the inbox/outbox/status filesystem
  protocol roles use to coordinate.
- **`docs/WEB_APP_DESIGN.md`** — the viewer + control panel spec.
- **`docs/REPO_AND_DISTRIBUTION.md`** — fork/distribute model for other cities.

## Honest summary for v0.1

- `./bootstrap` is end-to-end working.
- The web wizard is end-to-end working with live shell-outs.
- Role registration, cron registration, dispatcher subcommands all work.
- **Tool implementations are spec → partial.** First investigations will run
  the Investigator's procedure but may produce thin output until the
  underlying tools (`web_fetch`, `qmd_search`, `db_query`, `vault_put`) land.
- The Editor's chat-driven `delegate`-to-investigator flow is not yet wired —
  hand-write the YAML + call the dispatcher.

When the stub tools land, the gap from "fresh clone" to "running investigation
via chat" will close.
