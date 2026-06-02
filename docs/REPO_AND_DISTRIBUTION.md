---
title: Centinel — Repo & Distribution (LOCKED)
status: 🔒 Locked v1
created: 2026-04-26
parent: README.md
---

# Repo & Distribution

How a forker spins up `<their-city>-doge` from scratch. Locked 2026-04-26 (plan checkpoint v10).

## Distribution model

**GitHub template repo** at `lygos/centinel-template` (public).

```
gh repo create my-org/cleveland-doge --template lygos/centinel-template
cd cleveland-doge
./bootstrap
```

Three commands → working `<city>-doge` instance. NPX-style installer (`npx civic-doge init`) deferred to v0.2 if the template-repo flow proves clunky.

## Repo layout

```
centinel-template/                  # the public repo people fork
├── README.md                         # quickstart + philosophy
├── LICENSE                           # AGPL (civic-data project; viral copyleft fits the ethos)
├── CHANGELOG.md                      # breaking changes documented
├── bootstrap                         # the one entry script
├── .env.example                      # secrets template
├── doge.config.yaml.example          # city-specific config template
│
├── skills/                           # Centinel skills (pi-agent), packaged
│   ├── centinel-cartographer/SKILL.md
│   ├── centinel-investigator/SKILL.md
│   ├── centinel-archivist/SKILL.md
│   ├── centinel-data-reporter/SKILL.md
│   ├── centinel-watch-runner/SKILL.md
│   └── centinel-editor/SKILL.md
│
├── lib/                              # shared Python the skills import
│   ├── db/                           # schema, migrations, common queries
│   ├── extractors/                   # extractor catalog (post-spike)
│   ├── runtime/                      # inbox/outbox/status board helpers
│   ├── adapters/                     # ONLY if the role's web_fetch / pi-agent tools don't suffice (see below)
│   ├── doge.py                       # CLI used by web app server actions
│   └── __version__.py
│
├── web/                              # Next.js app
│   ├── app/
│   ├── package.json
│   └── ...
│
├── presets/                          # shipped, city-agnostic
│   ├── watches/
│   │   ├── errant-spending.yaml
│   │   ├── corruption-signals.yaml
│   │   └── policy-drift.yaml         # disabled by default
│   └── investigations/
│       ├── topic-dig.yaml
│       ├── contractor-profile.yaml
│       └── person-follow.yaml
│
├── city-overlay/                     # operator territory — survives upstream pulls
│   ├── README.md                     # explains the overlay rule
│   ├── extractors/                   # city-specific overrides
│   ├── watches/                      # operator's custom watches
│   └── exclude-patterns.yaml         # paths to skip on this city.gov
│
├── docker-compose.yml                # web + Datasette
├── docker-compose.dev.yml            # local dev overrides
└── tools/
    ├── doctor                        # health check
    ├── activate-cron                 # flips paused → active
    ├── reset-wiki                    # nuke + re-bootstrap (dangerous, prompts)
    └── snapshot-backup
```

**Not shipped in the repo:**
- `plans/` — internal design docs stay in private workspace, not part of the public template
- `~/wiki/<City>/` — created by bootstrap, lives at operator's chosen path, never in repo
- `.env` — secrets, `.gitignore`d

## Three layers of config

| Layer | Where | Owned by | Survives `git pull`? |
|---|---|---|---|
| **Template defaults** | `presets/`, `lib/`, `skills/`, `web/` | Upstream maintainers | No — pulls bring updates |
| **Operator config** | `doge.config.yaml`, `city-overlay/` | Operator (committed to their fork) | Yes — never overwritten |
| **Secrets** | `.env` | Operator (gitignored) | Yes — never in git |

### `doge.config.yaml` (operator-edited, committed)

```yaml
city:
  name: Cleveland
  slug: cleveland
  domain: clevelandohio.gov
  timezone: America/New_York

wiki:
  path: ~/wiki/Cleveland

watch_presets:
  - errant-spending
  - corruption-signals

cron_schedule_overrides:
  sitemap_lint: "0 3 * * 1"            # weekly Monday 3am
  watch_runner: "0 4 * * *"            # daily 4am
  briefings:    "0 9 * * 1"            # Monday 9am

confidential_investigations: []        # slugs to suppress from public /status
```

### `.env` (gitignored, never committed)

```bash
WEB_BASIC_AUTH_PASSWORD=xxxxxx
ANTHROPIC_API_KEY=sk-ant-...
# (centinel-server reads Anthropic creds via pi-agent's standard env)
NOTIFICATION_DISCORD_WEBHOOK=         # optional
# Adapter API keys (only if used — see Web tooling section below)
FIRECRAWL_API_KEY=
TAVILY_API_KEY=
```

### `city-overlay/` (operator-edited, committed)

The escape hatch for city-specific knowledge. Operator adds:

- `city-overlay/watches/utility-billing.yaml` — Cleveland-specific watch
- `city-overlay/extractors/cleveland-budget-pdf.yaml` — Cleveland's specific budget book layout
- `city-overlay/exclude-patterns.yaml` — paths that shouldn't be crawled on this domain

