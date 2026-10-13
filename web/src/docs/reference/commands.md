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
| `transcribe` | Transcribe collected audio on the configured transcription server. |
| `index` | Chunk extracted text into the search index. |
| `embed` | Embed indexed chunks into the vector table. |
| `ingest` | Fetch one or more URLs into the content-addressed store. |

## Classifiers

Everything here is also in the [workspace](../start/workspace.md), where it is the Classify,
Runs and Review pages. `classify` is listed with the stages by `centinel --help`, but `run`
never calls it: text leaves the machine only when a person starts a classifier run. See
[Classify](../use/classify.md).

| Command | What it does |
|---|---|
| `classify` | Score indexed documents with Jev: exclude junk, tag the rest. *(operator)* |
| `questions` | List the saved classifier questions, or restore the shipped defaults among them. *(operator)* |
| `evaluate` | Compare the classifier's answers with the review verdicts, per question. |

## Corpus

| Command | What it does |
|---|---|
| `search` | Search the corpus for a passage. |
| `read` | Read the extracted text of a collected document. |
| `download` | Download a collected document's bytes, base64-encoded, for saving elsewhere. |
| `open` | Open a collected document in an application. *(host)* |
| `status` | Count what is stored, by source and content kind, and what it occupies on disk. |
| `list` | List every source — configured or collected — with resource counts and liveness. |
| `check` | See what extraction makes of one link or file. Nothing is stored. |
| `verify` | Read every vector fragment back, and with `--repair` drop the ones that cannot be. *(operator)* |
| `investigate` | Ask the registry what it makes of an address. Nothing is stored. |
| `crumbs` | The off-host hosts this corpus links to, and the rulings on them. |
| `schedules` | Show configured schedules, when each next fires, and how the last one went. |
| `history` | Show what scheduled and manual runs did, newest first. |

## Host

| Command | What it does |
|---|---|
| `doctor` | Report host readiness: required binaries, inference servers, store location. |
| `update` | Check the repo and GitHub for a newer Centinel, and install it. *(host)* |

## Servers

| Command | What it does |
|---|---|
| `serve` | Run the HTTP server (ops as routes, plus MCP over HTTP). Default bind `127.0.0.1:8787`. |
| `serve start` / `stop` / `restart` / `status` | Run `serve` as a service of your login. See [The server](../operate/serve.md). |
| `mcp` | Run an MCP server over stdio. |
| `web` | Open the corpus workspace in a browser. Never starts the scheduler. See [The workspace](../start/workspace.md). |

`serve --no-schedule` serves the read API without firing any `[[schedule]]`.
`serve --tailscale` also serves it on this machine's tailnet over HTTPS.

`web` takes `--bind ADDR` (default `127.0.0.1:8787`), and `--server URL` to open this
binary's page against another machine's corpus. A source build also has `--rebuild`, which
rebuilds the page from the checkout and serves that instead of the one embedded in the binary.

## Global flags

| Flag | Effect |
|---|---|
| `--root DIR` | store root. Also `$CENTINEL_ROOT`. |
| `--config FILE` | config file. Also `$CENTINEL_CONFIG`. |
| `--json` | force JSON output on a terminal |
| `--pretty` | force rendered prose into a pipe |
| `-y`, `--yes` | answer yes to any confirmation — today, `investigate` offering to add the source it found, and the `schedule set` write |
| `--color auto\|always\|never` | override colour. `NO_COLOR` is honoured. |

Output format defaults to the destination: prose to a terminal, JSON to a pipe.

## Flags worth knowing

```bash
centinel run --source agartha --limit 50 --skip embed
centinel collect --source agartha --match /assets/ --rps 5
centinel embed --dry-run
centinel embed --limit 100
centinel embed --batch 64
centinel search "budget" --source agartha -n 20 --snippet-chars 0
centinel search "permits" --tag record_type:minutes --not-tag record_type:data_table
centinel classify --dry-run
centinel classify --source agartha --question page_kind
centinel schedules --check
centinel history --failed --since 2026-08-01T00:00:00Z
centinel history --run 8f3c
```

`--limit` on `run` and `collect` bounds **collection**, never discovery — a truncated
snapshot of a source's address set would look exactly like a source that shrank.

`--batch` is chunks per request to the embedding server, and overrides
`[inference.embed] batch` for one run. `embed --adopt` moves the stored vectors to the
configured server once it has measured that the server reproduces them — see [Inference
servers](inference.md).

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
