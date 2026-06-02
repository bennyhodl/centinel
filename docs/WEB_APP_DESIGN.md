---
title: Centinel — Web App Design (LOCKED)
status: 🔒 Locked v1
created: 2026-04-26
parent: README.md
---

# Centinel Web App Design

The viewer + control panel. Locked 2026-04-26 (plan checkpoint v8).

## Core principle

> The web app NEVER originates state. It reads files + DB. Every "action" is a small, well-formed file write that an agent already knows how to react to.

This is the rule that keeps maintenance burden near zero forever:
- No CMS. Operator edits content in Obsidian or git.
- No custom backend logic for content. Markdown render + Datasette embed.
- No write code paths the agents don't already react to.
- Every web app "action" = a file write that triggers an existing agent reaction.

| Action | What the web app does | What an agent does next |
|---|---|---|
| "Start investigation" | Writes `Investigations/<slug>.md` + registers cron | Investigator picks up next tick |
| "Approve entity merge" | Edits frontmatter `status: resolved` in operator-queue file | Data Reporter respects it next run |
| "Publish finding" | `mv draft/foo.md published/foo.md` | Web app re-renders feed |
| "Pause watch" | Edits frontmatter `status: paused` in watch YAML | Watch Runner skips it next run |
| "Tune watch" | Edits watch YAML | Watch Runner uses new config next run |
| Chat with Editor | Request to centinel-server's `/chat` endpoint (runs the `editor` role) | Editor reads wiki/DB, optionally delegates to specialists via the `delegate` tool |

## Auth

**Basic auth on everything in v0.1.** Single shared password in env var. Until the project warrants a real auth design, this is sufficient.

No public/operator distinction. Everyone with the password gets the full app: viewing, chat, state-changing actions. When auth is redesigned (v0.2+), we'll split into public-read / operator-write tiers.

## Distribution shape

**Single-machine self-host in v0.1.** Operators clone the repo; centinel-server runs locally and owns the cron loop. Standalone bundled-everything distribution is a stretch goal once there's evidence non-technical operators want it.

```
git clone github.com/lygos/centinel-template my-city-doge
cd my-city-doge
./bootstrap                    # builds centinel-server, seeds paused cron jobs
./bin/centinel-server &        # run the role runtime
pnpm --filter centinel dev     # web app
# open localhost:3000 → setup wizard → enter city.gov → kick off
```

## Setup wizard (first-run experience)

The wizard is the only thing rendered until `<wiki>/_runtime/setup-state.json` reports `complete`. All other routes redirect to `/setup` until then.

```
Step 1: City.gov URL                    → tampa.gov
Step 2: Project name + branding         → "Centinel" + logo (defaults fine)
Step 3: Watch presets                   → checkboxes: errant-spending, corruption, policy-drift
Step 4: Notification channel (optional) → Discord/Telegram for briefings only
Step 5: Confirm → "Start Bootstrap"
        ↓
        Server action shells out to centinel-server:
        ./bin/centinel role editor -q "bootstrap mode: build sitemap for tampa.gov, write to $WIKI/Sitemap/"
        Tails the log to the browser via Server-Sent Events
        Live progress: pages crawled, classified, descriptions written
        ETA 30–90 minutes for a city like Tampa
        ↓
Step 6: Sitemap review
        Operator skims /sitemap, marks bulk-categories needs_review → active
        ↓
Step 7: Activate cron
        Web app calls centinel-server: flip cron jobs from paused → active
        ↓
Done. /sitemap is the home. Operator launches first investigation from /investigations.
```

After Step 7, the web app is purely viewer + control panel. Browser can close. Cron runs forever.

## Routes

```
/setup              first-run wizard (gates everything until complete)
/                   redirects to /sitemap once setup complete
/sitemap            sitemap explorer (the dashboard)
/sitemap/[...path]  drill into sitemap subtree
/investigations     list all investigations + "+ New" form
/investigations/[slug]
/entities           index by type (contractors, people, orgs, projects, etc.)
/entities/[type]/[slug]   entity page rendered from wiki markdown
/findings           feed: published narratives + raw findings, sourced
/findings/[slug]    single finding + sources
/findings/draft     drafts awaiting review (no public link, but accessible)
/operator-queue     drainable items for the operator
/status             status board + 7-day activity feed (live via SSE)
/briefings          published weekly digests
/db                 embedded Datasette (read-only, sanitized public view)
/vault/*            stable URLs for vault files (verification)
/chat               Editor chat (the only chat surface)
```

