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

**Marker** — the address whose presence in the log proves a Resource was acquired. The
page itself for a site; the *metadata* sub-resource for a video. The single line on which
resumption varies. *Why it matters:* keying resumption on captions would re-fetch a whole
catalogue every run, because ~7% of a real council channel has none and never will.

**Refusal** — an acquisition that failed, carrying a `Liveness` rather than an error type.
Because the caller's job is to record *what kind* of failure this was, not to propagate
it. A WAF 403 and a 404 are the same `Err` and completely different facts. One type for
HTTP and `yt-dlp` alike.

**Note** — a line of provenance a Source wants shown, and how it should read. Lets a
report print which sitemaps were walked or which channel tabs returned nothing without the
renderer learning what a sitemap or a tab is. A new adapter explains itself through this
and edits no renderer.

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
own handle.

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
