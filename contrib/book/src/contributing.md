# Contributing

Centinel is MIT licensed and meant to be forked for your city. If you would rather push
than fork, these are the eight places where help is actually wanted, roughly in the order
they would change what the project can do.

Every one of them names what exists today, what is missing, and the constraint a fix has to
survive. That last part matters more than usual here: most of this system's rules exist
because the alternative silently recorded something false, and a contribution that
optimises one of them away is a regression that no test catches.

---

## 1. Authentication for MCP and HTTP

**Today:** there is none. `centinel serve` binds `127.0.0.1` and logs a warning rather than
silently exposing the store when told to bind anything else. Ops carry a **reach** —
`Public`, `Operator`, `Host` — and the remote surfaces enforce it by refusing to route
anything that is not `Public`. A remote caller cannot collect, cannot add a source, cannot
pull a model.

**Missing:** everything that would let a node serve a corpus to someone who is not sitting
at the machine. Loopback-only is a placeholder standing in for a decision nobody has made.

**Where to start:** the reach model in `crates/centinel-core/src/op.rs`, and the HTTP and
MCP surfaces that read it. See [Ops](internals/ops.md) and the reach table in
[User](start/user.md).

**The constraint:** a scheme here is deliberately unspecified because inventing one would
foreclose the decision. Two things it must not do — collapse the reach distinction into
"authenticated callers may do anything", and introduce a *second* node identity. Federation
has already settled on one key per node, and an auth design that grows its own keypair
makes two things to lose.

---

## 2. The CLI render engine

**Today:** `crates/centinel/src/progress.rs` and `crates/centinel-core/src/render.rs`. Two
renderers, chosen by whether stderr is a terminal: `indicatif` bars for a person, one line
per event for a pipe or a CI log. Finished items scroll past as history while a tally stays
pinned underneath, because a crawl paced at one request per second is asleep almost all of
the time and a run that printed only a counter was indistinguishable from one that had
hung.

**Missing:** it has teeth. The pinned footer and the scrolling log disagree about how many
rows they own, so moving the display in and out of view breaks the lines — and a resize, or
a long URL that wraps, gets the redraw counting rows that are no longer where it left them.
There is monkey-patching in there holding it together.

**Where to start:** the event stream is the good part and should survive — an `ItemOutcome`
is stage-agnostic, which is why `extract` gets `collect`'s display for free. The bug is in
the drawing, not the model.

**The constraint:** the terminal renders **the same erased JSON the HTTP route returns**. A
person may never be shown a field a machine would not get. Whatever replaces the drawing
code keeps that, and keeps the isatty split — bars in a CI log are 1,200 redraw lines
nobody wants.

---

## 3. Federation over Iroh

**The idea.** Centinel runs **one node per city** — an Agartha node, a Valhalla node, hundreds
more, each collecting and keeping its own corpus locally. Federation is how those nodes
share without anybody running a server in the middle.

A node offers **slices** of what it holds. Another node asks for the ones it wants, verifies
the bytes against their hashes, and keeps them as a **foreign store** — read-only, and
searchable beside its own corpus. Peers exchange signed messages to arrange it — *I would
like to peer*, *this is what I hold*, *send me this slice*, *I am restoring, this is me* —
and **Iroh** moves the bytes underneath.

That one transfer buys two things that look like separate features:

- **Durability.** Ten million documents on one machine is one machine away from gone. An
  archive that cannot survive its own disk is not an archive, and a peer holding a copy is
  the backup.
- **Cross-pollination.** A question about procurement across a region should reach
  Agartha, Valhalla and the county between them without one operator having to own all
  three.

At six hundred cities the whole is **terabytes**, so pulling everything is not the
mechanism. Slices are.

