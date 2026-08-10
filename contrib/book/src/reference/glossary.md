# Glossary

The words this codebase uses, and what each one is load-bearing for. A term earns a place
here when getting it wrong would produce working code that records something false.

The authoritative version is [`CONTEXT.md`](https://github.com/bennyhodl/centinel/blob/master/CONTEXT.md)
in the repository root, which goes deeper on each entry.

## Acquisition

**Source** — a trait, not an entity with a `kind` field. The one thing that varies between
a crawled website and a YouTube channel, quarantined behind `enumerate` and `acquire`.

**Adapter** — a concrete `Source`. `SiteSource` and `ChannelSource` today.

**Enumerate** — produce the complete Resource set a Source declares. Always a full
snapshot, never a delta.

**Acquire** — retrieve everything one address holds. Returns a **list**, because a video
is one address holding metadata, captions and audio.

**Artifact** — one thing retrieved, at its own address. A page is one; a video is up to
three.

**Enclosure** — a document a page carries at its own address rather than contains. One
level, same host.

**Marker** — the address whose presence in the log proves a Resource was acquired. The
page itself for a site; the *metadata* sub-resource for a video.

**Refusal** — an acquisition that failed, carrying a `Liveness` rather than an error type.

**Content kind** — one word for what a blob *is*, deliberately coarser than its format.
Decided from a 4 KB head.

**Declared vs inferred type** — a `content-type` a server sent, against one read off a
filename. Both feed classification and only the first is evidence.

**Note** — a line of provenance a Source wants shown, and how it should read.

**Crumb** — an off-host link recorded, not followed. One Source per exact host; the
operator promotes crumbs, and that is what bounds the recursion.

## The store

**Truth vs derived** — only `blobs/` and `log/` are truth. `current/`, `centinel.db` and
`vectors.lance/` are derived and rebuildable.

**Derived is not the same as cheap** — `centinel.db` is minutes; `vectors.lance/` is about
a day on a 400,000-chunk corpus.

**Replay** — one Source's log, read once and answerable many times. A snapshot: it answers
what the log said when it was read.

**Store root** — *which* store. The identity of the corpus. Defaults to `~/.centinel`.

**Head read vs whole read** — `blob_head` reads the first few kilobytes and verifies
nothing; `get_blob` reads the whole file and verifies it against its address.

**BlobSha vs Fingerprint** — the hash of the bytes *as served* (evidentiary) versus the
hash of *normalized* content (the change signal).

## The record

**Resource** — an *address*, not a thing in the world. The same meeting reachable four ways
is four Resources.

**Observation** — one **successful** acquisition, always backed by bytes. There is no
failure variant.

**Liveness** — `Live`, `Gone`, `Blocked`, `Error`.

**Blocked** — refused in a way that is *not* evidence of absence: WAF 403, 429, robots
denial, YouTube's bot wall.

**DiscoveryRun** — a full snapshot of the Resource set one enumeration observed.

**Truncated** — an enumeration that stopped on a ceiling rather than on the end of the
source. A count is printed as *at least* n wherever it is true.

**Underivable** — a derivation that was attempted and produced nothing. The peer of
`ResourceStatus` on the derivation side.

**Pipeline version** — carried on every `Underivable`. A verdict belongs to one pipeline at
one version and says nothing about the next.

## Strategies

**Strategy** — recognition and enumeration as one object. Answers *where are the
addresses*.

**Recognition** — what a strategy saw, and **on what evidence**. The operator accepts or
rejects on the evidence.

**Keyed** — what a strategy keys on: `Product`, `Framework`, `ServerDefault`, `Standard`.
There is no `Jurisdiction` variant and there will not be one.

**Specificity** — lower is more specific, and more specific wins. The difference between
collecting a site and collecting its front door.

**Pass** — one enumeration in progress: the queue, the ceilings, and what has been kept. A
strategy must not decide what a ceiling means.

**Lead** — a host nothing recognised, and what was measured about it.

## Extraction

**Primary and fallback reader** — two tools for one kind, tried in order, and the record
names whichever one spoke. A fallback is not a second guess at the same question; it is the
admission that the first tool's silence was never evidence.

**The order is data** — `readers_for` is a list per content kind, with one shared
definition of *produced nothing*.

**Marked region** — the part of a page the page itself declares to be its content:
`<main>`, `[role=main]`, `#main-content`, `.main-content`, `<article>`, widest first.

**Title** — the document's own name, written into the extracted text as an `# H1` rather
than recorded beside it, because only the text is searched.

## Retrieval

**Chunk** — a passage of derived text. Its id is the SHA-256 of the text itself.

**Chunk geometry** — the target and overlap sizes. Load-bearing far outside chunking,
because the geometry decides the text and the text decides the hash.

**Placement** — where a chunk sits: which address, which derived blob, which character
span. The address is part of its identity.

**Write batch** — the rows `index` commits as a unit. One document, because that is the
unit the skip predicate subtracts.

**Arm** — one retriever feeding the fusion. There are two: BM25 over FTS5, and cosine over
LanceDB.

**Rank vs pool** — a rank is a position *inside* a set and says nothing about the size of
that set.

**Method** — the name of the pipeline that produced this ordering, assembled from what
actually ran.

**Always on vs always available** — there is no flag that silently returns worse results.
That is not a promise that a machine with no weights refuses to search.

**Handle** — a hash that identifies one blob *and* that the tool will accept back, by
prefix.

**Original vs derived blob** — the bytes as served versus what an extraction or a
transcription produced from them. Both addressable; only the first is an Observation.

## The run report

**Tally** — the numbers one stage produced, folded across however many calls it took.

**count vs total** — work *this run* did (two calls add) versus what the store now *holds*
(the last answer wins).

**Partial failure** — a corpus-wide stage where some targets failed and others did not.
Still a failure, and it keeps the numbers of the calls that worked.

**Summary vs error** — the line a person reads, against every failure joined for a machine.

## Host

**Need** — what a missing binary costs: `required`, `optional`, or `planned`.

**Gate** — a pipeline stage that a set of weights blocks: search, or transcription. Rolled
up per role.

**Stale** — a binary that is present and working but old enough that breakage is expected.
Only `yt-dlp` answers.

**Tool** — one invocation of an external program. The only way this codebase starts a child
process.

**Deadline vs stall timeout** — a deadline bounds *total* time; a stall timeout bounds
*silence*, and is the only workable guard on a job whose honest duration is hours.

**Heartbeat** — output that proves a child is alive.

**Reach** — who may cause an op to run: `Public`, `Operator`, `Host`.