All routes behind basic auth in v0.1. No anonymous access.

## Tech stack (low-burden defaults)

- **Next.js 15 App Router** — uses the existing `nextjs-drizzle-betterauth-scaffold` skill as starting point (strip auth complexity for now)
- **Markdown render**: `react-markdown` + `remark-gfm` + `gray-matter` for frontmatter
- **Wikilink resolution**: small util maps `[[Contractors/acme]]` → `/entities/contractor/acme`
- **DB reads**: `better-sqlite3` direct, read-only mode, against `<wiki>/_data/tampa.db`
- **DB explorer**: embed [Datasette](https://datasette.io) at `/db` — public sanitized view via SQL views (`CREATE VIEW public_transactions AS SELECT ... WHERE confidence > 0.7`)
- **Search**: `qmd` BM25 over the wiki via HTTP endpoint
- **Status page rendering**: `chokidar` watches `status/board.md`; web app pushes to client via Server-Sent Events
- **Wizard state**: `<wiki>/_runtime/setup-state.json`. Middleware redirects to `/setup` until `complete`.
- **centinel-server integration**: server actions shell out to `./bin/centinel` (the dispatcher) which writes files / toggles cron / triggers roles.
- **Chat**: Vercel AI SDK / `@ai-sdk/anthropic` pointed at centinel-server's `/chat` endpoint, which loads the editor role's system prompt + `sitemap-builder` skill.

No ORM, no GraphQL, no custom auth provider, no CMS. ~5 npm dependencies that matter.

## /chat — the Editor surface

One chat. One persona. Talking directly to the head of the investigative unit.

See [`EDITOR_PERSONA.md`](./EDITOR_PERSONA.md) for the persona prompt, tool spec, and citation enforcement.

The Editor reads everything (wiki, DB, vault, findings, status, drafts, operator queue) and can write drafts, register investigations, tune watches, resolve queue items, promote findings. The Editor cites sources for every claim — vault path, DB methodology query, or wiki page reference. No source → "I don't have a source for that."

The chat is THE primary interface for serious work. The other routes are passive viewing; chat is where steering happens.

## /db — the database explorer

Embedded Datasette at `/db`. Read-only. Serves a **sanitized public view** via SQL views — confidence < 0.7 rows are excluded from the public view; raw entity hints (pre-reconciliation) are excluded; in-progress investigation transactions are excluded if the investigation has `confidential: true`.

The full DB lives at `<wiki>/_data/tampa.db`. Datasette public view loads `tampa.db` with a `--immutable` flag plus loads `<wiki>/_data/public-views.sql` defining what's visible.

This means anyone with the basic auth password can browse the DB, write SQL queries, export to CSV. Datasette gives this for free — zero custom code on our side.

## /status — the public-facing transparency layer

Renders `<wiki>/_runtime/status/board.md` live. SSE pushes updates within seconds of any agent's edit.

Plus a 7-day activity feed pulled from `<wiki>/_runtime/outbox/` — sender, recipient, type, summary, with vault paths and DB row IDs linkified.

Per RUNTIME_PROTOCOL.md, this is the project's "show your work" surface. Investigations marked `confidential: true` (e.g., active corruption probe with right-of-reply still pending) get suppressed from this page until publication; everything else surfaces by default.

## /vault/[...path] — verification anchors

Every vaulted document is served at a stable URL: `/vault/pdfs/2026-04-26-a1b2c3d4-fy2025-parks-awards.pdf`. Citations across the app link here. External readers (other journalists, citizens, opposing counsel) verify claims by clicking through to the original artifact.

The vault is read-only over HTTP. The web app server reads files from `<wiki>/Vault/` and serves with appropriate Content-Type. No caching layer; vault entries are immutable so browser caching with strong ETags works fine.

## /findings — the editorial output

Two stacks:

### `Findings/raw/` (auto-published)
Hard data points generated by Watch Runner or Investigator. Already cited. Auto-published, surface immediately.

### `Findings/published/` (Editor-drafted, human-promoted)
Narrative findings — connections, patterns, synthesized stories. Drafted by the Editor (chat persona) based on specialist agent output. Promoted to published only after human Reviewer (operator) confirms.

Both render at `/findings`. Filter UI lets viewers see raw-only, published-only, or both.

`/findings/draft` shows in-progress drafts. Behind basic auth (everyone sees it in v0.1) but visually distinct — banner reads "DRAFT — not yet reviewed by editor or counsel. Do not cite."

## Source material — mandatory across the system

Per Ben (2026-04-26): "we need to have source material for everything." Locked rule:

- Every PDF → vaulted, hashed, OCR'd, summarized
- Every Excel/CSV → vaulted, parsed, summarized
- Every news article → vaulted as HTML capture + screenshot
- Every web page → vaulted as HTML capture + screenshot
- Every transcript → vaulted with timestamps
- Every database row → has `source_vault_path` or `source_url` non-null

The vault is the evidence base. The wiki, DB, findings, and Editor's chat answers all cite back to it. Without a citation, a claim is not made.

## Setup wizard implementation detail

The shell-out approach: the `/setup` server action invokes `bin/centinel bootstrap-sitemap <domain>` (which under the hood runs `./bin/centinel role editor -q "<bootstrap prompt>"`) and streams the log file to the browser via the SSE endpoint at `/api/setup/bootstrap-log`. See `docs/PI_MIGRATION_PLAN.md` for the full invocation paradigm.

If the web process crashes mid-bootstrap, the bootstrap continues (it's a centinel-server role run, not a web request). On reconnect, web app reads the latest sitemap state and resumes its progress display from there.

If we hit reliability problems with shell-out, fall back to trigger-file pattern: web app writes `<wiki>/_runtime/triggers/bootstrap.json`, a tiny watcher daemon picks it up. Don't pre-build this — only switch if shell-out bites us.

## Cron registration

Per RUNTIME_PROTOCOL.md, cron is dynamic — each investigation's `schedule:` field becomes its own centinel-server cron entry.

When operator launches an investigation via web app or chat:
1. Server action writes `<wiki>/Investigations/<slug>.md` with frontmatter
2. Server action calls `./bin/centinel investigate register <slug> --cron "<sched>"`, which parses the investigation YAML's `schedule:` field and adds an entry to `.runtime/cron.json` targeting the `investigator` role.
3. Cron entry runs at next scheduled tick. To temporarily disable: `./bin/centinel cron pause investigator-tick` (or `./bin/centinel cron pause-all` for emergency stop).

When operator pauses an investigation: edits frontmatter `status: paused`. Cron entry remains registered but next run sees `status: paused` and skips. (Lighter weight than unregistering cron.)

## Acceptance criteria

- ✅ Fresh fork → `./setup` → `docker compose up` → wizard renders at localhost:3000 with no manual config
- ✅ Wizard completes in <90 minutes for tampa.gov
- ✅ After wizard, browser can close — cron continues, agents run on schedule
- ✅ Every "action" in the web app maps to exactly one file write the agents already react to
- ✅ Closing/reopening browser doesn't lose state — wizard resumes mid-flight
- ✅ Status page updates within 5 seconds of an agent edit
- ✅ Datasette public view excludes low-confidence rows
- ✅ Every finding (raw or published) links to at least one vault path
- ✅ Editor chat can answer "what do we know about ACME Construction" with citations to vault entries
- ✅ Operator can launch a new investigation from chat without leaving the chat
- ✅ Web app process restart doesn't break ongoing investigations or sessions

## Open questions (for later, non-blocking)

1. Mobile UX for the chat? Most steering will probably happen mobile (operator on the go). Plan for mobile-first chat UI. Other routes can be desktop-prioritized.
2. Multi-investigation chat context — when operator chats with Editor, should Editor remember context across sessions? Default: stateless per chat session (centinel-server hands the editor role a fresh thread each time), but offer "pin investigation" mode that scopes Editor to one investigation's context.
3. Real-time collaboration — two operators chatting with Editor simultaneously. centinel-server role runs are per-request; both will get independent Editor runs. Acceptable for v0.1.
4. Auth upgrade path — when v0.1 basic auth is no longer enough, prefer better-auth (already in scaffold skill) over rolling our own.