**Today:** a specification with **three of twelve decisions locked** — the node key, the
transport, and what a slice is. The reasoning, the holes, and the ticket that fills each
hole are in
[**`docs/FEDERATION.md`**](https://github.com/bennyhodl/centinel/blob/master/docs/FEDERATION.md).

**Missing:** most of it, and honestly so. Every locked decision was made *ahead of its
verification*, and the spec says so at the top. Nothing there is buildable yet.

**Where to start** — this is the area with the most open design and the least written code:

| Issue | |
|---|---|
| [#19](https://github.com/bennyhodl/centinel/issues/19) | MAP: federation — sharing, durability and restore between city nodes |
| [#21](https://github.com/bennyhodl/centinel/issues/21) | **Verify Iroh** — range streaming, set requests, embedding, relays, scale |
| [#20](https://github.com/bennyhodl/centinel/issues/20) | Prior art: Syncthing, Hypercore, Radicle |
| [#24](https://github.com/bennyhodl/centinel/issues/24) | Node identity and the pact between two operators |
| [#25](https://github.com/bennyhodl/centinel/issues/25) | What travels, exactly: payload shape and the provenance of derived text |
| [#26](https://github.com/bennyhodl/centinel/issues/26) | The foreign store: layout, read-only semantics, cross-store retrieval |
| [#27](https://github.com/bennyhodl/centinel/issues/27) | Discovery: what does a peer hold, cheaply and verifiably |
| [#28](https://github.com/bennyhodl/centinel/issues/28) | Restore: rebuilding a dead node from its peers |
| [#30](https://github.com/bennyhodl/centinel/issues/30) | The peer message protocol: signed envelopes, sessions, what peers say |
| [#31](https://github.com/bennyhodl/centinel/issues/31) | Is federation git-shaped? Have/want negotiation, signed commits |

[#21](https://github.com/bennyhodl/centinel/issues/21) is the one that unblocks the rest.
The transport is locked on Iroh with **no evidence yet**, and a measurement that says it
cannot do range streaming at this scale is worth more to this project than a month of
implementation.

Three more are settled and two are parked — see
[Research and decisions](#research-and-decisions) below.

**The constraint:** every constraint in
[`docs/SPEC.md`](https://github.com/bennyhodl/centinel/blob/master/docs/SPEC.md) binds here
and is not reopened. A
peer holding your corpus holds it as a foreign store it can read — there is no
blind-custodian role — and nobody is obliged to hold anything. A Source no other operator
finds interesting is a Source with one copy, and the design has to say that out loud rather
than let restore imply a guarantee it does not provide.

---

## 4. Chunking strategies

**Today:** markdown-aware seams with a **static size**. Headings and paragraphs are the
preferred breaks, and the heading path is carried into the chunk text so the embedder is
not guessing at context. But the numbers are uniform: 1,200 characters target, 150
overlap, 80 minimum — the same for a council transcript, a CSV of court filings, and a
one-page proclamation.

**Missing:** any reason to believe 1,200 is right for a spreadsheet row, or that a
transcript should be cut on paragraphs rather than on speaker turns and timestamps. This is
the most obviously improvable part of retrieval and the least defended by evidence.
[#38](https://github.com/bennyhodl/centinel/issues/38) covers the adjacent question of what
an extracted artifact should look like — spreadsheet chunks, page anchors.

**Where to start:** `crates/centinel-core/src/chunk.rs`, and
[Chunking and the index](internals/index.md).

**The constraint:** **a chunk's identity is the hash of its text.** Not its position, not
its document, not a row id. Two things fall out of that and both must survive any new
strategy — re-collecting a page whose footer changed re-chunks it, but every unchanged
chunk hashes the same, so only genuinely new text is ever embedded; and boilerplate on a
thousand pages is one chunk with a thousand placements rather than a thousand
near-identical vectors crowding the index. A strategy that makes chunk boundaries depend on
document position throws both away, and the bill arrives as a re-embed of the whole corpus.

Also: an over-long chunk is **refused, not truncated**. A shortened chunk stored under a
hash covering text that was never embedded makes the record lie about what it holds.

---

## 5. QA feedback

**Today:** the most valuable contribution that requires no Rust. Point Centinel at a city
you know and tell us where it fails.

**What is useful, specifically:**

- **A site that will not extract.** Run `centinel check <url>` and paste the output. That
  transcript shows the reader that won, the characters it produced, and whether an
  enclosure was found — which is usually the whole diagnosis.
- **A search that misses something you know is in the corpus.** Include `method`,
  `total_chunks_indexed` and `vectors_indexed` from the result envelope, because half of
  these turn out to be a corpus that was never embedded.
- **A host nothing recognises.** `centinel investigate <url>` printing nothing is a real
  finding, not a dead end.
- **A vendor product seen in the wild.** Recognising Hyland OnBase collects every city
  running OnBase; recognising Agartha collects Agartha.

**Where it goes:** `docs/FIELD-NOTES.md`. Catalogue the site before proposing a strategy —
**a shape earns a lever at two sightings, not one.** One weird city is an anecdote; the same
shape in two counties is a product.

**The constraint on scope:** the corpus is what a government published about itself. A
newsroom's account of a council meeting carries a frame the text does not mark, so news
organisations are out of scope even when they are the better write-up.

---

## 6. Benchmarking

**Today:** ad-hoc. The claims this project makes about retrieval — that reranking is worth
more than either retriever, that reranked BM25 measures more than twice as good as raw BM25
— came from measurements taken once, by hand, and not from anything you can re-run.

**Missing:** a repeatable harness over a known corpus with known answers, so that a change
to chunking, to the embedder tier, or to the fusion constant can be judged instead of
argued. Right now a chunking contribution (§4) has nothing to prove itself against, which is
a large part of why nobody has made one.

**The constraint:** the interesting question is not *did retrieval score well*. It is
**how much of what a city published did we end up holding, and is it the part that
mattered.** A benchmark that only measures ranking quality over documents already collected
will happily give full marks to a corpus of navigation menus.

---

## 7. An evaluation pass over documents

**Today:** nothing scores anything. Collection is indiscriminate on purpose — everything a
source declares is fetched, and a great deal of it is junk.

**Missing:** a usefulness or relevance signal, so that a search over 400,000 chunks is not
weighted the same for a budget ordinance and a page announcing a road closure in 2018.

**The constraint, and it is the sharp one:** agents are clients of the record, **never its
author**. A score is a *derived annotation*, and it lives under the same rules as every
other derivation:

- It must not gate collection. What gets collected cannot depend on what a model thought
  that day, or the archive becomes a record of the model's opinions.
- It must not delete or hide anything. The junk stays; a low score changes ranking, not
  existence.
- It must record its provenance — the model, the version, the tier — exactly as an
  extractor does, because output quality varies by machine and a score from a 0.6B model
  on a laptop is not the same claim as one from a 4B model on a workstation.

Get that layering right and this is one of the highest-leverage items on the list. Get it
wrong and it quietly turns the archive into an editorialised one.

---

## 8. People, and a rolodex

**Today:** the record is addresses and documents. A name appearing in four hundred
documents is four hundred unrelated strings.

**Missing:** an index of people — who signs the proclamations, who chairs the committee, who
the contract went to — so a question about a person is one lookup instead of four hundred
full-text hits. Email addresses are the obvious spine, since a government publishes staff
directories about itself.

**The constraint:** **a Resource is an address, not a thing in the world.** The same meeting
reachable four ways is four Resources, and the model makes no claim they are related —
identity resolution is deliberately not attempted, because four honest rows beat one
confident wrong one. A rolodex *is* identity resolution, so it has to be built as a derived
layer with its own confidence and its own provenance, sitting beside the record rather than
collapsing it. The moment "these two names are the same person" is written into the
evidence layer, the archive is making a claim it cannot support.

Scope it to what the government published about itself, and it is a public record. Scope it
wider and it is something else.

---

## 9. Change detection, and which version is current

The largest gap between what the specification promises and what the code does. Centinel's
one-line description says it keeps *the changes to all of them over time* — and no code path
delivers that today.

**Today: the vocabulary exists and nothing constructs it.**

Storage already splits hashing in two, which is the hard part and it is done:

| | Computed over | Used for |
|---|---|---|
| `blob_sha` | **raw bytes** | archive identity, the filename in the pool, evidentiary fidelity |
| `Fingerprint` | **normalized content** | *did this meaningfully change?* |

A `Fingerprint` is written on every Observation, and `store::observe` hands back the
previous one for exactly this comparison. `acquire` uses it at the moment of a fetch. That
is the mechanism working, once, at the narrowest point.

Everything above that is modelled and unexercised. `ChangeEvent` is a real type —
`resource`, `kind`, `at`, `from_fingerprint`, `to_fingerprint`, with
`Appeared` / `Modified` / `Vanished` — and it is constructed **nowhere**: no table, no log
record kind, no verb. `ChangeSignal` models the distinction between a vendor that *asserts*
a change and a crawl that *computes* one, and every Source returns the default, `Unknown`.
`Vanished` falls out of comparing two `DiscoveryRun` snapshots, and nothing compares them.

So the corpus retains every version of every page and cannot answer a question about the
difference between two of them.

**And normalization is a placeholder that says so.** `normalize_placeholder` collapses
ASCII whitespace and trims, and its own doc comment refuses to pretend otherwise. A
rotating banner or a *last updated* stamp still moves the fingerprint, so the signal fires
on cosmetic edits. That error runs in the safe direction — a false positive, not a silent
miss — but it is the wrong default at corpus scale.

### Which document is active

The second half, and the one a user notices first.

The index retains every version **by construction**: `placement`'s primary key includes
`derived_sha`, so a page that changes writes rows beside the old ones and nothing removes
them. That retention is correct and should stay. What is missing is the *mark*.

The consequence is that BM25 and the vector arm both rank last year's text beside this
year's, fuse them, and hand back two results that read identically. `observed_at` does not
rescue it — the index fold deliberately keeps the **earliest** observation of those bytes
at that address so it does not churn on every run. That is first-seen, not currency.

It is an error shape this codebase already names twice: `pages_needing_ocr` is a claim about
what a reader could decode, not about what the page holds; the vector share exists because a
rank says nothing about the size of the pool it came from. This is the third instance — a
fact about what has been **processed**, sitting where a reader will take it for a fact about
the world.

**Shape of the fix:** currency written at index time, `search` defaulting to current, and
`--as-of` / `--all-versions` to reach the rest. Deletion is not on the table; the point of
the archive is that the old version is still there.

### Why it cannot be solved by better retrieval

The [VersionRAG paper](https://arxiv.org/html/2510.08109v1) (arXiv 2510.08109, Oct 2025)
names the two failure modes and measures them:

| | Naive RAG | GraphRAG | VersionRAG |
|---|:--:|:--:|:--:|
| Version-specific queries | 55% | 100% | 100% |
| Change retrieval | 25% | 30% | 70% |
| **Implicit changes** | **0%** | 10% | 60% |

**Version conflation** is the first row: an index with no temporal discrimination returns
similar chunks from several versions and mixes three years of contradictory policy into one
answer. **Implicit change tracking** is the last, and the zero is the important number —
*when did this requirement disappear?* is unanswerable by similarity search, because **the
absence of text has no embedding**. No reranker recovers it. Change has to be an indexed
object or the question cannot be asked, which is why `ChangeEvent` is specified as a
rebuildable index rather than left implicit in the log.

### What is actually undecided

- **What normalization strips**, and whether the rules are per Source. Rotating banners,
  *last updated* stamps, CSRF tokens, session ids, view counters, asset hashes in URLs. A
  Drupal site and a Legistar portal do not want the same rules. **False negatives are the
  dangerous direction** — over-aggressive normalization silently suppresses a real change,
  and for a transparency tool a silent miss is the worst failure available.
- **Whether a vendor's assertion is trusted.** The Legistar API exposes
  `MatterLastModifiedUtc`, server-side filterable, and an opaque `MatterRowVersion`. One
  query replaces a full recrawl — but a vendor that fails to bump a timestamp on edit
  produces exactly that silent miss. The middle path is to trust the feed to choose *what to
  re-fetch* and still hash what comes back to confirm it.
- **Whether re-derivation is suppressed as a change.** When extraction changed to include
  titles, every document re-derived to a different derived hash from unchanged source bytes.
  `Derivation` records tool, version and model tier, which is what makes that *recoverable* —
  nothing currently uses it to suppress a false change.
- **When `Live` becomes `Gone`**, and it has to survive a source that refuses rather than
  answers. YouTube's bot wall would have written forty false disappearances into the record
  in a single run.

**The constraint:** change is computed over **source bytes**, never over extracted text.
Hash the extracted text and every extractor upgrade reports a phantom change across the
whole corpus — the source did not move, the reader did. And a `ChangeEvent` is a
*rebuildable index*, not evidence: the truth is `obs[n-1].fingerprint != obs[n].fingerprint`
in the log, and the table exists so search can retrieve over changes.

Ticket: [#7](https://github.com/bennyhodl/centinel/issues/7), which carries the full
history — including the two findings that forced the asserted-versus-computed split.

---

## Research and decisions

Centinel's decisions are argued in the issue tracker before they reach a specification, and
those issues stay as the reasoning. Four kinds:

| Kind | |
|---|---|
| **map** | the umbrella for a whole body of work, linking everything under it |
| **grilling** | one decision, pressure-tested until it can be written down |
| **research** | a survey of evidence, with citations, settling nothing by itself |
| **task** | build the thing the grilling settled |

Mostly research and decisions rather than tickets you can pick up and close. They are worth
having on the radar anyway — an open grilling is an invitation to argue, and a closed one is
where the *why* lives before it is compressed into `docs/SPEC.md`.

Two maps: [**#1**](https://github.com/bennyhodl/centinel/issues/1) is the collection
toolkit, [**#19**](https://github.com/bennyhodl/centinel/issues/19) is federation. The
federation cluster is in [§3](#3-federation-over-iroh) above; the rest is here.

### Still open

| Issue | |
|---|---|
| [#7](https://github.com/bennyhodl/centinel/issues/7) | Change detection, discovery deltas and scheduling semantics — what counts as a change. [§9](#9-change-detection-and-which-version-is-current) above is the standing summary |
| [#8](https://github.com/bennyhodl/centinel/issues/8) | YouTube as a first-class Source: archive, transcripts, Whisper fallback |
| [#11](https://github.com/bennyhodl/centinel/issues/11) | Distribution and packaging — how Centinel ships, given two binaries and a C++ toolchain |
| [#13](https://github.com/bennyhodl/centinel/issues/13) | Hardware profiling and local model tier selection — picking a tier for the machine it lands on |
| [#33](https://github.com/bennyhodl/centinel/issues/33) | Images in PDFs: an OCR strategy for the 322 that hold no text |
| [#36](https://github.com/bennyhodl/centinel/issues/36) | Vendor APIs as a Source type: Legistar, Granicus, Municode, ArcGIS |
| [#37](https://github.com/bennyhodl/centinel/issues/37) | What counts as one site — boundary, vendor portals, and pages a crawler cannot read |
| [#38](https://github.com/bennyhodl/centinel/issues/38) | The shape of an extracted artifact: spreadsheet chunks, page anchors, encoding thresholds |

[#37](https://github.com/bennyhodl/centinel/issues/37) and
[#36](https://github.com/bennyhodl/centinel/issues/36) are the two that most change what a
corpus can contain — they are the crumb question and the vendor-portal question, and today
both end at *the operator decides*.

### Settled — the reasoning behind what is already built

Closed, but not dead. Each one carries the argument that a line in the spec is the
compression of.

| Issue | |
|---|---|
| [#2](https://github.com/bennyhodl/centinel/issues/2) · [#5](https://github.com/bennyhodl/centinel/issues/5) | Capability survey and the language choice — Rust vs Python vs TypeScript, then the runtime |
| [#3](https://github.com/bennyhodl/centinel/issues/3) | Core domain model: the nouns of collection. `CONTEXT.md` is what this became |
| [#4](https://github.com/bennyhodl/centinel/issues/4) | Crawl scope, boundary and politeness policy for `.gov` targets |
| [#6](https://github.com/bennyhodl/centinel/issues/6) | Storage, content-addressing and version history |
| [#9](https://github.com/bennyhodl/centinel/issues/9) | Single definition → CLI, MCP and HTTP, generated rather than listed |
| [#10](https://github.com/bennyhodl/centinel/issues/10) | Search and retrieval: vector store, chunking, embeddings, reranking |
| [#12](https://github.com/bennyhodl/centinel/issues/12) | Document extraction pipeline: routing, OCR fallback, artifact shape |
| [#39](https://github.com/bennyhodl/centinel/issues/39) | Build the second retrieval arm — the task that shipped vector search, RRF and always-on rerank |

Federation's settled three: [#23](https://github.com/bennyhodl/centinel/issues/23) picked
the transport, [#29](https://github.com/bennyhodl/centinel/issues/29) settled one key per
node backed up by the operator, and [#22](https://github.com/bennyhodl/centinel/issues/22)
settled that a slice is *(chapter, source ids)* and a Source is indivisible.
[#32](https://github.com/bennyhodl/centinel/issues/32) — key recovery beyond one key per
node — is open and deliberately **deferred**.

## Working here

Two binaries, and you need both — `centinel` links `llama.cpp`, `centinel-whisper` links
`whisper.cpp`, and they meet over a pipe because linking both into one process silently
breaks transcription. See [Install](start/install.md).

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
just book                  # this book, rebuilt on every save
```

Those first three are CI, exactly as written. Warnings are errors.

**Releases decide themselves.** A green push to master runs `Release`, which reads the
conventional commits, bumps the workspace version, writes the changelog with `git cliff`,
tags, and publishes. There is nothing to do by hand.

Prebuilt binaries are a second workflow, `Binaries`, and it is **off**. It builds two GPU
assets — Metal for Apple Silicon, CUDA 12 for x86_64 Linux — and uploads them to the tag,
which is what turns an install from half an hour of C++ into a download. It is off because
a macOS runner bills at 10x while this repository is private, so one release is about 400
minutes of the monthly allowance. Set the repository variable `RELEASE_BINARIES` to `true`
to turn it on, or start it by hand against any existing tag from the Actions tab. A release
without assets is not broken: `install.sh` asks, does not find, and builds.

**Commits are conventional**, because the changelog is read off them by `git cliff` — the
commit subject *is* the release note, and anything without a prefix is dropped rather than
guessed at. Edit the commit, not `CHANGELOG.md`.

**The three principles are load-bearing**, not decoration. Documents over promises: every
byte content-addressed, and reading verifies the hash. Never trust memory: files on disk are
the only truth, and every index is derived and rebuildable. Notice what disappears: a page
that vanishes and a page that starts refusing you are different facts, recorded
differently. If a change makes one of those cheaper to violate, say so in the PR — that is a
conversation worth having, and it is a worse one to have after it merges.

The reasoning behind the settled decisions, with their accepted costs, is in
[`docs/SPEC.md`](https://github.com/bennyhodl/centinel/blob/master/docs/SPEC.md). The
research underneath it is in
[`docs/research/`](https://github.com/bennyhodl/centinel/tree/master/docs/research) — about
3,850 lines and 450 primary-source citations.
