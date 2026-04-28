---
title: Centinel — Installation & First Investigation
status: 📝 living doc (current state)
created: 2026-04-28
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
| [Hermes Agent](https://hermes-agent.nousresearch.com/) | latest | The runtime every agent runs in |
| Python | ≥ 3.11 | Dispatcher (`bin/centinel`), config loader |
| `pyyaml` | latest | Auto-installed by `./bootstrap` if missing |
| Node.js | ≥ 20 | Web app |
| `pnpm` | ≥ 9 | Web app package manager |
| Docker (optional) | latest | For Datasette + production deploy |
| `sqlite3` CLI (optional) | any | Bootstrap DB init falls back gracefully if missing |

Configure Hermes with a working model first (`hermes setup` or `hermes model`).
Centinel doesn't pin a provider — anything Hermes supports works. The Editor
uses whatever the default profile is configured for.

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

1. Checks `hermes`, `python3`, `docker` are installed.
2. Auto-installs `pyyaml` if missing.
3. Copies `doge.config.yaml.example` → `doge.config.yaml` and opens it in
   `$EDITOR` so you can fill in:
   - `city.name` (e.g. `Tampa`)
   - `city.slug` (e.g. `tampa` — used as DB filename)
   - `city.domain` (e.g. `tampa.gov`)
   - `wiki.path` (defaults `~/wiki/Tampa`)
4. Copies `.env.example` → `.env` (or creates a stub). You fill:
   - `CENTINEL_PASSWORD` — basic-auth password for the web app
   - `HERMES_API_KEY` — for the `/chat` Editor endpoint
5. Creates the entire wiki tree at `wiki.path`.
6. Initializes an empty SQLite DB at `<wiki>/_data/<slug>.db`.
7. Symlinks `skills/*` into `~/.hermes/skills/centinel/`.
8. Creates Hermes profiles: `investigator`, `archivist`, `data-reporter`,
   `watch-runner`.
9. Registers paused cron jobs across all profiles.
10. Prints a PATH hint and `docker compose` next-step.

**Verify:**

```bash
./bin/centinel doctor
```

Should show ✅ for hermes / city / wiki / db / each profile. Yellow warnings
are OK at this stage.

## Step 3 — Start the web app ✅

```bash
cd app
pnpm install
pnpm approve-builds   # one-time, allows better-sqlite3 to compile
pnpm dev
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
| 5 — Start bootstrap | 🟡 | Spawns `bin/centinel bootstrap-sitemap <domain>` detached |
| 6 — Review sitemap | ✅ | SSE log tail at `/api/setup/bootstrap-log` |
| 7 — Activate cron | ✅ | Calls `bin/centinel cron resume-all`, surfaces errors |

**🟡 Step 5 caveat:** The dispatcher *will* successfully spawn the
`sitemap-builder` skill in a Hermes session. But the skill body itself is still
spec-shaped — it describes the procedure but doesn't yet have all the tool
calls wired. Expect a partial sitemap on first run; it improves as the skill
implementation lands. The wizard advances either way.

**🟡 Step 7 caveat:** `cron resume-all` flips paused → active. The cron
**scheduler** runs whatever prompt was registered, in the right profile, with
the right skill loaded. Whether that prompt does useful work depends on the
skill's implementation status (see the skill matrix below).

## Step 5 — Beginning an investigation

Here's where the gap between design and implementation is most visible.

### What the spec promises

You open `/chat` and tell the Editor:

> "Start an investigation tracking parks contractors since 2021."

The Editor:
1. Calls its `register_investigation` tool.
2. Tool writes `<wiki>/Investigations/parks-contractors.md` with frontmatter.
3. Tool calls `bin/centinel investigate register parks-contractors`.
4. Dispatcher reads `schedule:` from the YAML, runs
   `hermes --profile investigator cron create ...`.
5. Investigator picks it up at the next scheduled tick.

### What works today (manual path)

```bash
# 1. Write the YAML by hand
cat > "$CENTINEL_WIKI_PATH/Investigations/parks-contractors.md" <<'EOF'
---
slug: parks-contractors
title: Parks Department contractors since 2021
goal: Track every contractor awarded work by Parks dept FY2021–present
status: active
schedule: "0 2 * * *"
seeds:
  - https://www.tampa.gov/parks/contracts
depth: 2
focus_entities: []
created: 2026-04-28
---

## Run log

(empty — first run pending)
EOF

# 2. Register the cron entry
./bin/centinel investigate register parks-contractors

# 3. (optional) Trigger first run immediately rather than waiting for cron
hermes --profile investigator cron list      # find the job ID
hermes --profile investigator cron run <id>  # next tick fires it
```

The Investigator profile picks it up, reads its skill, and runs.

**🚧 What's not built that the spec promises:**

| Piece | Status | Workaround |
|---|---|---|
| Editor's `register_investigation` tool | 🚧 | Hand-write the YAML + run dispatcher |
| `/investigations/new` web form | ❌ | Same |
| Editor system-prompt loading | 🟡 | Editor is currently the default Hermes profile with no `EDITOR_PERSONA.md` baked into the system prompt — load it manually with `/skill` if you want the persona, or wait for the `civic-doge-editor` skill |
| `civic-doge-editor` skill | ❌ | Doesn't exist yet — referenced in cron registrations as a forward declaration |
| `docker-compose.yml` | ❌ | Run web app via `pnpm dev` instead |

## Skill implementation matrix

What you can expect each skill to actually do today:

| Skill | Spec | Implementation | What runs |
|---|---|---|---|
| `sitemap-builder` | ✅ | 🟡 | Crawls and writes partial sitemap; classification works, descriptions partial |
| `civic-investigator` | ✅ | 🚧 | Will load and start; produces partial output until tools land |
| `civic-archivist` | ✅ | 🚧 | Same — needs `vault_*` tools to fully function |
| `civic-data-reporter` | ✅ | 🚧 | Needs `db_query` tool to land |
| `civic-watch-runner` | ✅ | 🚧 | Needs Match DSL evaluator + sitemap diff source |

Each `SKILL.md` opens with the locked spec and the QMD-mandatory rule. The
**procedure** sections are the working parts; the **tools** sections describe
what the agent expects to be available, which depends on tool implementation.

## Common operations

### Check health

```bash
./bin/centinel doctor
```

### Open a per-role terminal session

```bash
./bin/centinel-investigator                    # interactive
./bin/centinel-investigator -q "look into X"   # one-shot
./bin/centinel-archivist --continue            # resume last session
```

### See all Centinel cron jobs across profiles

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
hermes --profile investigator cron list           # find ID
hermes --profile investigator cron run <id>       # fires next tick
```

### Re-run bootstrap after a `git pull`

```bash
./bootstrap                # idempotent — picks up new skills/presets/migrations
./bin/centinel doctor      # confirm no regressions
```

## Troubleshooting

**`./bootstrap` fails on `hermes profile create`** — your Hermes install is
older than `profile` support. Update Hermes: `hermes update`.

**`./bin/centinel doctor` says profile not created** — re-run
`./bin/centinel setup-profiles`. It's idempotent.

**Step 5 spawns but log shows nothing** — open
`<wiki>/_runtime/logs/bootstrap-sitemap.log` directly. The SSE endpoint
streams that file; if the dispatcher crashed at startup, the log will show
why.

**Step 7 errors with "cron job not found"** — the paused jobs were never
created. Run `./bin/centinel setup-cron` from a terminal and try Step 7 again.

**The web app loads but `/chat` returns 500** — `HERMES_API_URL` /
`HERMES_API_KEY` in `.env` aren't pointing at a reachable Hermes endpoint.
Test with `curl`. If you're running Hermes locally, default is
`http://localhost:8000/v1`.

**`PromiseWithResolvers` TypeScript errors** — pre-existing tsconfig issue,
unrelated to functionality. Build still works via Next's bundler.

## What to read next

- **`docs/AGENT_INVOCATION.md`** — how agents are launched, the three runtime
  lanes (sync `delegate_task`, async inbox, autonomous cron), and why
  `bin/centinel-*` exists.
- **`docs/EDITOR_ANSWER_SOURCES.md`** — the locked priority order
  (DB → Vault → Findings → Investigations → Entities → QMD-always) and the
  rule that QMD runs on every freeform question.
- **`docs/AGENT_ROSTER.md`** — Spotlight model mapping. Which profile owns
  which skill, who writes where.
- **`docs/RUNTIME_PROTOCOL.md`** — the inbox/outbox/status filesystem
  protocol agents use to coordinate.
- **`docs/WEB_APP_DESIGN.md`** — the viewer + control panel spec.
- **`docs/REPO_AND_DISTRIBUTION.md`** — fork/distribute model for other cities.

## Honest summary for v0.1

- `./bootstrap` is end-to-end working.
- The web wizard is end-to-end working with live shell-outs.
- Profile creation, cron registration, dispatcher subcommands all work.
- **Skill implementations are spec → partial.** First investigations will run
  the Investigator's procedure but may produce thin output until the
  underlying tools (`db_query`, `vault_read`, Match DSL evaluator) land.
- The Editor's chat-driven `register_investigation` is not yet wired —
  hand-write the YAML + call the dispatcher.

When `civic-doge-editor` skill, the editor tools, and the per-skill tool
backings land, the gap from "fresh clone" to "running investigation via chat"
will close.
