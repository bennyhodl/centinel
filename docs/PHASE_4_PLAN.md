---
title: Centinel — Phase 4 Cleanup Plan
status: 🧠 Draft v0
created: 2026-05-21
parent: docs/PI_MIGRATION_PLAN.md
---

# Phase 4 — Cleanup pass

Goal: remove every Hermes-specific code path and dead artifact, scrub the
docs and skills to match pi-agent reality, and make a fresh-clone install
work without `hermes` installed. **No new features.**

---

## Inventory — what's still hanging around

### Code

| Path | Status | Action |
|---|---|---|
| `bin/centinel-investigator` | Hermes shim | **delete** |
| `bin/centinel-archivist` | Hermes shim | **delete** |
| `bin/centinel-data-reporter` | Hermes shim | **delete** |
| `bin/centinel-watch-runner` | Hermes shim | **delete** |
| `bin/centinel` | already new (TS) | keep |
| `bin/centinel-server` | already new (TS) | keep |
| `bin/README.md` | mentions all five shims | rewrite |
| `lib/` (Python dispatcher: `cli.py`, `config.py`, `__pycache__/`) | Python | **delete the directory** |
| `bootstrap` | shells `hermes`, installs profiles, registers Hermes cron | **rewrite**: node/pnpm deps, no Hermes, wires `centinel cron seed-paused` + docker compose for centinel-server |
| `app/src/lib/editor-persona.ts` | persona moved to server | **delete** |
| `app/src/lib/config.ts` | dead `hermesApi*` getters | drop them |
| `app/package.json` | `openai` dep only used by old chat route | drop it; refresh lockfile |
| `app/.env.example` (if present) | likely has `HERMES_*` | scrub |
| Root `.env.example` (if present) | same | scrub |

### Docs

| Path | Status | Action |
|---|---|---|
| `README.md` (top-level) | "Built as a self-hosted Hermes plugin" + Hermes stack section | rewrite intro, env-vars table, develop section |
| `docs/AGENT_INVOCATION.md` | entire doc is about Hermes lanes | add `SUPERSEDED — see PI_MIGRATION_PLAN.md` banner; keep file for historical reference |
| `docs/AGENT_ROSTER.md` | "Hermes profile" everywhere | terminology pass: profile → role, delegate_task → delegate |
| `docs/RUNTIME_PROTOCOL.md` | filesystem protocol — still accurate | small pass: Hermes cron mentions → centinel-server cron |
| `docs/INSTALLATION.md` | fresh-clone walkthrough still Hermes-flavored | full rewrite around centinel-server + bin/centinel |
| `docs/REPO_AND_DISTRIBUTION.md` | references Hermes profiles + cron | terminology pass |
| `docs/EDITOR_ANSWER_SOURCES.md` | "QMD search" + db_query — still accurate, no Hermes mentions | small pass |
| `docs/PLAN.md` | status block stale | refresh status, point at PI_MIGRATION_PLAN.md |
| `docs/PI_MIGRATION_PLAN.md` | Phase 4 placeholder | mark complete when done |
| `docs/EDITOR_PERSONA.md` | locked content | no changes |
| `docs/SCRAPER_AND_EXTRACTORS.md` | check for Hermes refs | quick scrub |
| `docs/ORG_STRUCTURE_AND_WORKFLOW.md` | Spotlight model, agent-agnostic | likely no changes |

### Skills

| Path | Action |
|---|---|
| `skills/README.md` | scrub "Hermes skill" → "pi-agent skill", drop profile mentions |
| `skills/civic-investigator/SKILL.md` | "You run inside the `investigator` Hermes profile" → "You run as the investigator role"; `delegate_task(skill=...)` → `delegate(target=...)` |
| `skills/civic-archivist/SKILL.md` | same pass |
| `skills/civic-data-reporter/SKILL.md` | same pass |
| `skills/civic-watch-runner/SKILL.md` | same pass |
| `skills/sitemap-builder/SKILL.md` | same pass |
| `skills/*/scripts/`, `references/`, `templates/` | grep for Hermes refs and `delegate_task`; usually clean but verify |

