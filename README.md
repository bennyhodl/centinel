# Centinel

Civic transparency platform for tracking city government — sitemaps, investigations, vaulted documents, watches, and findings. Built on [`@mariozechner/pi-coding-agent`](https://www.npmjs.com/package/@mariozechner/pi-coding-agent) so any city's accountability operation can fork it.

> The web app NEVER originates state. It reads files + DB. Every "action" is a small, well-formed file write that an agent already knows how to react to.

**Status:** v0.1 — pi-agent migration phases 0–4 complete. Web app shell complete; agent skills specced; tool implementations are mostly stubs. See [`docs/PI_MIGRATION_PLAN.md`](docs/PI_MIGRATION_PLAN.md) and [`docs/PLAN.md`](docs/PLAN.md).

## Repo layout

```
centinel/
├── app/                  # Next.js 16 viewer + control panel
├── server/               # @centinel/server — pi-agent runtime (cron, /run, /chat)
├── bin/                  # `centinel` + `centinel-server` shims
├── bootstrap             # one-time installer (idempotent)
├── docs/                 # locked design specs (the source of truth)
│   ├── PLAN.md
│   ├── PI_MIGRATION_PLAN.md     # current architecture
│   ├── PHASE_4_PLAN.md
│   ├── WEB_APP_DESIGN.md
│   ├── RUNTIME_PROTOCOL.md
│   ├── EDITOR_PERSONA.md
│   ├── AGENT_ROSTER.md
│   ├── ORG_STRUCTURE_AND_WORKFLOW.md
│   ├── REPO_AND_DISTRIBUTION.md
│   ├── SCRAPER_AND_EXTRACTORS.md
│   ├── INSTALLATION.md
│   ├── AGENT_INVOCATION.md      # SUPERSEDED — kept for historical context
│   └── EDITOR_ANSWER_SOURCES.md
└── skills/               # pi-agent skill specs loaded into roles
    ├── sitemap-builder/
    ├── civic-investigator/
    ├── civic-archivist/
    ├── civic-data-reporter/
    └── civic-watch-runner/
```

## The agent stack

Every agent runs as a **role** inside centinel-server — a single Node process built on pi-coding-agent. Roles are scoped: each one loads only its own skill and tools. Coordination across roles is via the wiki filesystem and the editor's `delegate` tool — no shared memory, no message broker.

| Role | Skill | Purpose |
|---|---|---|
| **editor** | `sitemap-builder` + Editor persona | fronts `/chat`, owns the sitemap, dispatches via `delegate` |
| **investigator** | `civic-investigator` | depth-crawl from seeds |
| **archivist** | `civic-archivist` | document intake, OCR, vault |
| **data-reporter** | `civic-data-reporter` | entity DB, queries |
| **watch-runner** | `civic-watch-runner` | continuous matchers over diffs |

Humans wear all editorial/legal/source-protection hats — agents only do ingest/structure/present. See [`docs/AGENT_ROSTER.md`](docs/AGENT_ROSTER.md).

Roles are reachable three ways:

- `centinel role <name> -p "..."` — one-shot from the operator's shell (streams events via the local server)
- `centinel role <name> --interactive` — pi's full TUI scoped to that role's skill + tools
- `delegate(target: "<name>", prompt: "...")` — the editor calls specialists in-process; each delegation appears live on `/status`

Cron-driven runs use the same code path as `delegate` and CLI. See [`docs/PI_MIGRATION_PLAN.md`](docs/PI_MIGRATION_PLAN.md) and [`docs/EDITOR_ANSWER_SOURCES.md`](docs/EDITOR_ANSWER_SOURCES.md).

## The web app

Next.js 16 App Router. ~18 routes. All gated by basic auth in v0.1.

- `/sitemap` — labeled map of the city's `.gov` surface (the home view)
- `/setup` — 7-step wizard, gates everything until complete
- `/chat` — Editor persona, streaming, mobile-first (proxies to centinel-server `/chat`)
- `/investigations`, `/findings`, `/entities`, `/briefings`
- `/operator-queue` — drainable items
- `/status` — live SSE board + 7-day activity feed
- `/db` — embedded Datasette
- `/vault/*` — stable URLs for verification anchors

See [`docs/WEB_APP_DESIGN.md`](docs/WEB_APP_DESIGN.md) for the full spec.

## Stack

- **Next.js 16** (App Router, standalone output)
- **Tailwind v4** (CSS-first via `@theme` in `src/app/globals.css`)
- **TypeScript**, **pnpm** (workspace; `app/`, `server/`)
- `@mariozechner/pi-coding-agent` — the agent runtime
- `react-markdown` + `remark-gfm` + `gray-matter` — wiki rendering
- `better-sqlite3` — read-only access to `<wiki>/_data/<city>.db`
- `zod` — schema validation
- `croner` — cron scheduling inside centinel-server

No ORM, no Postgres, no custom auth provider.

## Environment variables

| Var | Purpose | Default |
|---|---|---|
| `CENTINEL_PASSWORD` | shared password for basic-auth gate | _required_ |
| `CENTINEL_WIKI_PATH` | path to the operator's wiki root | from `doge.config.yaml` |
| `CENTINEL_EDITOR_PERSONA_PATH` | path to Editor persona markdown | `<repo>/docs/EDITOR_PERSONA.md` |
| `CENTINEL_HOST` | centinel-server bind/connect host | `127.0.0.1` |
| `CENTINEL_PORT` | centinel-server bind/connect port | `8787` |
| `CENTINEL_SERVER_URL` | full base URL for the Next app's `/chat` proxy | derived from host/port |
| `CENTINEL_RUNTIME_DIR` | where `.runtime/{runs,sessions}/cron.json` live | `<repo>/.runtime` |
| `ANTHROPIC_API_KEY` | model provider key (pi-agent default) | one of these required |
| `OPENAI_API_KEY` | alternative provider | |
| `DATASETTE_URL` | optional Datasette base URL | `http://localhost:8001` |

Copy `.env.example` → `.env` and fill in.

## Develop

**New here?** See [`docs/INSTALLATION.md`](docs/INSTALLATION.md) for the full fresh-clone walkthrough. Quick version:

```bash
./bootstrap                            # idempotent installer (deps, wiki tree, cron seed, doctor)
./bin/centinel-server                  # start the runtime server (in one terminal)
pnpm --filter centinel dev             # start the web app (in another terminal)
```

Open http://localhost:3000. The browser prompts for basic auth — user can be blank, password = `CENTINEL_PASSWORD`.

Health check at any time: `./bin/centinel doctor`.

## Build

```bash
pnpm build           # builds both centinel app and @centinel/server
```

The app's Next build is standalone (`output: 'standalone'`), ready for Coolify or any Docker host.

## License

MIT (forks-encouraged). See `LICENSE`.
