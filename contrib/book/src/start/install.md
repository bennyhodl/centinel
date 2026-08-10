# Install

You need Rust 1.91+ and a C++ toolchain. Two dependencies compile `llama.cpp` and
`whisper.cpp` from source, so expect a long first build.

```bash
git clone https://github.com/bennyhodl/centinel
cd centinel
./install.sh
```

The script checks the host before it builds anything, and names the command for whatever
is missing rather than installing a toolchain behind your back.

| Flag | Effect |
|---|---|
| `--accel auto\|none\|cuda\|vulkan\|rocm` | override the detected GPU backend |
| `--portable` | build for the baseline CPU, so the binary can be copied elsewhere |
| `--bin-dir <dir>` | somewhere other than `~/.cargo/bin` |
| `--deps` | install `ffmpeg` and `yt-dlp` too, with this host's package manager |
| `--no-doctor` | skip the closing `centinel doctor` |

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

`cargo` cannot take two `--path` arguments, and the workspace root is a virtual manifest,
so from a clone it is two commands:

```bash
cargo install --path crates/centinel
cargo install --path crates/centinel-whisper
```

Without a clone, one command does both:

```bash
cargo install --git https://github.com/bennyhodl/centinel centinel centinel-whisper
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
[Models](../operate/models.md) for what `models pull` fetches and how much disk it wants.

Next: [Your first corpus](first-corpus.md).