### Final tasks not covered above

- Add `centinel role <r> --interactive` (pi's `InteractiveMode`) —
  replaces the deleted shims with a real interactive entry point per the
  migration plan promise.
- Add `centinel doctor` — health checks the new stack (server reachable,
  cron table valid, skills resolvable, wiki path readable, API key
  present, etc.).
- Add `centinel cron seed-paused` — explicit CLI for bootstrap to seed the
  cron table without booting the server.

---

## Ordered work-list (6 tasks)

Each task is independently shippable. Run them in order; don't combine.

### Task 1 — Delete dead app code

**Files:**
- `app/src/lib/editor-persona.ts` → delete
- `app/src/lib/config.ts` → remove `hermesApiUrl`, `hermesApiKey` getters
- `app/package.json` → remove `openai` dependency; re-run `pnpm install`
- `app/AGENTS.md`, `app/CLAUDE.md` → quick grep + scrub if they mention Hermes endpoints

**Verify:**
- `pnpm --filter centinel exec tsc --noEmit` clean
- `pnpm --filter centinel build` clean
- `grep -rn "hermes\|HERMES\|editor-persona" app/src` returns 0 hits

**Risk:** low — none of these are imported anywhere after Phase 3.

---

### Task 2 — Implement `centinel role <r> --interactive`

The migration plan promised this as the replacement for `bin/centinel-<role>`.
Opens pi's full TUI scoped to a role's skill + tools.

**Files:**
- `server/src/cli.ts` → add `--interactive` flag handling to the `role` command
- `server/src/runtime/interactive.ts` (new) → builds an `AgentSessionRuntime`
  for a given role and hands it to `InteractiveMode` per pi's SDK docs

**Behavior:**
```bash
centinel role investigator --interactive       # full TUI session
centinel role investigator -p "..."             # one-shot (existing)
```

**Verify:**
- Build clean
- `centinel role investigator --interactive` opens the pi TUI with the
  investigator skill loaded and the same custom tools the cron path uses
- Exit drops back to the shell cleanly

**Risk:** medium — pi's `InteractiveMode` setup is more complex than the
print/SDK paths. Keep the wiring isolated in `interactive.ts` so a failure
here doesn't touch other code paths.

**Out of scope:** session resume across `--interactive` invocations (pi
defaults already cover this).

---

### Task 3 — Implement `centinel doctor`

A diagnostic command bootstrap and operators can run to check the new
stack.

**Checks:**
1. Server reachable on configured host/port (`GET /health`).
2. `.runtime/` writable; `runs/`, `sessions/`, `cron.json` either exist or
   can be created.
3. `skills/<each-role>/SKILL.md` exists and is readable.
4. `docs/EDITOR_PERSONA.md` (or `$CENTINEL_EDITOR_PERSONA_PATH`) exists.
5. Wiki path from `doge.config.yaml` exists and is writable.
6. At least one provider API key is configured (`ANTHROPIC_API_KEY`,
   `OPENAI_API_KEY`, or an OAuth token in `~/.pi/agent/auth.json`).
7. `croner` parses every entry in the cron table.

**Files:**
- `server/src/doctor.ts` (new) — pure-function checks returning
  `{ name, ok, detail }[]`
- `server/src/cli.ts` — wire `centinel doctor`
- (Optional) `GET /doctor` HTTP endpoint that reuses the same checks

**Verify:**
- `centinel doctor` prints a checklist with ✅/❌ per item; exit code 0
  if all pass, 1 if any fail.

**Risk:** low.

---

### Task 4 — Rewrite `bootstrap`

The existing Bash script is ~370 lines and ~half is Hermes plumbing.

**New responsibilities:**
1. Sanity checks: `node ≥ 20`, `pnpm`, `docker`. Drop `hermes` and
   `python3`.
2. `doge.config.yaml` / `.env` from examples (mostly unchanged).
3. Wiki tree creation + SQLite init (unchanged).
4. `pnpm install` at repo root (workspace).
5. `pnpm --filter @centinel/server build`.
6. `bin/centinel cron seed-paused` — replaces Hermes profile + paused-cron
   registration. Server currently auto-seeds on boot; add an explicit CLI
   command so bootstrap can do it without booting.
7. Docker compose up: web + datasette + **centinel-server**.
8. `bin/centinel doctor` for sanity.
9. Print the next-step URL.

**Files:**
- `bootstrap` — rewrite
- `server/src/cli.ts` — add `centinel cron seed-paused` subcommand
- `server/src/cron/cronTable.ts` — already does the work; just expose via
  CLI
- Docker compose file (if present in `app/` or root) — add centinel-server
  service

**Verify:**
- Fresh clone on a box without Hermes → `./bootstrap` runs to completion;
  operator can reach `/health` + `/chat`.
- Re-running `./bootstrap` is idempotent.
- Old Hermes-related lines are gone from the script.

**Risk:** medium — bootstrap is the operator-facing front door. Test
re-running carefully.

---

### Task 5 — Delete legacy bins, Python, and dead Hermes references

**Deletes:**
```
rm bin/centinel-investigator
rm bin/centinel-archivist
rm bin/centinel-data-reporter
rm bin/centinel-watch-runner
rm -rf lib/                              # Python dispatcher + __pycache__
```

**Updates:**
- `bin/README.md` → rewrite around `centinel` + `centinel-server` only
  (mention `centinel role <r> --interactive` as the replacement for the
  per-role shims)
- `.gitignore` → drop `lib/__pycache__/` (the dir is gone)
- Root `package.json` → drop any references to lib/ in scripts (if any)

**Verify:**
- `grep -rn "hermes\|Hermes\|hermes-agent" . --exclude-dir=node_modules \
    --exclude-dir=.git --exclude-dir=dist` returns 0 hits outside of
  `docs/AGENT_INVOCATION.md` (SUPERSEDED banner, see Task 6).
- `grep -rn "bin/centinel-investigator\|bin/centinel-archivist\
    |bin/centinel-data-reporter\|bin/centinel-watch-runner" .` returns 0
  hits.
- `find . -name "*.py" -not -path "./node_modules/*"` returns 0 hits.

**Risk:** low. Easy to revert via git if needed.

---

### Task 6 — Docs and skills pass

The most time-consuming task. Done last so all the code shape is stable.

**Skills (5 files + the README):**
Find/replace pass:
- "Hermes profile" → "Centinel role"
- "Hermes skill" → "pi-agent skill"
- "your `<role>` Hermes profile (`~/.hermes/profiles/<role>/`)" →
  "the `<role>` role inside centinel-server"
- "`delegate_task(skill=…)`" → "the `delegate` tool with `target: '<role>'`"
- "`hermes --profile <r> ...`" → "`centinel role <r> ...`"
- Toolset references: pi tools are `read/write/edit/bash`; web-related
  calls use the role's `web_fetch` (still a stub; mark `[TODO: stub]` if
  the skill copy depends on real behavior)

