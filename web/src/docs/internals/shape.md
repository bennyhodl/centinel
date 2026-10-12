# The shape

A **library** first. The CLI, the HTTP server and the MCP server are thin consumers of it.
Agents are clients, not the engine.

```
crates/
  centinel-core/    domain model · store · config · op registry · ops · rendering
  centinel-macros/  the #[op] attribute
  centinel/         the binary: CLI, HTTP, MCP
  centinel-whisper/ the transcription worker — links whisper.cpp, and nothing else may
docs/
  SPEC.md           the settled specification
  research/         ~3,850 lines, ~450 primary-source citations
centinel.toml       what to collect, and where to keep it
~/.centinel/        the store, by default
```

## One process, six stages

```
  per source   discover → collect                      network-bound, per-host paced
  then once    extract → transcribe → index → embed    CPU-bound, model-backed
```

Each stage is a chapter in this part of the book, in that order.

## Where the variation is quarantined

Two ideas carry most of the weight, and both are about keeping a difference in one place.

**`Source` is a trait, not an entity with a `kind` field.** A crawled website and a
YouTube channel differ in `enumerate`, `acquire` and `change_signal`. Everything
downstream is one shared model. The alternative is a `kind` field and a `match` on it at
every stage, which is what the codebase actually had before the trait had implementations
— nine sites a third kind would have had to find.

**A Resource is an address, not a thing in the world.** The January 14 council meeting
reachable as a Granicus RSS item, an HTML page, a Legistar Matter and a YouTube video is
**four Resources**, and the model makes no claim they are related. Identity resolution
across access paths is fuzzy, and a wrong merge silently corrupts the record. Four honest
rows beat one confident wrong one.

The half that does *not* vary lives in `acquire`: one loop that derives the work list from
the log, turns refusals into a status, and keeps the counters — for any Source. So
`discover` and `collect` are single verbs that name what happens rather than how. There is
no `centinel youtube`, and a third Source kind adds no verb either.

## The same shape, four times

The codebase reaches one conclusion repeatedly, and it is worth naming once because
everything in this part is an instance of it:

> **A registry is a list whose elements answer for themselves. It holds no `match`.**

| Where | The list |
|---|---|
| content kinds | magic bytes: *(signature → kind)*, each element answering for itself |
| readers | `readers_for(kind)` — an ordered list per content kind, tried in order |
| strategies | each strategy tests the seed itself; nothing dispatches on a name |
| ops | link-time registration; the binary names no individual op, it iterates |

The failure mode is identical every time it is got wrong: adding a case means edits in
several places, and the compiler asks for none of them. One missing arm meant every
caption track landed on disk as `.bin`. Another meant a fallback reader was unreachable
for the 168 documents it was written for.

## Depth, and where the seams are

The interface is the test surface. `acquire`'s loop is tested through the `Source` trait
by a scripted adapter, which is how resumption, liveness-on-refusal and multi-artifact
addresses became testable without standing up HTTP or `yt-dlp`.

One adapter is a hypothetical seam; two is a real one. The `Source` trait was drawn before
the second adapter existed, and its first shape was wrong: `fetch(&Resource) -> Fetched`
had no possible implementation for a video, which is one address holding metadata,
captions and audio. So `acquire` returns a **list**.

Next: [The store](store.md).
