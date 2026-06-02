---
title: Centinel — Migration from Hermes to pi-agent
status: 🔒 Locked v1 (decisions confirmed 2026-05-21)
created: 2026-05-21
parent: README.md
supersedes_when_locked:
  - docs/AGENT_INVOCATION.md (Lane 1/3 details)
  - bootstrap (Hermes profile + cron sections)
  - bin/centinel-<role> shims
---

# Migrating Centinel off Hermes onto pi-agent

## Why

The Hermes runtime gave us five profiles, a cron daemon, `delegate_task`, and
an inbox/outbox filesystem protocol — but in practice it has been:

- **Flaky.** Profile cron jobs miss, sometimes silently. Re-runs are
  un-debuggable without diving into Hermes internals.
- **Opaque.** When the operator runs `bin/centinel-investigator -q "..."` they
  get a black-box pass through Hermes. There is no programmatic way to see
  *which* tool calls happened, what the model thought, what file writes
  landed, or to replay the run.
- **Coupled to two invocation paths.** The web app shells out to
  `bin/centinel <subcommand>` which shells out to `hermes`. Cron also shells
  out to `hermes`. Two paths, neither inspectable in one place.

The fix is to **make the runtime a long-running Node server inside the repo**
that owns the same five roles and the same scheduling, but is built on top of
[`@mariozechner/pi-coding-agent`](https://www.npmjs.com/package/@mariozechner/pi-coding-agent).
Every workflow becomes a TypeScript function that:

1. Can be invoked by an internal cron (the same way Hermes cron did it).
2. Can be invoked by an HTTP endpoint that the web app or the operator hits
   directly to get a streamed response back.
3. Subscribes to `AgentSession` events, so we always have a structured run
   log of what happened.

Same functions, two callers, one inspectable place.

---

## What stays exactly the same

These are the parts of the system that were designed correctly and are not
being rebuilt:

- **The wiki is still the brain.** `<wiki>/_runtime/inbox/`, `outbox/`,
  `status/board.md`, `huddle/`, `operator-queue/` per `RUNTIME_PROTOCOL.md` —
  unchanged. Filesystem ordering, idempotent IDs, monthly outbox rotation,
  status board flock, the whole protocol.
- **The five skill packages** under `skills/` (`sitemap-builder`,
  `civic-investigator`, `civic-archivist`, `civic-data-reporter`,
  `civic-watch-runner`). They are SKILL.md files plus templates/scripts; pi
  loads SKILL.md the same way Hermes did.
- **The Spotlight role mapping** in `docs/AGENT_ROSTER.md`. Five agents, same
  responsibilities, same human gates.
- **`doge.config.yaml`** as the per-city config (city, wiki path,
  watch presets, cron overrides, confidential investigations).
- **The web app** (Next.js 16, `/sitemap`, `/chat`, `/investigations`,
  `/status`, etc.). It talks to the runtime over HTTP instead of shelling
  out, but the routes themselves do not change.
- **`Editor persona`** lives in `EDITOR_PERSONA.md` and is injected as the
  system prompt for the editor role.

---

## What gets replaced

| Hermes concept | Replaced by |
|---|---|
| `~/.hermes/profiles/<role>/` | A `RoleConfig` object in TypeScript — model, skill, tools, system prompt, optional `cwd` |
| `hermes --profile <role>` | A `runRole(role, prompt)` SDK call building an `AgentSession` |
| `hermes cron create ...` | `centinel-server`'s internal scheduler (`node-cron`) firing the same `runRole(...)` |
| `delegate_task(skill=...)` (Lane 1) | A custom pi tool `delegate(role, prompt)` registered on the editor session that calls `runRole(...)` inline and returns its text |
| Inbox drain via Hermes cron tick (Lane 2) | `runRole("investigator", "drain inbox")` scheduled and also exposed as an HTTP trigger |
| `bin/centinel-<role>` shims | `centinel role <name> --prompt "..."` CLI subcommand that hits the local server's HTTP API |
| `bin/centinel` dispatcher (Python) | `centinel` CLI (TypeScript) that proxies to the server. Bootstrap subcommands stay shell-callable for first-run wiring |
| Hermes' OpenAI-compatible endpoint backing `/chat` | The runtime server exposes `/chat` (SSE) backed by an `AgentSession` running as the Editor role |

---

## Target architecture

```
┌──────────────────────────────────────────────────────────────────┐
│  centinel-server  (long-running Node process; pi-agent SDK)      │
│  ────────────────────────────────────────────────────────────    │
│                                                                  │
│   roles/                                                         │
│     editor.ts          — default role, sitemap-builder + persona │
│     investigator.ts    — civic-investigator                      │
│     archivist.ts       — civic-archivist                         │
│     data-reporter.ts   — civic-data-reporter                     │
│     watch-runner.ts    — civic-watch-runner                      │
│                                                                  │
│   each role exports:                                             │
│     async function run(input: RunInput): Promise<RunResult>      │
│                                                                  │
│   ┌────────────────┐    ┌─────────────────┐   ┌──────────────┐   │
│   │ HTTP/SSE API   │    │ Internal cron    │   │ delegate()   │   │
│   │  POST /run/:r  │    │ (node-cron)      │   │ tool         │   │
│   │  POST /chat    │───►│ fires runRole()  │──►│ in-process   │   │
│   │  GET  /runs/:id│    └─────────────────┘   │ subagent     │   │
│   └────────────────┘             │            └──────────────┘   │
│            │                     │                     │         │
│            └─────────┬───────────┴─────────────────────┘         │
│                      ▼                                           │
│              run-logger writes:                                  │
│                 .runtime/runs/<id>.jsonl  (pi events)            │
│                 .runtime/runs/<id>.json   (summary, exit)        │
│                 <wiki>/_runtime/status/board.md  (per protocol)  │
└──────────────────────────────────────────────────────────────────┘
                              ▲                  ▲
                              │ HTTP             │ HTTP
                              │                  │
                ┌─────────────┴─┐   ┌────────────┴───────────┐
                │ Next.js app   │   │ centinel CLI           │
                │ (/chat, /run, │   │ centinel role          │
                │  /status SSE) │   │   investigator -p ...  │
                └───────────────┘   │ centinel cron list     │
                                    │ centinel run tail <id> │
                                    └────────────────────────┘
```

Key properties:

- **One process owns everything.** Server, cron, delegation, chat, run log.
- **Same function for every invocation path.** Cron, HTTP, in-process
  delegate, CLI all call `runRole(role, input)`. No second code path that
  cron uses but you can't reach.
- **Every run is a session.** Pi's `SessionManager` persists each invocation
  to `.runtime/sessions/<role>/<id>.jsonl`. You can replay any past run.
- **Every run is also a structured event log.** The server subscribes to
  `session.subscribe(...)` and writes a `runs/<id>.jsonl` of pi events —
  every tool call, every text delta, every error. The web app `/status`
  streams from this directly.

---

## Role model

```ts
// server/roles/types.ts
import type { Tool } from "@mariozechner/pi-coding-agent";

export interface RoleConfig {
  name: "editor" | "investigator" | "archivist" | "data-reporter" | "watch-runner";
  /** SKILL.md path(s) to load — usually one, editor loads two */
  skills: string[];                 // e.g. ["skills/civic-investigator/SKILL.md"]
  /** Optional system-prompt suffix (editor uses EDITOR_PERSONA.md here) */
  systemPromptOverride?: () => string;
  /** Tools available to this role */
  tools: Tool[];
  /** Model + thinking level (per-role overridable via settings) */
  model?: { provider: string; id: string };
  thinkingLevel?: "off" | "low" | "medium" | "high";
}

export interface RunInput {
  /** The natural-language prompt to send to the role */
  prompt: string;
  /** Optional structured context the caller wants injected */
  context?: Record<string, unknown>;
  /** Caller tag for the run log: "cron", "http", "delegate", "cli" */
  source: "cron" | "http" | "delegate" | "cli";
  /** Optional explicit run id (default: uuid) */
  runId?: string;
}

export interface RunResult {
  runId: string;
  sessionFile: string;
  ok: boolean;
  finalText: string;            // last assistant text response
  toolCalls: Array<{ tool: string; ok: boolean }>;
  durationMs: number;
}
```

Each `roles/<role>.ts` exports:

```ts
export const config: RoleConfig = { ... };
export async function run(input: RunInput): Promise<RunResult>;
```

`run()` is a thin wrapper over `createAgentSession()`:

1. Build a `DefaultResourceLoader` with `skillsOverride` so **only this
   role's skills are loaded** (we don't want the Archivist to inherit the
   Investigator's SKILL.md).
2. Build tools (read/write/edit/bash for filesystem, plus role-specific
   custom tools — e.g. `db_query`, `qmd_search`, `vault_put`).
3. `createAgentSession({ resourceLoader, tools, sessionManager: SessionManager.create(cwd), customTools, ... })`.
4. Subscribe to events → run-logger.
5. `await session.prompt(input.prompt)`.
6. Return `RunResult` with the final assistant text and tool-call summary.

The editor role additionally exposes a `delegate` custom tool whose
implementation is just `await runRole(targetRole, { prompt, source: "delegate" })`.
That is how Lane 1 (sync delegation) is reborn: it's the same `runRole`
that cron and HTTP use, only the caller differs.

---

## Replacing the three Hermes lanes

From `docs/AGENT_INVOCATION.md`, today's lanes are:

| Lane | Today | After migration |
|---|---|---|
| Sync delegation (Editor mid-chat) | `delegate_task(skill=...)` | Editor's `delegate` custom tool → `runRole(target, { source: "delegate" })` in-process |
| Async inbox (durable work) | Editor writes `inbox/<role>/<task>.md`; Hermes cron drains | Editor writes the same file. Server cron schedules `runRole("<role>", { prompt: "drain inbox" })`. **Same protocol, same files** |
| Autonomous cron | `hermes cron create ...` per role | `node-cron` inside the server, table-driven from `doge.config.yaml` |

The filesystem protocol in `RUNTIME_PROTOCOL.md` does not change at all.
What changes is who reads/writes it (the server's role functions, not Hermes
profiles).

---

## Cron, programmatically provoked

The whole point of this rewrite. Each scheduled job is just an entry in a
table:

```ts
// server/cron/schedule.ts
export const defaultSchedule: CronJob[] = [
  { name: "sitemap-lint",       cron: "0 3 * * 1",  role: "editor",        prompt: "weekly sitemap lint" },
  { name: "investigator-tick",  cron: "0 */4 * * *", role: "investigator", prompt: "drain inbox and re-run scheduled investigations" },
  { name: "archivist-tick",     cron: "*/15 * * * *", role: "archivist",    prompt: "drain OCR/vault queue" },
  { name: "data-reporter-tick", cron: "0 */6 * * *", role: "data-reporter", prompt: "import new records, run alias passes" },
  { name: "watch-runner-tick",  cron: "0 */4 * * *", role: "watch-runner", prompt: "scan sitemap diffs + new pages against active watches" },
  { name: "huddle-rollup",      cron: "0 18 * * *", role: "editor",        prompt: "roll up today's huddle" },
  { name: "briefings",          cron: "0 9 * * 1",  role: "editor",        prompt: "weekly briefing" },
  { name: "vault-manifest",     cron: "*/15 * * * *", role: "archivist",   prompt: "refresh vault manifest" },
];
```

Operator overrides in `doge.config.yaml` (`cron_schedule_overrides:`) layer
on top of this. Per-investigation cron entries are inserted by
`POST /cron/investigations/:slug/register`, which writes them to the same
table.

**The cron tick is literally `runRole(role, { prompt, source: "cron" })`.**
Which means:

```bash
# Operator wants to trigger the same job manually and watch it:
centinel cron fire investigator-tick --tail
```

…calls the server, which calls the exact same function the scheduler would
have called at 4:00 AM, and streams the events back to the terminal. No
divergence.

---

## HTTP surface

Minimal, mostly streaming. All under one server (default `localhost:8787`).

```
POST   /run/:role                 → start a role run
       body: { prompt, context?, runId? }
       response: { runId } (and tails SSE if Accept: text/event-stream)

GET    /runs/:runId               → final RunResult JSON
GET    /runs/:runId/events        → SSE stream of pi events (live or replay)
GET    /runs                      → list recent runs (role, source, status)

POST   /chat                      → SSE; editor session, multi-turn via session id
GET    /chat/sessions             → list editor chat sessions
POST   /chat/sessions/:id/abort   → abort a streaming chat

GET    /cron/jobs                 → list scheduled jobs
POST   /cron/jobs/:name/fire      → fire now (returns runId, optionally tail)
POST   /cron/jobs/:name/pause     → pause
POST   /cron/jobs/:name/resume    → resume
POST   /cron/investigations/:slug/register   → register per-investigation job
DELETE /cron/investigations/:slug             → unregister

GET    /status/board              → cached render of <wiki>/_runtime/status/board.md
GET    /status/stream             → SSE: board updates + recent run events

GET    /health                    → liveness + last-cron-tick per job
```

The Next.js app:
- `/chat` page proxies to `POST /chat`.
- `/status` page subscribes to `GET /status/stream`.
- `/investigations/[slug]` server actions hit
  `POST /cron/investigations/:slug/register` (replaces today's shell-out to
  `bin/centinel investigate register`).
- `/setup` Step 5 calls `POST /run/editor` with a "bootstrap sitemap" prompt;
  Step 7 calls `POST /cron/resume-all`.

---

## CLI surface (`bin/centinel`)

Becomes a thin TypeScript binary that talks to the local server. No more
Python dispatcher, no per-role shims (Hermes profiles are gone).

```
centinel server start              # foreground; for systemd / launchd / docker
centinel server status

centinel role <name> --prompt "..."           # one-shot run, streams events
centinel role <name> --interactive            # opens pi interactive mode for that role

centinel cron list
centinel cron fire <job> [--tail]
centinel cron pause <job> | resume <job>
centinel cron resume-all                       # for wizard Step 7

centinel investigate register <slug>          # web wizard helper
centinel run tail <runId>
centinel run list [--role investigator] [--limit 20]
centinel doctor                                # health checks
```

`centinel role investigator --interactive` replaces today's
`bin/centinel-investigator`. It opens an interactive pi `InteractiveMode`
bound to the investigator role's config — same skills, same tools, same
custom delegate-tool, but you can talk to it directly with the full pi TUI.

---

## Observability

This is the headline win. For every invocation:

1. **Session file** — `.runtime/sessions/<role>/<runId>.jsonl` (pi's native
   format; replayable via `pi --session ...`).
2. **Event log** — `.runtime/runs/<runId>.jsonl`, one JSON line per pi event
   (`message_update`, `tool_execution_start`, `tool_execution_end`,
   `turn_end`, etc.).
3. **Summary** — `.runtime/runs/<runId>.json`:
   ```json
   {
     "runId": "...",
     "role": "investigator",
     "source": "cron",
     "cronJob": "investigator-tick",
     "startedAt": "...",
     "endedAt": "...",
     "model": "claude-opus-4-5",
     "ok": true,
     "toolCalls": [...],
     "finalText": "...",
     "filesWritten": ["<wiki>/Findings/raw/...md", ...],
     "errors": []
   }
   ```
4. **`<wiki>/_runtime/status/board.md`** is still updated per the runtime
   protocol — the role itself is responsible for that (no change).

The `/status` page in the web app now has two layers:
- The human-readable board (markdown render, as today).
- A live "what's executing right now" tail driven by the event log, with
  expandable tool-call details. This is the thing we never had with Hermes.

---

## Skills — what pi does and doesn't load

Pi's `DefaultResourceLoader` discovers skills from `~/.pi/agent/skills/`,
`<cwd>/.pi/skills/`, `<cwd>/.agents/skills/`. We don't want pi to load
*all* five skills into *every* role. Two options:

1. **Per-role skill scoping (preferred).** Each `runRole` builds a
   `DefaultResourceLoader` with `skillsOverride` returning only the skills
   that role needs. That gives us deterministic system prompts.
2. **One skill dir, role-specific frontmatter filter.** Less explicit; we
   skip.

The editor role loads `sitemap-builder` plus the editor persona injected as
a system prompt suffix (via `systemPromptOverride`). Each specialist loads
its single skill. Reused skills (`humanized-writing`, `llm-wiki`) load into
the editor role for the briefings + lint passes.

The skill SKILL.md files in `skills/` may need a one-time pass to remove
Hermes-specific language ("You run inside the `investigator` Hermes
profile"). That's a doc edit, not a behavior change.

---

## Bootstrap changes

The current `bootstrap` script does ten things; here's the diff:

| Step | Today | After migration |
|---|---|---|
| 1. Dep check | `hermes`, `python3`, `docker` | `node` ≥ 20, `pnpm`, `docker`. Drop `hermes` and `python3`. |
| 2. `doge.config.yaml` | unchanged | unchanged |
| 3. `.env` | `HERMES_API_URL`, `HERMES_API_KEY` | replaced by pi-agent provider creds: `ANTHROPIC_API_KEY` (or `OPENAI_API_KEY`, etc.) and per-role model overrides in `doge.config.yaml`. The web app's `HERMES_API_URL` config goes away — `/chat` is served by the runtime server. |
| 4. Wiki tree | unchanged | unchanged |
| 5. SQLite init | unchanged | unchanged |
| 6. Symlink skills into `~/.hermes/skills/centinel/` | **dropped.** Skills are loaded by path from `<repo>/skills/`. |
| 7. `setup-profiles` (create Hermes profiles) | **dropped.** No profiles. |
| 8. `setup-cron` (register paused Hermes cron) | becomes `centinel cron seed --paused` — writes the cron table with all jobs in `paused: true`. |
| 9. Bring up Docker | now brings up: web (Next.js) + Datasette + **centinel-server**. |
| 10. Print URL | unchanged |

Setup wizard Step 7 (`Activate cron`) now hits
`POST /cron/resume-all` on the server instead of shelling out.

---

## Editor `/chat` — the trickiest piece

Today: Next.js `/chat` route calls Hermes' OpenAI-compatible endpoint with
the editor system prompt. Streaming text comes back via SSE.

After: `/chat` POSTs to `centinel-server` `/chat`. The server holds one
`AgentSession` per chat thread (keyed by `chatSessionId`), with the editor
role config (default profile + sitemap-builder + EDITOR_PERSONA system
prompt + tools incl. `delegate`, `qmd_search`, `db_query`, file tools).
SSE streams pi events back. The chat thread persists as a pi session file
under `.runtime/sessions/editor/<chatSessionId>.jsonl`, so:

- Reloading the page resumes mid-thread.
- Branching/forking the chat is free (pi supports it).
- Every chat is also a fully-replayable session you can debug.

This is also where `delegate` becomes interesting — when the operator asks
"look into ACME's relationships" and the editor decides it needs a real
investigation, calling `delegate("investigator", "...")` produces an
in-process subagent run whose events are visible in `/status` live.

---

## What we lose (or have to rebuild)

Honest inventory of things Hermes did for us that we'll need to replicate
or accept losing:

- **Hermes' built-in `qmd-search` and `db_query` tools.** Per
  `EDITOR_ANSWER_SOURCES.md`, the editor and investigator both depend on
  these. We need to implement them as pi `defineTool()` calls. Both are
  thin: `qmd-search` shells out to an existing qmd binary; `db_query` opens
  the SQLite DB. Both are local. **This is required and we should scope it
  separately.**
- **Hermes' file/web/terminal toolsets** for the skills that reference
  them. Pi ships `read`, `write`, `edit`, `bash`. Web fetches will need a
  `web_fetch` custom tool (a few hundred lines, off-the-shelf).
- **Per-profile credential pools.** Pi has one `AuthStorage` per server.
  This is fine — we never actually needed per-profile creds. Per-role
  model selection still works via `RoleConfig.model`.
- **Hermes' memory/session ergonomics across profiles.** We were
  intentionally not using cross-profile memory (per
  `AGENT_INVOCATION.md` § Profiles), so nothing lost here.

---

## Phased migration

Five phases. Each is independently shippable; the system keeps working
through all of them.

### Phase 0 — Scaffold (no behavior change)

- [ ] Add `server/` workspace (TypeScript, pnpm).
- [ ] `pnpm add @mariozechner/pi-coding-agent` and pick the model.
- [ ] Stand up `centinel-server` binary that does nothing but `GET /health`
      and listens on a configured port.
- [ ] Add `centinel` CLI (TypeScript) with one working command: `centinel
      server status` that hits `/health`.
- [ ] CI builds both alongside the existing `app/` Next.js build.

**Exit criteria:** `centinel server start` runs; `centinel server status`
prints `ok`. Hermes is still doing all the actual work.

### Phase 1 — One role behind a feature flag (Investigator)

- [ ] Implement `roles/investigator.ts` (config + `run()`).
- [ ] Implement the minimum custom tools investigator needs:
      `web_fetch`, `qmd_search`, `db_query`, `vault_put`. (Or stub the
      ones we can defer.)
- [ ] Wire `POST /run/investigator` + SSE event stream.
- [ ] Wire `centinel role investigator --prompt "..."` to that endpoint.
- [ ] Implement run-logger writing `.runtime/runs/<id>.{jsonl,json}`.
- [ ] **Side-by-side test:** trigger the investigator with the same prompt
      via `bin/centinel-investigator` (Hermes) and via `centinel role
      investigator` (pi). Diff results.

**Exit criteria:** an operator can run an investigation through pi and the
output (wiki writes, findings drafts, inbox responses) matches the Hermes
output within tolerance. Hermes still runs the cron.

### Phase 2 — Internal cron + all five roles

- [ ] Implement remaining four roles (`editor`, `archivist`,
      `data-reporter`, `watch-runner`).
- [ ] Implement the cron table + `node-cron` scheduler.
- [ ] Implement `POST /cron/jobs/:name/fire`, `pause`, `resume`,
      `resume-all`.
- [ ] Operator overrides from `doge.config.yaml` wired in.
- [ ] Pause all Hermes cron jobs; resume the pi cron table.

**Exit criteria:** scheduled work is done by pi cron, not Hermes. Operator
can `centinel cron fire investigator-tick --tail` and watch a run live.
The wiki's `_runtime/` state (inbox/outbox/status) still passes the
`RUNTIME_PROTOCOL.md` acceptance criteria.

### Phase 3 — Editor and `/chat` ✅

- [x] Editor role with sitemap-builder + persona system prompt (appended
      to pi's default system prompt; mtime-cached).
- [x] `delegate` custom tool on the editor role. Targets the four
      specialists; routes through `runRole(..., { source: "delegate" })`
      so every delegation is a normal run with full `.runtime/runs/<id>`
      artifacts. Global concurrency cap of 2 (per-session refinement
      deferred to Phase 4).
- [x] `POST /chat` (SSE) backed by per-thread `AgentSession`s,
      persisted to `.runtime/sessions/editor-chat/<id>.jsonl`.
- [x] Next.js `/chat` route is now a thin proxy to `centinel-server`,
      translating SSE → the existing text-delta client contract. A new
      `CENTINEL_SERVER_URL` env var configures it.
- [x] `HERMES_API_URL` / `HERMES_API_KEY` no longer wired into the chat
      route (the env-var getters remain in `app/src/lib/config.ts` as
      dead code; deleted in Phase 4).

**Exit criteria:** the operator chats with the editor through pi, and
mid-chat delegations to the investigator/archivist show up in `/runs`
in real time as nested runs.

### Phase 4 — Decommission Hermes ✅ done (2026-05-21)

- [x] Update `docs/AGENT_INVOCATION.md` (and friends) to point at this doc.
- [x] Delete `bin/centinel-<role>` shims; keep `centinel role <r>
      --interactive` as the new entry point.
- [x] Delete `lib/cli.py` (Python dispatcher); the TypeScript `centinel`
      CLI fully replaces it.
- [x] `bootstrap` no longer creates Hermes profiles or registers Hermes
      cron jobs.
- [x] Remove `hermes` from the dep check.
- [x] One-time pass over `skills/*/SKILL.md` to remove Hermes-specific
      phrasing.

**Exit criteria (met):** clean install on a fresh box doesn't require Hermes.
Bootstrap takes the operator from `git clone` → working centinel-server →
sitemap → first investigation, all through the new stack.

---

## Open questions

1. **One model or per-role models?** Cheap roles (archivist OCR-trigger,
   watch-runner) might want a smaller/cheaper model than editor +
   investigator. Default proposal: one model for all in v0, per-role
   override knob in `doge.config.yaml` in v1.
2. **Where does the runtime server live in dev?** Docker compose alongside
   web + datasette (matches current bootstrap step 9), or `pnpm dev`
   alongside the Next.js app? I'd suggest both: docker for prod, `pnpm dev`
   for hacking.
3. **Backpressure on `delegate`.** If the editor delegates 5 things in one
   turn, do we serialize, run all 5 in parallel, or cap concurrency?
   Proposal: cap at 2 in-process delegates per editor session, queue the
   rest.
4. **Session retention.** Pi sessions are JSONL on disk and grow forever.
   Need a `.runtime/sessions/` rotation policy. Default: keep 90 days of
   non-chat sessions, infinite chat sessions, monthly compaction.
5. **Per-investigation cron entries** today are written by
   `bin/centinel investigate register <slug>`. Where does the cron table
   live so a server restart preserves them? Proposal: `.runtime/cron.json`
   (committed to the wiki? or to the repo? leaning wiki since it's per-
   city state).

---

## Decisions (locked 2026-05-21)

1. **Server location:** in this repo under `server/`. One repo, one place to
   read. CI builds `server/` alongside `app/`.
2. **Python dispatcher:** dropped entirely in Phase 4. The TypeScript
   `centinel` CLI fully replaces `lib/cli.py` and the `bin/centinel-<role>`
   shims. No parallel paths.
3. **Model selection:** one default model for every role in v0. The
   `RoleConfig.model` field exists but is unused until a per-role override
   is requested. Configured via a single `CENTINEL_MODEL` env var (or
   `model:` key in `doge.config.yaml`), defaulting to whatever the operator
   has API keys for.

## Status (2026-05-21)

Phases 0–3 complete. The runtime server owns every role, the cron, and
`/chat`. Hermes is still installed but no longer in any critical path the
Next.js app uses. Phase 4 is decommissioning + cleanup.