Each SKILL.md gets a top-of-file note:
"This skill loads into the pi-agent runtime via `roles/<name>.ts`. See
`docs/PI_MIGRATION_PLAN.md`."

**Docs:**

| File | Pass |
|---|---|
| `docs/AGENT_INVOCATION.md` | Add header banner: `> SUPERSEDED by docs/PI_MIGRATION_PLAN.md. Retained for historical context.` Don't delete — the lane model is still pedagogically useful. |
| `docs/AGENT_ROSTER.md` | Replace "Hermes profile" with "role"; drop the Hermes invocation block. Spotlight mapping unchanged. |
| `docs/RUNTIME_PROTOCOL.md` | Change "Hermes cron tick" to "centinel-server cron tick" wherever it appears. Filesystem protocol unchanged. |
| `docs/INSTALLATION.md` | Rewrite to: `./bootstrap` → `pnpm --filter centinel dev` (Next.js) + `centinel-server` (or docker). Step through first investigation via `centinel investigate register` + `centinel cron resume investigator-tick`. |
| `docs/REPO_AND_DISTRIBUTION.md` | Remove Hermes profile/cron setup. Replace with the centinel CLI and `.runtime/` layout. |
| `docs/EDITOR_ANSWER_SOURCES.md` | Verify tool names match what the editor role now has registered. |
| `docs/PLAN.md` | Status block update: "v0.1: Phases 0–4 of pi-agent migration complete. Next: real tool implementations." |
| `docs/PI_MIGRATION_PLAN.md` | Mark Phase 4 complete. |
| `README.md` (top-level) | Rewrite intro: "Built on `@mariozechner/pi-coding-agent`." Update env-vars table (drop `HERMES_*`, add `CENTINEL_SERVER_URL`, `ANTHROPIC_API_KEY`, `CENTINEL_EDITOR_PERSONA_PATH`). Update "Develop" section. Drop the "Hermes plugin" framing. |

