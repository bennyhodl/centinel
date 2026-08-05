# Domain language

The words this codebase uses, and what they are load-bearing for. The nouns come from
`docs/SPEC.md` §4; this file is where they get sharpened as the code makes them real.

A term earns a place here when getting it wrong would produce working code that records
something false.

## Acquisition

**Source** — a trait, not an entity with a `kind` field. The one thing that varies between
a crawled website and a YouTube channel, quarantined behind `enumerate` and `acquire`.
Everything downstream of acquisition is a shared model. *Why it matters:* the alternative
is a `kind` field and a `match` on it at every stage, which is what the codebase had
before the trait had implementations — nine sites that a third kind would have had to
find.

**Adapter** — a concrete `Source`. `SiteSource` and `ChannelSource` today; `ApiClient`
(Legistar/OData) is the shape they were built to leave room for. Lives in
`crates/centinel-core/src/sources/`.

**Enumerate** — produce the complete Resource set a Source declares. A sitemap walk, a
playlist listing, a paged query. Always a full snapshot, never a delta. Distinct from
"discover", which is the *verb the user types* for enumerate-and-record.

**Acquire** — retrieve everything one address holds. Returns a **list**, because a video
is one address holding metadata, captions and audio. An earlier interface returned one
blob and no adapter could implement it — that mismatch is why the trait sat unimplemented
while its job was done twice by hand.

**Artifact** (`Acquired`) — one thing retrieved, at its own address. A page is one; a
video is up to three. Each becomes its own Observation with its own history.

**Enclosure** — a document a page carries at its own address rather than contains: the PDF
a CMS renders in a viewer, an RFQ's attached drawings. Found in the page's HTML, fetched
during `acquire`, and stored as its own Artifact with its own Observation and history.
*Why it matters:* without it the page is a wrapper that enters the corpus looking collected
and carrying nothing — on `tampa.gov`, 915 of 1005 pages whose extracted text was a date
and a print notice, with the proclamation itself at an address nothing had fetched. **One
level, same host.** The page's own HTML is scanned and what comes back is not, because a
second level makes acquisition a recursive crawler with no snapshot to bound it — and that
is `enumerate`'s job, which is where a *complete* address set comes from.

**Marker** — the address whose presence in the log proves a Resource was acquired. The
page itself for a site; the *metadata* sub-resource for a video. The single line on which
resumption varies. *Why it matters:* keying resumption on captions would re-fetch a whole
catalogue every run, because ~7% of a real council channel has none and never will. An
enclosure is the same case one level down: a page whose attachment 404s is still a page we
have, and keying on the attachment would re-fetch the page forever.

**Refusal** — an acquisition that failed, carrying a `Liveness` rather than an error type.
Because the caller's job is to record *what kind* of failure this was, not to propagate
it. A WAF 403 and a 404 are the same `Err` and completely different facts. One type for
HTTP and `yt-dlp` alike.

**Content kind** — one word for what a blob *is*, and deliberately coarser than its
format. `document` covers Word, PowerPoint, OpenDocument, RTF and EPUB, because
extraction asks all five the same question. It is decided from a 4 KB head, so it can
only ever answer what the first bytes prove: a `.docx` and a `.pptx` are both
`zip-container` until something reads the ZIP central directory at the *end* of the file.
*Why it matters:* the precise format is a **different question, answered later**, by
`extract_document`, which holds the whole verified blob. Sharpening the kind instead —
making it say `docx` — would put a guess in the record at the one point where nothing has
read enough of the file to know, and every stage downstream would carry the guess.

**Note** — a line of provenance a Source wants shown, and how it should read. Lets a
report print which sitemaps were walked or which channel tabs returned nothing without the
renderer learning what a sitemap or a tab is. A new adapter explains itself through this
and edits no renderer.

## The store

**Truth vs derived** — only `blobs/` and `log/` are truth. `current/`, `centinel.db` and
the vector cache are derived and can be deleted at any time. *Why it matters:* it is what
makes the index disposable and the corpus something you can hand to somebody with `rsync`.

**Replay** — one Source's log, read once and answerable many times. Every derived view —
liveness, the latest Observation per Resource, what was derived from what — is an
in-memory scan over the records that one disk read produced. A `Replay` is a **snapshot**:
it answers what the log said when it was read, so a caller that appends and wants to see
the append takes a new one.

**The layout** — where each thing lives under the store root is named in `store` and
nowhere else. A path spelled out by a caller is a second, unenforced copy of this file's
header.

