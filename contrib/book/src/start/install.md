# Install

```bash
curl --proto '=https' --tlsv1.2 -sSf \
  https://raw.githubusercontent.com/bennyhodl/centinel/master/install.sh | sh
```

One command installs and updates. It downloads a release binary when the latest release
carries one this host can run, and builds from source when it does not — which is most
hosts, and every host until binary releases are switched on.

A download needs no Rust and no C++ toolchain. A build needs Rust 1.91+, a C++ compiler,
`cmake`, `libclang` and `protoc`; the script checks for each before it starts, and names
the command for whatever is missing rather than installing a toolchain behind your back.

## What a release carries

Two binaries, and both are GPU builds. Embedding is the stage measured in days, so a
CPU-only download would be the slow half of Centinel handed over as an install.

| Asset | Wants |
|---|---|
| `aarch64-apple-darwin` | an Apple Silicon Mac. Metal is compiled in, shaders and all |
| `x86_64-unknown-linux-gnu`, CUDA 12 | an NVIDIA driver, the CUDA runtime, and AVX2 |

`centinel` links cuBLAS statically and needs only the driver. `centinel-whisper` does not,
so the CUDA asset wants the runtime present — `cuda-runtime-12-4` is about 150 MB and
carries no compiler. Bundling those libraries into the asset would be most of a gigabyte.

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
  | sh -s -- --accel cuda --deps
```

| Flag | Effect |
|---|---|
| `--build` | build from source even where a release binary would fit |
| `--download` | download, and fail rather than quietly build when there is no asset |
| `--force` | install again when the release is the version already here |
| `--accel auto\|none\|cuda\|vulkan\|rocm` | override the detected GPU backend |
| `--portable` | build for the baseline CPU, so the binary can be copied elsewhere |
| `--bin-dir <dir>` | somewhere other than `~/.cargo/bin` |
| `--tag <tag>` | a released tag rather than the latest release or the default branch |
| `--deps` | install what is missing with this host's package manager |
| `--no-doctor` | skip the closing `centinel doctor` |

`--accel`, `--bin-dir` and `--tag` are also `CENTINEL_ACCEL`, `CENTINEL_BIN_DIR` and
`CENTINEL_TAG`; `CENTINEL_METHOD=download|build` is `--download` and `--build`;
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
whether this host downloads or builds, the accelerator, the CPU tuning and the
two-binaries-in-one-directory rule are all decided there and a second copy of that logic
would be wrong the first time the real one changed. A clone pulls (fast-forward only) and
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

**It selects the GPU backend.** Metal on macOS, CUDA or ROCm on Linux when their
toolchains are present.

**It tunes for the CPU it is building on.** This matters more than it sounds like.
`llama-cpp-sys-2` reads `target-cpu` back out of the Rust flags, and when it is not
`native` it sets `GGML_NATIVE=OFF` and derives ggml's instruction-set flags from the
*baseline* target features instead. On `x86_64-unknown-linux-gnu` those are
`fxsr,sse,sse2,x87` — so a plain `cargo install` compiles llama.cpp's CPU kernels with no
AVX, no AVX2 and no FMA, and nothing recovers it at runtime. `whisper.cpp` has no such
handling and builds native already. So the untuned case is not "both a little slow". It is
the transcriber tuned and the embedder not, and embedding is the stage measured in days.

The cost is that the binary is then built for the CPU that built it. `--portable` is the
way out, and it leaves `RUSTFLAGS` alone, so a middle tier that still carries AVX2 and FMA
is:

```bash
RUSTFLAGS="-C target-cpu=x86-64-v3" ./install.sh --portable
```

The release binaries are built exactly that way: `x86-64-v3` on Linux, and on macOS the
apple-m1 baseline every Apple Silicon Mac already shares. Both are hosts where the GPU
does the embedding, so what a native build would win back lands on stages that were never
the slow ones. On a CPU-only host it is the other way round, which is why no release
carries a CPU-only binary.

**It installs both binaries into one directory.** Which is the thing transcription does
not work without.

## Why two binaries

`whisper.cpp` and `llama.cpp` each vendor their own copy of **ggml**, and both export the
same ~534 `ggml_*` symbols. Linked into one binary, the linker keeps one copy and silently
resolves the other library's calls to it. The two versions are not the same.

Measured on identical audio and model, the linked crates the only variable:

| binary | result |
|---|---|
| `whisper-rs` alone | 2 segments — *"The council meeting will come to order."* |
| `whisper-rs` + `llama-cpp-2` | **0 segments**, every token at `p=0.000` |

It links without a warning, runs without a crash, and transcribes nothing. There is no
error to catch. So `centinel` links `llama.cpp`, `centinel-whisper` links `whisper.cpp`,
and the two meet over a pipe.

`centinel` finds the worker beside itself first, then `$CENTINEL_WHISPER_BIN`, then
`PATH`. Installing both with the same command puts them in the same directory, which is
all it needs.

## By hand

One command does both:

```bash
cargo install --git https://github.com/bennyhodl/centinel centinel centinel-whisper
```

From a clone it is two, because `cargo` cannot take two `--path` arguments and the
workspace root is a virtual manifest:

```bash
cargo install --path crates/centinel
cargo install --path crates/centinel-whisper
```

Any GPU backend other than Metal is a `--features` flag on **each** of the two commands.
Passing `--features cuda` to one and not the other leaves half the pipeline on the CPU.

## External tools

Centinel shells out rather than running a second language runtime.

| Binary | Needed for | Required |
|---|---|---|
| `yt-dlp` | YouTube acquisition | yes |
| `ffmpeg` | decodes audio to 16 kHz mono PCM for transcription | yes |
| `pdftoppm` (poppler), `tesseract` | OCR | not yet — nothing calls them |

```bash
brew install yt-dlp ffmpeg          # macOS
sudo apt install yt-dlp ffmpeg      # Debian/Ubuntu
```

Keep `yt-dlp` current. It ships releases in emergency clusters when YouTube changes, and
`centinel doctor` warns once yours is past ninety days.

## Then

```bash
centinel doctor         # what this machine is missing, and the command for each gap
centinel models pull    # weights for search and transcription
```

Run `doctor` first and after every step. It names the fix beside each gap it finds. See
[Models](../internals/models.md) for what `models pull` fetches and how much disk it wants.

Next: [Operator](operator.md).
