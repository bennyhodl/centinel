# bin/

Centinel's CLI surface. Two executables, both thin shims into the TypeScript
codebase under `server/dist/`.

| Script | Purpose | Called by |
|---|---|---|
| `centinel` | Operator CLI — server lifecycle, cron management, role runs, doctor, chat. Wraps `server/dist/cli.js`. | `bootstrap`, the Next.js web app (server actions), the operator. |
| `centinel-server` | Boots the runtime server in the foreground. Equivalent to `centinel server start`. Wraps `server/dist/server.js`. | `bootstrap` (recommended via process supervisor / docker compose), the operator. |

Both wrappers rebuild the TypeScript sources if `server/dist/` is missing.

## Common subcommands

```
centinel server start                       # boot the runtime server (foreground)
centinel server status                      # probe /health

centinel role <name> --interactive          # open pi's TUI scoped to <name>
centinel role <name> -p "..."               # one-shot run; streams events

centinel run list                           # recent runs
centinel run get <runId>                    # run summary JSON
centinel run tail <runId>                   # SSE-tail a run

centinel cron list
centinel cron fire <name>                   # manual trigger
centinel cron pause <name> | resume <name>
centinel cron pause-all | resume-all
centinel cron seed-paused                   # offline seed (used by bootstrap)

centinel investigate register <slug> --cron "<expr>" [--prompt "..."]
centinel investigate unregister <slug>

centinel chat send -m "..." [--session <id>]
centinel chat list [--active]
centinel chat abort <sessionId>

centinel doctor                             # health check
```

The five Centinel **roles** (editor, investigator, archivist, data-reporter,
watch-runner) live inside the runtime server. There is no per-role shim — use
`centinel role <name> --interactive` to drop into a TUI scoped to that role's
skill and tools. See `docs/PI_MIGRATION_PLAN.md`.

## Adding to PATH

```sh
export PATH="$(pwd)/bin:$PATH"
```

Bootstrap prints this hint if `bin/` isn't on the operator's PATH.