**Store root** — *which* store, and the one question `store` does not answer: it is
handed a root. `config` decides it, from `--root`/`$CENTINEL_ROOT`, then `root` in the
config file, then `~/.centinel`. *Why it matters:* the root is the identity of the
corpus, and it defaulted to `.centinel` in the **working directory** — so `centinel run`
from two directories built two corpora that shared no blobs, answered no search against
each other, and looked identical from the inside. A default in `$HOME` is what makes "the
store" a thing there is one of.

**Head read vs whole read** — `get_blob` reads the whole file and verifies it against its
address, because this is an evidentiary archive. `blob_head` reads the first few kilobytes
and verifies nothing, because a partial read cannot be checked against a whole-file digest.
Classification uses the second; anything shown to a person or written back into the record
uses the first.

## What still needs doing

Every stage computes its own work list as a **subtraction**, and none of them keeps a
checkpoint. That is what makes the pipeline resumable — and it means each stage's skip
predicate has to be exactly right, because a wrong one either redoes work forever or skips
work that was never done.

| Stage | What it subtracts | Where the answer lives |
|---|---|---|
| collect | observed markers from the latest DiscoveryRun | the log |
| extract | blobs with a Derivation **or an Underivable** from the latest Observations | the log |
| transcribe | blobs derived by the transcriber from the audio blobs | the log |
| index | **placements** already written, per address | `centinel.db` |
| embed | cached chunk hashes from indexed chunk hashes | the vector cache |

**Underivable** — a derivation that was attempted and produced nothing. The peer of
`ResourceStatus` on the derivation side, and it exists for the same reason: a `Derivation`
always has bytes, so "we tried and there was nothing to get" needs its own record. Without
it the extract predicate could only ever be "a Derivation exists", which is never true for
an audio file — so every one of them was read, hashed and re-attempted on every run.

**Pipeline version** — carried on every Underivable. A verdict belongs to one pipeline at
one version and says nothing about the next; bumping it is how a better extractor gets
another go at what an older one gave up on.

**Chunk geometry** — the target and overlap sizes chunking uses. Load-bearing far outside
chunking, because a `chunk_hash` hashes the chunk's *text* and the geometry decides the
text. Changing it produces a wholly different set of hashes, so the old chunks stay in the
index and every vector in the cache is orphaned. The index records the geometry its hashes
were built with, and refuses a change that is not a rebuild.

## The run report

**Tally** — the numbers one stage produced, folded across however many calls it took. Two
kinds of figure, and confusing them records something false:

| | | |
|---|---|---|
| **count** | work *this run* did | two calls **add** — 30 chunks + 30 chunks is 60 |
| **total** | what the store now *holds* | two calls do **not** add — the last answer wins |

`total_chunks` is the size of the whole index, so summing a three-source run's three
answers would report the index as three times its size.

**Partial failure** — a corpus-wide stage where some targets failed and others did not.
Each stage used to return on the first error, so one broken source left every source behind
it underived and unmentioned — the mistake acquisition already avoids, where one site's WAF
block does not cancel the nineteen after it. The stage is still a failure, and it keeps the
numbers of the calls that worked: half a corpus extracted is still half a corpus extracted.

**Summary vs error** — a `StageRun` carries both. The `summary` is the line a person reads
and says `1 of 19 failed`; the `error` is every failure joined, for a machine. Rendering the
second in place of the first shows one source's error as though it were the whole story.

## Retrieval

**Handle** — a hash that identifies one blob *and* that the tool will accept back. The
rule is that anything Centinel prints, Centinel takes back, by prefix, git-style. A
citation is only useful if the form on screen is the form you can type; printing an
identifier the tool then refuses is worse than printing nothing, because it looks like it
worked. `search`, `read` and `open` all lead their provenance line with one.

**Original vs derived blob** — the bytes as served versus what an extraction or a
transcription produced from them. Both are addressable. Only the first is an Observation
— no server ever served the second — which is why resolving a derived hash means finding
the Observation it was derived *from* and saying so.

**Placement** — where a chunk of text sits: which address, which derived blob, which
character span. A chunk can have several, because identical text appears under several
addresses; each placement is a separate document with its own bytes, so each carries its
own handle. *Why it matters:* **the address is part of a placement's identity, and the
derived blob is not enough on its own.** Two pages can extract to byte-identical text —
two proclamations issued the same day, once the template is stripped — and every rule that
treated one derived blob as one document lost the addresses after the first: the index key
collapsed them, and the resume predicate called them done. That is 285 of 1005 pages
collected, extracted, and absent from every search, each citing another page's URL.

