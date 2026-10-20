<p align="center">
  <img src="assets/centinel-watchman.jpg" alt="Centinel — a 1787-era ink etching of a lone watchman on the rampart with candle, scroll, and quill, gazing over a sleeping colonial town" width="600">
</p>

*A civic transparency toolkit — built on the warnings of a Pennsylvania watchman.*

---

**[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)** — How it is built. The store, the domain model, and how one function definition becomes a CLI command, an MCP tool, and an HTTP route. Includes the quickstart.

**[docs/RETRIEVAL.md](docs/RETRIEVAL.md)** — How a question becomes a cited passage. Chunking, the two stores, the embedder and reranker, and what a result tells you about how much of the corpus it could actually see.

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

Everything runs where you point it. No document, no transcript, no page leaves the machine for a third-party API unless your config file sends it to a model server you chose — and by default that server is on `127.0.0.1`.

Bryan lost the immediate fight. Pennsylvania ratified the Constitution in December 1787. But the pressure he and his fellow dissenters generated forced the **Bill of Rights** into existence — protections the powerful had not wanted to grant.

The watchman loses individual fights. He wins the ones that matter.

Light the candle.

---

## Install

```bash
curl --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/bennyhodl/centinel/master/install.sh | sh
```

The same command installs and updates. It downloads a release binary when the latest release carries one this host can run, and builds from source when it does not. A download needs no Rust and no toolchain; a build checks for Rust, a C compiler and `protoc`, and names the command for whatever is missing rather than installing a toolchain behind your back.

A release carries one asset per platform — `aarch64-apple-darwin`, `x86_64-unknown-linux-gnu` (AVX2), `x86_64-unknown-linux-gnu-v2` (x86_64 without AVX2) and `aarch64-unknown-linux-gnu` — and none carries a GPU backend, because Centinel runs no model itself. Embedding, reranking and transcription are requests to servers you run; see [Models run on servers](#models-run-on-servers) below.

Nothing about the download is load-bearing. No asset, no release, a checksum that does not match, a binary that will not start on this host — each falls back to the build, so the worst a bad release does is cost one request. The one exception is a checksum that is published and wrong, which stops instead: that is somebody handing you a different file.

Flags go after `-s --`:

```bash
curl -sSf https://raw.githubusercontent.com/bennyhodl/centinel/master/install.sh | sh -s -- --deps
```

| | |
|---|---|
| `--build` | build from source even where a release binary would fit |
| `--download` | download, and fail rather than quietly build when this host has no asset |
| `--force` | install again when the release is the version already here |
| `--portable` | build for the baseline CPU rather than this one's, so the binary can be copied to another machine |
| `--bin-dir <dir>` | somewhere other than `~/.cargo/bin` |
| `--tag <tag>` | a released tag rather than the latest release or the default branch |
| `--deps` | install what is missing with this host's package manager: `ffmpeg` and `yt-dlp`, and the toolchain when building |
| `--no-doctor` | skip the closing `centinel doctor` |

From a clone it installs **the clone**, so a contributor testing a change installs the change:

```bash
git clone https://github.com/bennyhodl/centinel
cd centinel
./install.sh
```

Afterwards, `centinel update` is the same decision run backwards: it asks the clone this
binary was built from, then GitHub's latest release, and installs through `install.sh`
whichever way you installed. `--check` reports and stops.

### By hand

```bash
cargo install --git https://github.com/bennyhodl/centinel centinel   # or, from a clone:
cargo install --path crates/centinel
```

### Models run on servers

Centinel sends embedding, reranking and transcription to servers named in the `[inference]` block of `centinel.toml`, and stores what comes back. The tuning that makes a model fast on a given machine — GPU offload, batch shape, threads, VAD — belongs to the server, set once for that machine. Any server that speaks OpenAI's `/embeddings` and `/audio/transcriptions`, and the Cohere/Jina `/rerank`, will do:

```bash
llama-server -hf Qwen/Qwen3-Embedding-4B-GGUF:Q8_0 --embedding --pooling last --port 8080
llama-server -hf ggml-org/Qwen3-Reranker-0.6B-Q8_0-GGUF --reranking --port 8081
whisper-server -m ggml-large-v3-turbo-q8_0.bin --port 8178 \
  --inference-path /v1/audio/transcriptions --vad --vad-model ggml-silero-v5.1.2.bin
```

```toml
[inference]
base_url = "http://127.0.0.1:8080/v1"

[inference.rerank]
model = "qwen3-reranker-0.6b"
base_url = "http://127.0.0.1:8081/v1"

[inference.transcribe]
model = "whisper-large-v3-turbo"
base_url = "http://127.0.0.1:8178/v1"
```

A corpus is keyword-searchable with no server at all; the vector arm, the reranker and transcription each wait for theirs, and `centinel doctor` says which are answering. OpenRouter, or a team's shared GPU box, is a `base_url` and an `api_key_env`. [Inference servers](web/src/docs/reference/inference.md) has every key, and how to move a corpus embedded by an earlier release onto a server without re-embedding it (`centinel embed --adopt`).

### External tools

Corpus processing shells out to standalone tools. Serving the web app and remote MCP also requires Node.js 22.13+ for the managed TanStack Start authentication server.

| Binary | Needed for | Required |
|---|---|---|
| `yt-dlp` | YouTube acquisition | yes |
| `ffmpeg` | decodes audio to the 16 kHz WAV a transcription server is sent | yes |
| `pdftoppm` (poppler), `tesseract` | OCR | not yet — nothing calls them |

```bash
brew install yt-dlp ffmpeg          # macOS
sudo apt install yt-dlp ffmpeg      # Debian/Ubuntu
```

The install script reports whichever of the two is missing, and installs them itself under `--deps`.

Keep `yt-dlp` current. It ships releases in emergency clusters when YouTube changes, and `centinel doctor` warns once yours is past ninety days.

### Then

```bash
centinel doctor         # what this machine is missing, and which model servers answer
```

`doctor` is the check to run first and after every step. It names the fix beside each gap it finds.

[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) has the quickstart — adding a source, collecting it, and searching what comes back.

---

*MIT licensed. Fork this for your city.*