**Verify:**
- `grep -rn -i "hermes" docs/ skills/ README.md` returns only the explicit
  historical mentions in `AGENT_INVOCATION.md` under the SUPERSEDED
  banner.
- Every doc cross-link still resolves.

**Risk:** low for content correctness; tedious. Best done as one focused
PR.

---

## What this pass explicitly does NOT do

These are real follow-ons but not "cleanup":

1. **Real implementations for stub tools.** `qmd_search`, `db_query`,
   `vault_put`, `web_fetch` keep returning `not_yet_implemented`. Each is
   a focused mini-project of its own:
   - `db_query`: read-only `better-sqlite3` against `<wiki>/_data/<city>.db`
   - `vault_put`: hash + write under `<wiki>/Vault/` + manifest update
   - `web_fetch`: `fetch()` + readability/markdown conversion
   - `qmd_search`: requires the qmd binary or a TS reimplementation
     (BM25 + embedding + reranker); the largest of the four
2. **Rich chat UI surface.** The Next.js `/chat` page still renders plain
   text deltas. Tool-call cards, delegation expanders, live `/status`
   integration = UI sprint, not cleanup.
3. **Per-editor-session delegate concurrency cap.** Open question #3 in
   the migration plan. Today's cap is global (2). Small functional change,
   not cleanup.
4. **Session retention policy.** Open question #4 — pi sessions grow
   forever. A rotation cron is a small new feature.
5. **`/status` board live render.** The migration plan mentioned "live
   tail of tool calls per run" — that's a new web page, not cleanup.

---

## Suggested order + estimated effort

| Task | Effort | Why this order |
|---|---|---|
| 1. Delete dead app code | 15 min | No deps, isolates the Next.js app cleanly first |
| 2. `--interactive` mode | 1–2 h | Needed before deleting the Hermes shims so operators have a replacement entry point |
| 3. `centinel doctor` | 45 min | Needed before rewriting bootstrap so bootstrap can call it |
| 4. Rewrite `bootstrap` | 1–2 h | Depends on doctor + `cron seed-paused` |
| 5. Delete shims, `lib/`, Hermes refs in code | 30 min | Safe once Tasks 2 + 4 land |
| 6. Docs and skills scrub | 2–3 h | Last, so all language matches the now-stable code |

**Total:** ~6–9 hours. Splittable across 2–3 sessions.

---

## Verification at the end

A green Phase 4 means:

1. ✅ Fresh `git clone` on a box without Hermes → `./bootstrap &&
   ./bin/centinel server start` → operator can chat at `/chat` and fire
   `centinel cron fire investigator-tick`.
2. ✅ `grep -rni "hermes" .` (excluding `node_modules`, `.git`, `dist`)
   returns only intentional historical references in
   `docs/AGENT_INVOCATION.md`.
3. ✅ No Python files. No per-role bin shims. No `openai` dep in the app.
4. ✅ `centinel doctor` passes on a configured machine.
5. ✅ Docs and skills speak the pi-agent vocabulary throughout.