Skills load `presets/<thing>` first, then merge `city-overlay/<thing>` on top. Operator changes never collide with upstream updates.

## Web tooling (revised)

The pi-agent toolset gives each role a `web_fetch` tool — currently a stub (see `PI_MIGRATION_PLAN.md`). Skills call it directly:

| Need | pi-agent tool | Adapter required? |
|---|---|---|
| Fetch HTML page → markdown | `web_fetch` (TODO: stub) | No |
| Fetch PDF → markdown | `web_fetch` (TODO: stub) | No |
| Search the web | (TODO: stub) | No |
| Render JS-heavy SPA | rendered fetch path (TODO: stub) | No |
| Take screenshot | (TODO: stub) | No |
| Crawl a domain (sitemap.xml + recursive) | Not built-in | **Maybe** — `lib/adapters/site_mapper.py` |

The only place we may need a third-party adapter is **bulk site-mapping** — Firecrawl `/map` produces a more complete URL list than walking sitemap.xml + recursive crawl with built-in tools.

**Default assumption: pi-agent tools will suffice for v0.1 once the stubs land.** Adapters get added under `lib/adapters/` only if real-world use surfaces concrete gaps, with operator-supplied API keys in `.env`.

This is a meaningful simplification: no `Scraper` interface, no Firecrawl/Tavily/Playwright juggling. Skills just call `web_fetch(url)` and trust the role's runtime.

## How `bootstrap` works

```bash
#!/usr/bin/env bash
set -euo pipefail

# 1. Sanity checks
require_command node        # centinel-server needs a recent Node + pnpm
require_command pnpm
require_command python3.11  # skill helper scripts

# 2. Read or generate config
[ -f doge.config.yaml ] || cp doge.config.yaml.example doge.config.yaml
[ -f .env ] || cp .env.example .env
prompt_if_unset_in_yaml "city.domain"
prompt_if_unset_in_yaml "city.name"
prompt_if_unset_in_yaml "city.slug"
prompt_if_unset_in_yaml "wiki.path"
prompt_if_unset_in_env  "WEB_BASIC_AUTH_PASSWORD"

# 3. Create wiki structure
WIKI=$(yq '.wiki.path' doge.config.yaml | envsubst)
ensure_dir "$WIKI/Sitemap"
ensure_dir "$WIKI/Investigations"
ensure_dir "$WIKI/Vault/{pdfs,html,transcripts,images}"
ensure_dir "$WIKI/Watches/_presets"
ensure_dir "$WIKI/_runtime/{inbox,outbox,status,operator-queue,huddle}"
ensure_dir "$WIKI/_data"
write_if_missing "$WIKI/SCHEMA.md" templates/SCHEMA.md
write_if_missing "$WIKI/log.md"    templates/log.md
write_if_missing "$WIKI/_runtime/setup-state.json"  '{"status":"pending"}'

# 4. Initialize SQLite database
python3 -m lib.db.init    "$WIKI/_data/$CITY_SLUG.db"
python3 -m lib.db.migrate "$WIKI/_data/$CITY_SLUG.db"

# 5. Build centinel-server (and the web app)
#    Skill specs are checked into `skills/<name>/SKILL.md` and resolved at
#    runtime by centinel-server's role loader — no symlinking required.
pnpm install
pnpm --filter centinel-server build

# 6. Copy presets idempotently (cp -n: never overwrite operator edits)
for preset in presets/watches/*.yaml; do
  cp -n "$preset" "$WIKI/Watches/_presets/"
done

# 7. Seed centinel-server cron jobs in the paused state.
#    centinel-server owns the cron loop; `centinel cron seed-paused` writes the
#    canonical jobs into .runtime/cron.json so the wizard's Step 7 can resume
#    them once the operator confirms.
./bin/centinel cron seed-paused
# Per-investigation crons get registered dynamically as operator creates investigations.

# 8. Bring up runtime services
docker compose up -d web datasette

# 9. Final report
echo
echo "✅ Bootstrap complete."
echo "🌐 Web app: http://localhost:3000"
echo "🗄️  Datasette: http://localhost:8001"
echo "📖 Open the web app to run the setup wizard."
```

### Idempotency rules (for re-runs and upstream pulls)

`bootstrap` must be safe to re-run. After upstream pulls, operator runs it again to pick up new skills/presets/migrations.

| Action | Behavior on re-run |
|---|---|
| Wiki dirs | Created if missing, never destroyed |
| `SCHEMA.md`, `log.md` | `cp -n` — never overwrite |
| DB init | `CREATE TABLE IF NOT EXISTS`, then run migrations forward-only |
| Skill symlinks | `ln -sfn` — refresh, idempotent |
| Presets | `cp -n` — never overwrite (operator may have tuned them) |
| Cron entries | `centinel cron seed-paused` is upsert-safe by job name (skip if already in `.runtime/cron.json`) |
| `doge.config.yaml`, `.env` | `cp` only if missing |

Result: `git pull && ./bootstrap` brings in upstream skill/lib/preset updates without touching operator config or wiki content.

## Skill update model