**Title** — the document's own name, which on a `.gov` page is in `<title>`, `og:title`
and `<h1>` and **nowhere in the body**. So it is written into the extracted text as an
`# H1`, not merely recorded beside it: only the text is searched, and as a heading it
enters every chunk's heading path. The same rule the caption extractor already followed,
for the same reason — a recording titled *"Mayor Castor 2026 Budget"* never says "Castor"
aloud, and a proclamation page never says what it proclaims.

## Host readiness

**Need** — what a missing binary costs: `required` (code calls it and a stage stops),
`optional` (code calls it and a stage degrades), or `planned` (nothing calls it yet, and
the pipeline that will is not built). *Why it matters:* `pdftoppm` and `tesseract` were
reported as required with zero call sites between them, so a correctly installed machine
was told it was not ready. A readiness check that is wrong pessimistically is the kind
people learn to ignore.

**Gate** — a pipeline stage that a set of weights blocks: search, or transcription. Rolled
up per *role*, not per model, because the registry carries alternates and any one
installed model fills its role.

**The fix** — the command that resolves a missing dependency. `centinel models pull` is
spelled in exactly one place, `models::resolve`'s error. It was written out at seven call
sites, so renaming the command would have left six of them telling people to run something
that does not exist.

**Stale** — a binary that is present and working but old enough that breakage is expected
rather than surprising. Only `yt-dlp` answers, because it is the one dependency whose
staleness is a *predictable* failure: it warns at ninety days and ships releases in
emergency clusters.

## External programs

**Tool** — one invocation of an external program: `yt-dlp`, `ffmpeg`, the whisper worker,
whichever application opens a PDF. The only way this codebase starts a child process.
*Why it matters:* every child it starts dies with its caller, carries a deadline, and
never reads our stdin. Seven call sites used to make those choices separately, and all
seven made none of them.

**Deadline vs stall timeout** — a deadline bounds *total* time and suits a call with a
known shape: a version probe, a metadata fetch. A stall timeout bounds *silence*, and is
the only workable guard on a job whose honest duration is hours. A transcription still
reporting progress after four hours is working; one that has said nothing for ten minutes
is wedged.

**Heartbeat** — output that proves a child is alive. The whisper worker's stderr is both
its diagnostics and its heartbeat, which is why the stall timer resets on *any* line
rather than only on a progress report.

**Refusal vs transport fault** — `yt-dlp` reporting "video unavailable" is evidence about
the video. A missing binary, or a hang that had to be killed, is evidence about this
machine and says nothing about the video. Recording the second as `Gone` would mark a live
recording deleted, which is the mistake **Blocked** (below) exists to prevent one level up.

## The record

**Resource** — an *address*, not a thing in the world. The same meeting reachable four ways
is four Resources, and the model makes no claim they are related. *Why it matters:*
identity resolution across access paths is fuzzy, and a wrong merge silently corrupts the
record. Four honest rows beat one confident wrong one.

**Observation** — one **successful** acquisition, always backed by bytes. There is no
failure variant, by construction. Failures mutate `ResourceStatus` in place instead.

**Blocked** — refused in a way that is *not* evidence of absence: WAF 403, 429, robots
denial, YouTube's bot wall. Distinct from `Gone` because a CloudFront 403 would otherwise
be indistinguishable from "the page didn't change", and recording it as `Gone` would log a
live page as deleted.

**BlobSha vs Fingerprint** — the hash of the bytes *as served* (evidentiary: what the
server actually gave us) versus the hash of *normalized* content (the change signal). A
page whose only variation is a rotated CSRF token moves the first and not the second.

**DiscoveryRun** — a full snapshot of the Resource set one enumeration observed. A sitemap
*is* one of these, not a separate entity. Resources appearing and vanishing between runs
is the discovery delta. *Why it matters:* a truncated snapshot looks exactly like a source
that shrank, so nothing may silently cap one — which is why `run --limit` applies to
collection and not to discovery.

## Structure

The architecture vocabulary — **module**, **interface**, **implementation**, **depth**,
**seam**, **adapter**, **leverage**, **locality** — is defined in the `codebase-design`
skill and used here exactly as written there. In particular:

- **the interface is the test surface** — `acquire`'s loop is tested through `Source`, by a
  scripted adapter, which is how resumption, liveness-on-refusal and multi-artifact
  addresses became testable without standing up HTTP or `yt-dlp`;
- **one adapter is a hypothetical seam, two is a real one** — the `Source` trait was
  drawn before the second adapter existed, which is why its first shape was wrong.
