<p align="center">
  <img src="assets/centinel-watchman.jpg" alt="Centinel — a 1787-era ink etching of a lone watchman on the rampart with candle, scroll, and quill, gazing over a sleeping colonial town" width="600">
</p>

*A civic transparency toolkit — built on the warnings of a Pennsylvania watchman.*

---

**[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)** — How it is built. The store, the domain model, and how one function definition becomes a CLI command, an MCP tool, and an HTTP route. Includes the quickstart.

**[docs/RETRIEVAL.md](docs/RETRIEVAL.md)** — How a question becomes a cited passage. Chunking, the two stores, the local embedder and reranker, and what a result tells you about how much of the corpus it could actually see.

**[docs/SPEC.md](docs/SPEC.md)** — The settled specification. Every locked decision with its reasoning and its accepted costs, plus the six that are still open.

**[docs/research/](docs/research/)** — The evidence underneath it. ~3,850 lines, ~450 primary-source citations.

---

> *"The federal government will... necessarily absorb the state legislatures."*
> — Centinel, 1787

On October 5, 1787 — eighteen days after the Constitutional Convention adjourned in Philadelphia — a Pennsylvania clerk named Samuel Bryan published the first of twenty-four essays under a pseudonym he chose with care.

He called himself **Centinel**. The watchman on the wall. The lone soldier who stays awake while the camp sleeps, whose only job is to warn before the enemy arrives. He addressed his essays *"To the Freemen of Pennsylvania."* Not the gentlemen. Not the delegates. The freemen.

What Bryan warned, you have inherited.

He warned that a federal government with unchecked taxing power would **absorb the state legislatures**. He warned that a republic stretching across an entire continent could only be governed despotically — that distance from the people is itself a form of tyranny. He warned that the states would wither, that a standing army would enforce taxes the citizens had no voice in setting, that local accountability would be the first casualty of consolidated power.

He was right. He just had the wrong scale.

The federal government Bryan feared has come, and grown, and the warnings he wrote echo louder at the level he never wrote about: **the city**. The municipal government across the street from you spends in a year what would have stunned a 1787 freeman. It contracts with names you have never heard. It holds meetings whose minutes you will never read. It awards procurement to relatives of officials whose connections you will never trace.

It answers to no one because no one is watching.

**You are the freeman now.** The watchman's seat is empty.

This is what fills it.

---

Centinel collects the public record of a city — website maps, documents, transcripts, and the changes to all of them over time — and keeps it in a form nobody can quietly edit.

It is built on three principles drawn directly from Bryan's playbook.

**Documents over promises.** Every byte is content-addressed. The hash covers the raw bytes as served — not a summary, not a re-render, not a cleaned-up copy. Reading a document back verifies that hash, so an edit in place is an error rather than a silent success. The watchman demands the original record, not the official summary.

**Never trust memory.** Files on disk are the only truth. Every index, every database, every embedding is derived and rebuildable — delete them all and you lose minutes, not evidence. Nothing in this system can answer from recall, because there is nothing to recall from. There is only the record, read again.

**Notice what disappears.** Every version is retained, and every collection run is a full snapshot — so a page that vanishes is a fact the archive holds, not a gap it forgets. A page that *starts refusing you* is a different fact, recorded differently. Conflating "this was deleted" with "this is now blocked" is how a record quietly corrupts, and the model refuses to do it. Bryan did not warn that *something* would go wrong. He named the way it would go wrong, and he was right.

---

Centinel is a library, a CLI, a server, and an MCP endpoint. The agents come later and sit **on top** — they are clients of the record, never its author. What is collected does not depend on what any model happened to think that day.

Everything runs locally. No document, no transcript, no page ever leaves the machine for a third-party API.

Bryan lost the immediate fight. Pennsylvania ratified the Constitution in December 1787. But the pressure he and his fellow dissenters generated forced the **Bill of Rights** into existence — protections the powerful had not wanted to grant.

The watchman loses individual fights. He wins the ones that matter.

Light the candle.

---

## Install

Rust 1.91+ and a C++ toolchain — two of the dependencies compile `llama.cpp` and `whisper.cpp`.

```bash
curl --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/bennyhodl/centinel/master/install.sh | sh
```

The script checks this host before it builds anything and names the command for whatever is missing, rather than installing a toolchain behind your back. Expect a long first build — there is no prebuilt binary to download, because both packages compile a C++ library from source and the binary correct for a host is the one built on it.

