# Centinel

Data collection for `.gov` web surfaces and YouTube channels — website maps, documents, transcripts, and the changes to all of them over time.

A **library** first. The CLI, the HTTP server and the MCP server are thin consumers of it. Agents are clients, not the engine.

> **Status: spine.** The domain model, the store, and the CLI/MCP/HTTP derivation are built and tested. Search and retrieval is specified but **not implemented**. Seven design decisions remain open — see [`docs/SPEC.md`](docs/SPEC.md) §8.

## The idea

Files on disk are the only source of truth. Every index is derived and rebuildable:

```
<root>/
  blobs/ab/cd/abcd1234…          TRUTH    immutable, content-addressed, pooled across sources
  log/<source>/YYYY-MM.jsonl     TRUTH    append-only observations, discovery runs, status, derivations
  current/<source>/…             DERIVED  URL-mirroring tree
  cache/embeddings/              DURABLE  survives an index rebuild
  centinel.db                    DERIVED  SQLite metadata + FTS5
  index/                         DERIVED  LanceDB vectors
```

Delete everything derived and you lose minutes, not evidence. The corpus is `rsync`-able and complete on its own.

Two properties this buys, both load-bearing:

- **Two hashes.** `blob_sha` covers raw bytes and proves what the server actually served. `fingerprint` covers normalized content and answers whether anything *meaningfully* changed. A rotated CSRF token produces a new blob and no change event.
- **A blocked page is not a deleted page.** A CloudFront/Akamai 403 is recorded as `Blocked`, never as `Gone`. Conflating them would silently record a live page as removed — measured against real `.gov` hosts, not hypothetical.

## One definition, three surfaces

An op is an ordinary async function. Annotating it puts it on the CLI, in the MCP tool list, and at an HTTP route — with **no central registration list to update**:

```rust
/// List sources in the store with resource counts and liveness.
#[op]
pub async fn list(ctx: &Ctx, args: ListArgs) -> anyhow::Result<ListReport> { … }
```

```console
$ centinel list --max-problems 5              # CLI: flags and help from the same struct
$ curl -X POST localhost:8787/ops/list        # HTTP: JSON in, JSON out
{"jsonrpc":"2.0","method":"tools/list"}       # MCP: JSON Schema from the same struct
```

Long-running ops emit progress once and each surface renders it in its own idiom — a stderr counter on the CLI, an SSE stream over HTTP, a single return value over MCP. The op never learns who called it.

## Try it

```bash
cargo build

# What is this machine missing?
centinel doctor

# Collect. Legistar is keyless and OData-queryable — for this kind of content,
# querying beats crawling.
centinel --root ./.centinel ingest --source hillsboroughcounty \
  --url "https://webapi.legistar.com/v1/hillsboroughcounty/bodies"

# What is in the store, and what state is it in?
centinel --root ./.centinel list

# Serve it
centinel --root ./.centinel serve          # HTTP + MCP over HTTP
centinel --root ./.centinel mcp            # MCP over stdio
```

Re-run `ingest` on an unchanged URL: it stores the observation, dedupes the blob, and reports `changed: false`.

## Layout

```
crates/
  centinel-core/    domain model, store, op registry, ops
  centinel-macros/  the #[op] attribute
  centinel/         the binary: CLI, HTTP, MCP
docs/
  SPEC.md           the settled specification — read this first
  research/         ~3,850 lines, ~450 primary-source citations
```

## Requirements

Rust 1.85+. Centinel shells out to standalone binaries rather than running a second language runtime:

| Binary | Needed for | Required |
|---|---|---|
| `pdftoppm` (poppler) | rasterising PDF pages for OCR — Rust cannot do this natively | yes |
| `tesseract` | OCR | yes |
| `yt-dlp` | YouTube acquisition | yes |
| `ffmpeg` | audio extraction | no |

`centinel doctor` reports what is missing. Everything runs locally — no OCR, transcription, embedding or reranking leaves the machine.

## Not built yet

Search and retrieval, crawling, YouTube, document extraction, scheduling. [`docs/SPEC.md`](docs/SPEC.md) §8 lists the seven open decisions and what each one owns; §3–§6 are settled and should not be relitigated without reopening the ticket they came from.

## License

MIT (forks-encouraged). See `LICENSE`.
