# Centinel

Civic transparency platform for tracking city government — sitemaps, investigations, vaulted documents, watches, and findings. Built as a self-hosted **Hermes plugin** so any city's accountability operation can fork it.

> The web app NEVER originates state. It reads files + DB. Every "action" is a small, well-formed file write that an agent already knows how to react to.

**Status:** v0.1 — web app shell complete, agent skills specced and partially implemented, bootstrap not yet wired to live shell-out.

## Repo layout

```
centinel/
├── app/                  # Next.js 16 viewer + control panel
├── docs/                 # locked design specs (the source of truth)
│   ├── PLAN.md           # top-level plan & checkpoints
│   ├── WEB_APP_DESIGN.md # the web app spec
│   ├── RUNTIME_PROTOCOL.md
│   ├── EDITOR_PERSONA.md
│   ├── AGENT_ROSTER.md
│   ├── ORG_STRUCTURE_AND_WORKFLOW.md  (Spotlight model reference)
│   ├── REPO_AND_DISTRIBUTION.md
│   ├── SCRAPER_AND_EXTRACTORS.md
│   ├── INSTALLATION.md  (fresh-clone → first investigation, with current-state honesty)
│   ├── AGENT_INVOCATION.md  (how agents are actually launched)
│   └── EDITOR_ANSWER_SOURCES.md  (DB/vault/QMD priority for Editor answers)
└── skills/               # Hermes skill specs for the agent stack
    ├── sitemap-builder.md       (Cartographer)
    ├── civic-investigator.md    (Investigator)
    ├── civic-archivist.md       (Archivist)
    ├── civic-data-reporter.md   (Data Reporter)
    └── civic-watch-runner.md    (Watch Runner)
```

## The agent stack

Each non-Editor agent is a separate **Hermes profile** (`~/.hermes/profiles/<name>/`) with its own config, skills, memory, and cron. They coordinate via the wiki filesystem only — no shared memory, no message broker.

| Profile | Skills | Role |
|---|---|---|
| **default** (main agent) | `sitemap-builder` + Editor persona | **Editor + Cartographer** — fronts `/chat` API, owns the sitemap |
| `investigator` | `civic-investigator` | depth-crawl from seeds |
| `archivist` | `civic-archivist` | document intake, OCR, vault |
| `data-reporter` | `civic-data-reporter` | entity DB, queries |
| `watch-runner` | `civic-watch-runner` | continuous matchers over diffs |

Plus reused skills running in the default profile: `humanized-writing` (briefings), `llm-wiki` (vault lint).

Humans wear all editorial/legal/source-protection hats — agents only do ingest/structure/present. See [`docs/AGENT_ROSTER.md`](docs/AGENT_ROSTER.md).

Each role launches via a `bin/centinel-<role>` wrapper that resolves to `hermes --profile <role>` (operator terminal access only — not in the runtime loop). The Editor reaches specialists via `delegate_task` (sync) or the filesystem inbox (async). There is no `hermes session run X` primitive — sessions are composed from `(profile + skills + prompt)`. See [`docs/AGENT_INVOCATION.md`](docs/AGENT_INVOCATION.md) and [`docs/EDITOR_ANSWER_SOURCES.md`](docs/EDITOR_ANSWER_SOURCES.md).

## The web app

Next.js 16 App Router. ~18 routes. All gated by basic auth in v0.1.

- `/sitemap` — labeled map of the city's `.gov` surface (the home view)
- `/setup` — 7-step wizard, gates everything until complete
- `/chat` — Editor persona, streaming, mobile-first
- `/investigations`, `/findings`, `/entities`, `/briefings`
- `/operator-queue` — drainable items
- `/status` — live SSE board + 7-day activity feed
- `/db` — embedded Datasette
- `/vault/*` — stable URLs for verification anchors

See [`docs/WEB_APP_DESIGN.md`](docs/WEB_APP_DESIGN.md) for the full spec.

## Stack

- **Next.js 16** (App Router, standalone output)
- **Tailwind v4** (CSS-first via `@theme` in `src/app/globals.css`)
- **TypeScript**, **pnpm**
- `react-markdown` + `remark-gfm` + `gray-matter` — wiki rendering
- `better-sqlite3` — read-only access to `<wiki>/_data/tampa.db`
- `openai` — chat against Hermes' OpenAI-compatible endpoint
- `zod` — schema validation

No ORM, no Postgres, no custom auth provider.

## Environment variables

| Var | Purpose | Default |
|---|---|---|
| `CENTINEL_PASSWORD` | shared password for basic-auth gate | _required_ |
| `CENTINEL_WIKI_PATH` | path to the operator's wiki root | `~/wiki/Tampa` |
| `CENTINEL_EDITOR_PERSONA_PATH` | path to Editor persona markdown | `~/plans/centinel/EDITOR_PERSONA.md` |
| `HERMES_API_URL` | OpenAI-compatible base URL for `/chat` | _required_ |
| `HERMES_API_KEY` | API key for Hermes endpoint | _required_ |
| `DATASETTE_URL` | browser-facing Datasette URL (iframe + links). Default same-origin proxy. | `/datasette/` |
| `DATASETTE_INTERNAL_URL` | server-side probe + proxy upstream | `http://datasette:8001` (compose) |

Copy `.env.example` → `.env.local` and fill in.

## Develop

**New here?** See [`docs/INSTALLATION.md`](docs/INSTALLATION.md) for the full fresh-clone walkthrough — what's wired today, what's still spec, and how to start your first investigation. Quick version:

```bash
./bootstrap                  # idempotent installer (profiles, cron, wiki tree)
cd app && pnpm install && pnpm dev
```

Open http://localhost:3000. The browser prompts for basic auth — user can be blank, password = `CENTINEL_PASSWORD`.

## Build

```bash
cd app
pnpm run build
pnpm start
```

The build is Dockerized output (`output: 'standalone'`), ready for Coolify or any Docker host.

## Status of v0.1

- ✅ Web app shell — all routes, dark theme, empty states
- ✅ Setup wizard — 7-step, persisted state, redirect gate
- ✅ Sitemap viewer — schema-validated against `sitemap-builder` output
- ✅ Wiki readers — investigations, findings, entities, briefings, queue
- ✅ Editor chat — streaming OpenAI client, system prompt from `EDITOR_PERSONA.md`
- ✅ Live status board — SSE + 7-day outbox feed
- ✅ Vault file streaming — traversal-guarded, immutable cache
- 🚧 Setup bootstrap shell-out → `sitemap-builder` skill (stubbed)
- 🚧 Cron registration on activate (stubbed)
- 🚧 Agent skills (specs locked in `skills/`, implementations TBD)
- 🚧 `bin/centinel-*` profile wrappers (one per role; replaces the old `centinel-cli` idea — see `docs/AGENT_INVOCATION.md`)

## License

MIT (forks-encouraged). See `LICENSE`.