Three things it does that a `cargo install` line cannot. It selects the GPU backend for the host — Metal on macOS, CUDA or ROCm on Linux when their toolchains are present. It tunes for the CPU it is building on, which matters more than it sounds like (below). And it puts **both** binaries in one directory, which is the thing transcription does not work without.

Flags go after `-s --`:

```bash
curl -sSf https://raw.githubusercontent.com/bennyhodl/centinel/master/install.sh | sh -s -- --accel cuda --deps
```

| | |
|---|---|
| `--accel auto\|none\|cuda\|vulkan\|rocm` | override the detected backend |
| `--portable` | build for the baseline CPU, so the binary can be copied to another machine |
| `--bin-dir <dir>` | somewhere other than `~/.cargo/bin` |
| `--tag <tag>` | build a released tag rather than the default branch |
| `--deps` | install `ffmpeg` and `yt-dlp` too, with this host's package manager |
| `--no-doctor` | skip the closing `centinel doctor` |

From a clone it installs **the clone**, so a contributor testing a change installs the change:

```bash
git clone https://github.com/bennyhodl/centinel
cd centinel
./install.sh
```

### Why it tunes for your CPU

`llama-cpp-sys-2` reads `target-cpu` back out of the Rust flags and, when it is not `native`, sets `GGML_NATIVE=OFF` and derives ggml's instruction-set flags from the *baseline* target features instead. On `x86_64-unknown-linux-gnu` those are `fxsr,sse,sse2,x87` — so a plain `cargo install` compiles llama.cpp's CPU kernels with **no AVX, no AVX2 and no FMA**, and nothing recovers it at runtime, because the runtime dispatch that would (`GGML_CPU_ALL_VARIANTS`) ships with a feature this build does not enable. On Linux `aarch64` the same build script pins `GGML_CPU_ARM_ARCH=armv8-a` and drops dotprod.

`whisper.cpp` has none of this — `whisper-rs-sys` passes no `target-cpu` handling, so ggml's own default applies and it builds native already. So the untuned case is not "both a little slow". It is the transcriber tuned and the embedder not, and embedding is the stage measured in days.

The cost is that the binary is then built for the CPU that built it and must not be copied to an older one. `--portable` is the way out, and it leaves `RUSTFLAGS` alone — so a middle tier that still carries AVX2 and FMA but runs on anything since about 2013 is:

```bash
RUSTFLAGS="-C target-cpu=x86-64-v3" ./install.sh --portable
```

### By hand

**Centinel is two binaries, and you need both.** Without a clone, one command does both:

```bash
cargo install --git https://github.com/bennyhodl/centinel centinel centinel-whisper
```

From a clone it is two commands — cargo cannot take two `--path` arguments, and the workspace root is a virtual manifest:

```bash
cargo install --path crates/centinel
cargo install --path crates/centinel-whisper
```

Both packages compile a C++ library from source, so the first build is long either way. Any GPU backend other than Metal is a `--features` flag on **each** of the two commands: passing `--features cuda` to one binary and not the other leaves half the pipeline on the CPU.

### Why two

`whisper.cpp` and `llama.cpp` each vendor their own copy of **`ggml`**, and both export the same ~534 `ggml_*` symbols. Linked into one binary the linker keeps one copy and silently resolves the other library's calls to it. The two versions are not the same. Measured on identical audio and model, the linked crates the only variable:

| binary | result |
|---|---|
| `whisper-rs` alone | 2 segments — *"The council meeting will come to order."* |
| `whisper-rs` + `llama-cpp-2` | **0 segments**, every token at `p=0.000` |

It links without a warning, runs without a crash, and transcribes nothing. There is no error to catch. So `centinel` links `llama.cpp`, `centinel-whisper` links `whisper.cpp`, and the two meet over a pipe. [`docs/SPEC.md`](docs/SPEC.md) §3.6 has the full reasoning and the alternatives it rejected.

`centinel` finds the worker beside itself first, then `$CENTINEL_WHISPER_BIN`, then `PATH`. Installing both with the same command puts them in the same directory, which is all it needs — and the install script checks they landed there rather than trusting that they did.

### External tools

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

The install script reports whichever of the two is missing, and installs them itself under `--deps`.

Keep `yt-dlp` current. It ships releases in emergency clusters when YouTube changes, and `centinel doctor` warns once yours is past ninety days.

### Then

```bash
centinel doctor         # what this machine is missing, and the command for each gap
centinel models pull    # weights for search and transcription
```

`doctor` is the check to run first and after every step. It names the fix beside each gap it finds.

[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) has the quickstart — adding a source, collecting it, and searching what comes back.

---

*MIT licensed. Fork this for your city.*
