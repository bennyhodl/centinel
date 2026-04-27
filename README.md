# Tampa-DOGE

Viewer + control panel for the Tampa-DOGE civic transparency project. **Hermes plugin (v0.1)** — see [`/home/ben/plans/tampa-doge/WEB_APP_DESIGN.md`](../../plans/tampa-doge/WEB_APP_DESIGN.md) for the locked spec.

> The web app NEVER originates state. It reads files + DB. Every "action" is a small, well-formed file write that an agent already knows how to react to.

## Stack

- **Next.js 16** (App Router, standalone output)
- **Tailwind v4** (CSS-first via `@theme` in `src/app/globals.css`)
- **TypeScript**, **pnpm**
- `react-markdown` + `remark-gfm` + `gray-matter` — wiki rendering
- `better-sqlite3` — read-only access to `<wiki>/_data/tampa.db`
- `openai` — chat against Hermes' OpenAI-compatible endpoint
- `zod` — schema validation

No ORM, no Postgres, no custom auth provider. v0.1 auth = single-password basic-auth middleware on every route.

## Environment variables

| Var | Purpose | Default |
|---|---|---|
| `TAMPA_DOGE_PASSWORD` | shared password for basic-auth gate | _required_ |
| `TAMPA_DOGE_WIKI_PATH` | path to the operator's wiki root | `~/wiki/Tampa` |
| `HERMES_API_URL` | OpenAI-compatible base URL for `/chat` | _required_ |
| `HERMES_API_KEY` | API key for Hermes endpoint | _required_ |

Copy `.env.example` → `.env.local` and fill in.

## Develop

```bash
pnpm install
pnpm approve-builds   # once, to allow better-sqlite3 to compile
pnpm dev
```

Open http://localhost:3000. The browser will prompt for basic auth — user can be blank, password = `TAMPA_DOGE_PASSWORD`.

## Build

```bash
pnpm run build
```

(See `next.config.ts` — `serverExternalPackages: ["better-sqlite3"]` is required so the native module isn't bundled.)

## Layout

```
src/
  app/
    layout.tsx        top nav + shell
    page.tsx          / → /sitemap (or /setup if not bootstrapped)
    setup/            wizard (placeholder)
    sitemap/          sitemap explorer + drill-in
    investigations/   list + detail
    entities/[type]/[slug]
    findings/         feed + draft + detail
    operator-queue/
    status/           SSE-driven (TODO)
    briefings/
    db/               will embed Datasette
    chat/             Editor persona
    vault/[...path]/  immutable file server for <wiki>/Vault/
  components/
    MarkdownView.tsx  react-markdown + wikilink rewriter
  lib/
    config.ts         env-driven path resolution
    setup-state.ts    reads <wiki>/_runtime/setup-state.json
    wiki.ts           markdown read/list + wikilink resolver
    db.ts             lazy read-only better-sqlite3
  middleware.ts       basic-auth gate
```

## Data layer

All reads go through `<wiki>/`:

- Markdown pages, frontmatter, wikilinks
- SQLite at `<wiki>/_data/tampa.db` (read-only, immutable mode)
- Vault files at `<wiki>/Vault/...` served verbatim under `/vault/...`
- Runtime state at `<wiki>/_runtime/`

The web app writes nothing here directly — write actions go through Hermes session shell-outs (TODO, not yet wired in v0.1 scaffold).

## Plugin model

This repo lives at `~/code/tampa-doge/` and is consumed by Hermes as a plugin. Setup wizard registers the project's skills + cron jobs into the operator's Hermes config. Until `<wiki>/_runtime/setup-state.json` reports `complete`, every route redirects to `/setup`.
