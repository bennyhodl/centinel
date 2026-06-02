# @centinel/server

The Centinel runtime server — built on
[`@mariozechner/pi-coding-agent`](https://www.npmjs.com/package/@mariozechner/pi-coding-agent).
Eventually owns every agent role (Editor, Investigator, Archivist, Data
Reporter, Watch Runner), the internal cron, the HTTP/SSE API, the run log,
and the operator-facing `centinel` CLI.

See [`../docs/PI_MIGRATION_PLAN.md`](../docs/PI_MIGRATION_PLAN.md) for the
full migration plan.

## Phase 2 — all roles + internal cron

What's wired right now:

- `centinel-server` listening on `127.0.0.1:8787`.
- All five roles registered: `editor`, `investigator`, `archivist`,
  `data-reporter`, `watch-runner`. Each loads only its own SKILL.md.
- Persistent cron table at `.runtime/cron.json` seeded with the default
  schedule; doge.config.yaml `cron_schedule_overrides:` honored.
- Internal scheduler (croner) fires `runRole()` — same code path as HTTP
  + CLI invocations.
- HTTP surface:
  - `GET  /health`
  - `POST /run/:role` (JSON or SSE)
  - `GET  /runs`, `/runs/:id`, `/runs/:id/events` (SSE replay + tail)
  - `GET  /cron/jobs`, `/cron/jobs/:name`
  - `POST /cron/jobs/:name/{fire,pause,resume}` (`fire` supports SSE)
  - `POST /cron/{pause-all,resume-all}`
  - `POST /cron/investigations/:slug/register`, `DELETE` to remove
- CLI: `centinel server|role|run|cron|investigate` subcommands.

What's still TODO:

- Editor persona system-prompt override + `delegate` custom tool + `/chat`
  endpoint (Phase 3).
- Real implementations for `qmd_search`, `db_query`, `vault_put`,
  `web_fetch` (currently return `not_yet_implemented` stubs).
- Phase 4: Hermes decommissioned ✅

## Build & run

From the repo root:

```bash
pnpm install
pnpm --filter @centinel/server build

# Terminal 1
pnpm --filter @centinel/server exec centinel-server

# Terminal 2
pnpm --filter @centinel/server exec centinel server status
```

Or from inside `server/`:

```bash
pnpm build
node dist/server.js          # or: pnpm exec centinel-server
node dist/cli.js server status
```

## Environment

| Var              | Purpose                  | Default     |
| ---------------- | ------------------------ | ----------- |
| `CENTINEL_HOST`  | bind host                | `127.0.0.1` |
| `CENTINEL_PORT`  | bind port                | `8787`      |

## Layout

```
server/
├── src/
│   ├── server.ts            # HTTP entry; route table
│   ├── cli.ts               # centinel CLI entry
│   ├── config.ts            # host/port/repoRoot/runtimeDir
│   ├── dogeConfig.ts        # parse doge.config.yaml
│   ├── http/
│   │   ├── util.ts          # JSON, SSE, error helpers
│   │   ├── runRoutes.ts     # /run/:role, /runs/*
│   │   └── cronRoutes.ts    # /cron/*
│   ├── roles/
│   │   ├── types.ts         # RoleConfig, RunInput, RunResult
│   │   ├── registry.ts      # name → builder map
│   │   ├── editor.ts
│   │   ├── investigator.ts
│   │   ├── archivist.ts
│   │   ├── dataReporter.ts
│   │   └── watchRunner.ts
│   ├── runtime/
│   │   ├── runRole.ts       # single entry point for every role run
│   │   ├── runLogger.ts     # .runtime/runs/<id>.{jsonl,json}
│   │   ├── runStore.ts      # active runs + disk replay
│   │   └── customTools.ts   # qmd_search / db_query / vault_put / web_fetch stubs
│   ├── cron/
│   │   ├── schedule.ts      # default cron jobs + override application
│   │   ├── cronTable.ts     # .runtime/cron.json read/write
│   │   └── scheduler.ts     # croner wrapper, skip-if-running policy
│   └── chat/
│       └── chatSessions.ts  # per-thread editor AgentSession manager
├── tsconfig.json
└── package.json             # bin: centinel + centinel-server
```

## Roadmap

- ~~**Phase 0:** scaffold.~~ ✅
- ~~**Phase 1:** investigator role + run log + HTTP/SSE.~~ ✅
- ~~**Phase 2:** remaining four roles + internal cron table.~~ ✅
- **Phase 3:** editor `/chat` + persona + `delegate` tool.
- ~~**Phase 4:** Hermes decommissioned.~~ ✅
