# Install

```bash
curl --proto '=https' --tlsv1.2 -sSf \
  https://raw.githubusercontent.com/bennyhodl/centinel/master/install.sh | sh
```

One command installs and updates. It downloads a release binary when the latest release
carries one this host can run, and builds from source when it does not — which is most
hosts, and every host until binary releases are switched on.

A download needs no Rust and no toolchain. A build needs Rust 1.91+, a C compiler and
`protoc`; the script checks for each before it starts, and names the command for whatever is
missing rather than installing a toolchain behind your back.

Centinel runs no model itself. Embedding, reranking and transcription are requests to
servers you run — see [Inference servers](../reference/inference.md) — so the binary is the
same on a laptop and on a GPU box, and nothing here asks which GPU you have.

## What a release carries

One asset per platform, one binary in each:

| Asset | Wants |
|---|---|
| `aarch64-apple-darwin` | an Apple Silicon Mac |
| `x86_64-unknown-linux-gnu` | AVX2 — any x86_64 since about 2013 |
| `aarch64-unknown-linux-gnu` | an arm64 Linux host, the DGX Spark included |

None of them carries a GPU backend, because none of them runs a model. The GPU belongs to
the inference server.

Nothing about the download is load-bearing. No asset for this host, no release, a checksum
that does not match, a binary that will not start — each falls back to the build, so the
worst a bad release does is cost one request. The exception is a checksum that is published
and does not match, which stops: that is somebody handing you a different file.

Before it downloads, the script asks the latest release which version it is and stops early
when that is the one already installed. `--force` installs it again.

## Flags

Flags go after `-s --`, which is how a piped shell is handed arguments:

```bash
curl -sSf https://raw.githubusercontent.com/bennyhodl/centinel/master/install.sh \
  | sh -s -- --deps
```

| Flag | Effect |
|---|---|
| `--build` | build from source even where a release binary would fit |
| `--download` | download, and fail rather than quietly build when there is no asset |
| `--force` | install again when the release is the version already here |
| `--portable` | build for the baseline CPU, so the binary can be copied elsewhere |
| `--bin-dir <dir>` | somewhere other than `~/.cargo/bin` |
| `--tag <tag>` | a released tag rather than the latest release or the default branch |
| `--deps` | install what is missing with this host's package manager |
| `--no-doctor` | skip the closing `centinel doctor` |

`--bin-dir` and `--tag` are also `CENTINEL_BIN_DIR` and `CENTINEL_TAG`; `CENTINEL_METHOD=download|build` is `--download` and `--build`;
`CENTINEL_NATIVE=0` is `--portable`. Nothing is prompted, so an unattended run behaves the
same as an attended one.

## From a clone

The same script, run from a checkout, installs **the checkout** — so a contributor testing
a change installs the change rather than the default branch.

```bash
git clone https://github.com/bennyhodl/centinel
cd centinel
./install.sh
```

It decides which of the two it is doing by whether it is a file on disk beside a workspace.
Piped, `$0` is the shell and there is no clone to find, so the sources come from git.

A clone always builds. Downloading a release over the change somebody is testing would be
the wrong answer to the question they asked, so a clone downloads only when told to in as
many words: `./install.sh --download`.

## Updating

```bash
centinel update
```

It asks two things, in that order: **the repo, then GitHub**. The repo is the clone this
binary was built from — the build stamps which directory that was, so the answer is about
the binary you are running rather than about whichever checkout you happen to be standing
in. GitHub is the latest published release, and it is asked either way: a clone that has
not been fetched from in a month still wants to be told a release happened.

Both answers are true at once and neither cancels the other. A contributor's checkout ahead
of the last tag is the ordinary state of a clone, so `update` reports the commits *and* the
release and leaves the reading to you.

```text
centinel 0.5.0  clone · /home/ben/centinel

repo
  master a56c1ea2a92c
  ! 7 commits in this checkout that this binary was not built from
  ! 2 commits behind origin

github
  ! v0.6.0  released 2026-08-09
    https://github.com/bennyhodl/centinel/releases/tag/v0.6.0
```

Then it installs, by running `install.sh` — the same script this page opens with, because
whether this host downloads or builds, which asset it takes and the CPU tuning are all
decided there, and a second copy of that logic would be wrong the first time the real one
changed. A clone pulls (fast-forward only) and
runs its own copy; anything else fetches the script **at the release tag** and prints the
address it came from before running it — so a binary that arrived as a download updates
through the same pipe that installed it, and reports itself as `release binary` rather
than as a clone it never had.

| Flag | Effect |
|---|---|
| `--check` | report and stop, without building anything |

Nothing is built when nothing is newer, and nothing is pulled into a working tree with
uncommitted changes — `update` says so and stops rather than touching your work. If neither
authority could be reached, it says *that*: an unreachable GitHub never reads as "up to
date".

## What the script does that `cargo install` cannot

**It tunes for the CPU it is building on**, with `-C target-cpu=native`, because it installs
to the machine that built it. The cost is that the binary is then built for that CPU.
`--portable` is the way out, and it leaves `RUSTFLAGS` alone, so a middle tier that still
carries AVX2 and FMA is:

```bash
RUSTFLAGS="-C target-cpu=x86-64-v3" ./install.sh --portable
```

The release binaries are built exactly that way: `x86-64-v3` on x86_64 Linux, the arm64
baseline elsewhere, and on macOS the apple-m1 baseline every Apple Silicon Mac shares.

**It says what an older install left behind.** Before inference moved to servers, Centinel
installed a second binary, `centinel-whisper`, and kept model weights in a cache directory.
Neither is used any more. The script names both, with their size, and deletes neither —
the weights are the same files `llama-server` and `whisper.cpp`'s server load, so
you may want to point a server at them rather than download them again.

## By hand

```bash
cargo install --git https://github.com/bennyhodl/centinel centinel
```

From a clone:

```bash
cargo install --path crates/centinel
```

Building `centinel` from source also builds the workspace page that is embedded in it, so a
source build needs **Node.js 22.12 or newer** on the machine. `cargo build` runs `npm ci`
and the page build itself, and refuses to start without Node. A release download needs
none of this, because the page is already inside the binary.

## External tools

Corpus processing shells out to standalone tools. Serving the web app and remote MCP also requires Node.js 22.13+ for the managed TanStack Start authentication server.

| Binary | Needed for | Required |
|---|---|---|
| `yt-dlp` | YouTube acquisition | yes |
| `ffmpeg` | decodes audio to the 16 kHz WAV the transcription server is sent | yes |
| `pdftoppm` (poppler), `tesseract` | OCR | not yet — nothing calls them |

```bash
brew install yt-dlp ffmpeg          # macOS
sudo apt install yt-dlp ffmpeg      # Debian/Ubuntu
```

Keep `yt-dlp` current. It ships releases in emergency clusters when YouTube changes, and
`centinel doctor` warns once yours is past ninety days.

## Then

```bash
centinel doctor         # what this machine is missing, and which servers are answering
```

Run `doctor` first and after every step. It names the fix beside each gap it finds. Search
works on keywords alone; the vector half and the reranker need an embedding and a rerank
server, and the transcribe stage needs a transcription server. [Inference
servers](../reference/inference.md) has the commands that start them.

Then open the workspace, which is how most of Centinel is used:

```bash
centinel web
```

It needs nothing beyond the binary. The page is embedded in it, with no asset directory and
no Node runtime. Only a build from source needs Node 22.12 or newer, to make the page.

Next: [The workspace](workspace.md), then [Your first corpus](first-corpus.md).
