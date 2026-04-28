# bin/

Centinel's CLI surface. Two kinds of executable here:

| Script | Purpose | Called by |
|---|---|---|
| `centinel` | **Dispatcher** — setup, cron management, investigation registration. Real Python (`lib/cli.py`). | `bootstrap`, the Next.js web app (server actions), the operator. |
| `centinel-investigator` | Profile shim — opens an interactive Hermes session in the `investigator` profile. | The operator (terminal only). Not used by web app or cron. |
| `centinel-archivist` | Profile shim → `archivist`. | Operator only. |
| `centinel-data-reporter` | Profile shim → `data-reporter`. | Operator only. |
| `centinel-watch-runner` | Profile shim → `watch-runner`. | Operator only. |

The role shims are 1-line `exec hermes --profile <role> "$@"` wrappers. They exist so the operator can drop into a clean per-role Hermes session without remembering `--profile` flags. They are NOT called by the runtime (web app delegates via `delegate_task`; cron uses `hermes --profile <role> cron create ...`). See `docs/AGENT_INVOCATION.md` for the full lane model.

## Dispatcher subcommands

```
centinel bootstrap-sitemap [DOMAIN]   # wizard Step 5; runs sitemap-builder
centinel setup-profiles                # bootstrap step; idempotent
centinel setup-cron                    # bootstrap step; registers paused jobs
centinel cron resume-all               # wizard Step 7
centinel cron pause-all                # emergency stop
centinel cron list                     # show all centinel-owned jobs
centinel investigate register <slug>   # register per-investigation cron
centinel doctor                        # health check
```

## Adding to PATH

`./bootstrap` (post-v0.1) symlinks these into `~/.local/bin/`. For dev, just add the repo's `bin/` to your PATH or run `./bin/centinel <subcommand>`.
