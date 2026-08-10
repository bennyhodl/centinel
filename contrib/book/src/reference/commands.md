# Commands

Every verb below is one annotated Rust function, and each is reachable from the CLI, from
HTTP at `POST /ops/<name>`, and from MCP — except where **reach** forbids it. See
[Ops](../internals/ops.md).

`centinel --help` builds this list by iterating the registry, so it is never out of date.

## Pipeline

| Command | What it does |
|---|---|
| `run` | Collect everything new for every configured source, then index and embed it. |
| `source` | Add, list and remove the sources `centinel run` walks. |
| `schedule` | Write and remove the cadences `centinel serve` fires runs on. |

## Stages

Each is what `run` does for you, available on its own.

| Command | What it does |
|---|---|
| `discover` | Enumerate every address a source declares. |
| `collect` | Acquire every address the latest discovery run found, skipping what is already stored. |
| `extract` | Derive searchable text from collected documents. |
| `transcribe` | Transcribe collected audio with a local Whisper model. |
| `index` | Chunk extracted text into the search index. |
| `embed` | Embed indexed chunks into the vector table. |
| `ingest` | Fetch one or more URLs into the content-addressed store. |

## Corpus

| Command | What it does |
|---|---|
| `search` | Search the corpus for a passage. |
| `read` | Read the extracted text of a collected document. |
| `open` | Open a collected document in an application. *(host)* |
| `list` | List sources in the store with resource counts and liveness. |
| `check` | See what extraction makes of one link or file. Nothing is stored. |
| `investigate` | Ask the registry what it makes of an address. Nothing is stored. |
| `schedules` | Show configured schedules, when each next fires, and how the last one went. |
| `history` | Show what scheduled and manual runs did, newest first. |

## Host

| Command | What it does |
|---|---|
| `doctor` | Report host readiness: required binaries, store location, corpus size. |
| `models` | Inspect, fetch, verify and remove model weights. |

## Servers

| Command | What it does |
|---|---|
| `serve` | Run the HTTP server (ops as routes, plus MCP over HTTP). Default bind `127.0.0.1:8787`. |
| `mcp` | Run an MCP server over stdio. |

`serve --no-schedule` serves the read API without firing any `[[schedule]]`.

## Global flags

| Flag | Effect |
|---|---|
| `--root DIR` | store root. Also `$CENTINEL_ROOT`. |
| `--config FILE` | config file. Also `$CENTINEL_CONFIG`. |
| `--json` | force JSON output on a terminal |
| `--pretty` | force rendered prose into a pipe |
| `--color auto\|always\|never` | override colour. `NO_COLOR` is honoured. |

Output format defaults to the destination: prose to a terminal, JSON to a pipe.

## Flags worth knowing

```bash
centinel run --source agartha --limit 50 --skip embed
centinel collect --source agartha --match /assets/ --rps 5
centinel embed --dry-run
centinel embed --limit 100
centinel search "budget" --source agartha -n 20 --snippet-chars 0
centinel schedules --check
centinel history --failed --since 2026-08-01T00:00:00Z
centinel history --run 8f3c
```

`--limit` on `run` and `collect` bounds **collection**, never discovery — a truncated
snapshot of a source's address set would look exactly like a source that shrank.

`--user-agent` and `--timeout-secs` are available on the ops that fetch on your behalf
(`check`, `investigate`). A descriptive User-Agent measurably reduces WAF 403s.

## Handles

`search`, `read` and `open` all print a short blob hash, and all three accept one back by
prefix, git-style.

```bash
centinel read 3f9a2c1
centinel open 3f9a2c1
```

Both the original `blob_sha` and the `derived_sha` are valid targets.