Skill specs live in-tree at `skills/<name>/SKILL.md`. centinel-server's role loader reads them at runtime via pi-agent's `DefaultResourceLoader` skills override; no system-wide install step is needed.

- Skills shipped in `skills/<name>/SKILL.md` are pristine and version-controlled.
- Operators who want a custom variant copy the file to `city-overlay/skills/<name>/SKILL.md` and edit there. The role loader prefers overlay paths over presets when both exist.
- `git pull` updates the canonical files in `skills/`. Operator edits to overlay copies are untouched.

CHANGELOG.md documents breaking changes (DB schema migrations, skill API breaks, runtime protocol changes). The `tools/doctor` script runs on every bootstrap and reports any version mismatches between `lib/__version__.py` and what the wiki expects.

## Forking & upgrading

```bash
# Fork the upstream template:
gh repo fork lygos/centinel-template --clone
cd centinel-template
./bootstrap                          # asks for city-specific config
# operator visits http://localhost:3000 → setup wizard → kick off

# Pull upstream improvements over time:
git pull lygos main
# resolve merge conflicts in: skills/, lib/, presets/  (rare)
# city-overlay/ and doge.config.yaml are untouched
./bootstrap                          # idempotent re-run picks up new migrations + presets
./tools/doctor                       # verify health
```

## `tools/doctor` — health check

Idempotent health check. Run after bootstrap, after pulls, when something's off.

Checks:
- Node + pnpm installed, versions >= minimum
- centinel-server build present (server/dist) and runnable
- Docker running
- All cron entries registered and in expected paused/active state
- Wiki path exists, all expected subdirectories present
- DB exists, schema version matches `lib/__version__.py`
- Skill symlinks valid (no broken links)
- `<wiki>/_runtime/setup-state.json` consistent (status=complete iff wizard done)
- Web app responding on configured port
- Datasette responding on configured port
- Latest `<wiki>/Vault/manifest.jsonl` line parses as JSON
- All active investigation YAMLs parse cleanly
- All active watch YAMLs parse cleanly
- Last successful agent run timestamps are within expected freshness windows

Output: green/yellow/red per check, suggested fix command for each red.

## Versioning conventions

- `lib/__version__.py` is the canonical version (`__version__ = "0.3.1"`)
- Tagged releases on GitHub: `v0.3.1`
- DB migrations under `lib/db/migrations/<version>__<description>.sql`, forward-only
- `CHANGELOG.md` follows Keep a Changelog format with breaking changes called out
- Skills carry `version:` in frontmatter; doctor reports if any skill is older than `__version__` minimum

## License: AGPL-3.0

Civic data, public-good project, defense against extractive private forks. AGPL forces any hosted variant (e.g., a private SaaS spin-off) to publish their changes back. If a contributor objects, drop to MIT — but I'd start AGPL and only loosen if it bites.

## Ownership of the upstream template

`lygos/centinel-template` is the canonical upstream. Maintainers (Ben + collaborators) merge improvements from forks back upstream. Bitcoin Bay Foundation could be a co-maintainer org if civic-tech aligns with its scope (separate decision).

## What's NOT in the repo (deliberately)

- `plans/` — design docs stay private to the maintainers' workspace
- `~/wiki/<City>/` — operator's wiki content is theirs, never in the template repo
- `.env` — gitignored
- The vault — never anywhere near git
- Any city-specific data in the template defaults

The template ships **only the apparatus**. Every operator's content is theirs.

## Acceptance criteria

- ✅ Fresh clone of template + `./bootstrap` produces a working web app + paused cron in <10 minutes
- ✅ Wizard at localhost:3000 walks operator through bootstrap → first sitemap → cron activation
- ✅ Re-running `./bootstrap` after upstream pull is safe (idempotent, never destroys operator data)
- ✅ Operator edits to `doge.config.yaml`, `city-overlay/`, `.env` survive `git pull`
- ✅ Operator skill overrides in `city-overlay/skills/` win over `skills/` presets at load time
- ✅ A new city forker (e.g., cleveland-doge) needs to edit only `doge.config.yaml`, `.env`, and optionally `city-overlay/`
- ✅ `tools/doctor` reports green on a healthy install
- ✅ DB migration on upgrade is forward-only and non-destructive
- ✅ AGPL license file present and acknowledged in README

## Open questions (non-blocking)

1. Multi-city in one centinel-server install — should the cron names always be city-prefixed (yes, current design)? What if the same operator runs `centinel` and `cleveland-doge` side by side? Multiple wikis, multiple DBs, distinct `.runtime/` dirs per checkout? Defer until someone actually wants two cities.
2. Web app + centinel-server on different machines? Default assumes co-located. Distributed setup is plausible (web app on a small VPS, centinel-server + wiki on a beefier home server) — defer until needed.
3. `tools/snapshot-backup` design — encrypted snapshot of wiki + DB + vault to S3-compatible storage. Lean on the systemd-weekly-backup skill conventions; specify post-v0.1.
4. Auth upgrade path — when basic auth is no longer enough, prefer better-auth (already in scaffold skill) over rolling our own.
