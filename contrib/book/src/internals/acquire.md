# Acquisition

Two verbs, one loop.

**`enumerate`** produces the complete Resource set a Source declares. A sitemap walk, a
playlist listing, a paged query. Always a full snapshot, never a delta.

**`acquire`** retrieves everything one address holds. It returns a **list**, because a
video is one address holding metadata, captions and audio. An earlier interface returned
one blob, and no adapter could implement it — that mismatch is why the trait sat
unimplemented while its job was done twice by hand.

Each returned **artifact** becomes its own Observation with its own history.

## The loop does not vary

`discover` and `collect` name what happens, not how. One loop derives the work list from
the log, turns refusals into a `ResourceStatus`, and keeps the counters — for any Source.
`sources::from_config` is the only code that picks an adapter.

So there is no `centinel youtube`, and adding a third Source kind adds no verb.

## The marker

The address whose presence in the log proves a Resource was acquired. The single line on
which resumption varies:

| Source kind | Marker |
|---|---|
| site | the page itself |
| channel | the **metadata** sub-resource |

Keying resumption on captions would re-fetch a whole catalogue every run, because about 7%
of a real council channel has none and never will. An enclosure is the same case one level
down: a page whose attachment 404s is still a page we have, and keying on the attachment
would re-fetch the page forever.

## Refusals

An acquisition that failed carries a **Liveness** rather than an error type, because the
caller's job is to record *what kind* of failure this was, not to propagate it. A WAF 403
and a 404 are the same `Err` and completely different facts. One type covers HTTP and
`yt-dlp` alike.

## Enclosures

A page can carry a document at its own address rather than containing it: the PDF a CMS
renders in a viewer, an RFQ's attached drawings. Those are found in the page's HTML,
fetched during `acquire`, and stored as their own artifacts with their own Observations
and histories.

Without this, the page enters the corpus looking collected and carrying nothing. On
`tampa.gov`, 915 of 1005 pages extracted to a date and a print notice, with the
proclamation itself at an address nothing had fetched.

**One level, same host.** The page's own HTML is scanned; what comes back is not. A second
level makes acquisition a recursive crawler with no snapshot to bound it — and that is
`enumerate`'s job, which is where a *complete* address set comes from.

A strategy that names documents rather than pages skips the scan entirely. See
[`addresses_are`](strategies.md#pages-or-documents).

## Politeness

Per host, and deliberately slow. `rps = 1.0` by default. Acquisition runs per source for
this reason, and because a 403 on one site must not stop the next.

`robots.txt` is honoured, and a robots denial is recorded as `Blocked` — refused, not
absent. A descriptive `User-Agent` measurably reduces WAF 403s.

## External programs

Every child process goes through one module. That is what makes these true of all of them
at once:

- it dies with its caller,
- it carries a deadline,
- it never reads our stdin.

Seven call sites used to make those choices separately, and all seven made none of them.

**Deadline versus stall timeout.** A deadline bounds *total* time and suits a call with a
known shape — a version probe, a metadata fetch. A stall timeout bounds *silence*, and is
the only workable guard on a job whose honest duration is hours. A transcription still
reporting progress after four hours is working; one that has said nothing for ten minutes
is wedged.

**Heartbeat.** Output that proves a child is alive. The whisper worker's stderr is both
its diagnostics and its heartbeat, which is why the stall timer resets on *any* line
rather than only on a progress report.

The one exception is `open`'s launcher. It may be somebody's editor, so it takes the
terminal and waits.

## Content kinds

One word for what a blob *is*, and deliberately coarser than its format. `document` covers
Word, PowerPoint, OpenDocument, RTF and EPUB, because extraction asks all five the same
question.

It is decided from a **4 KB head**, so it can only ever answer what the first bytes prove.
A `.docx` and a `.pptx` are both `zip-container` until something reads the ZIP central
directory at the *end* of the file. Sharpening the kind — making it say `docx` — would put
a guess in the record at the one point where nothing has read enough of the file to know,
and every stage downstream would carry the guess. The precise format is a **different
question, answered later**, by a reader holding the whole verified blob.

### The evidence order

`classify` asks, in order:

1. the **declared** type — a `content-type` a server actually sent;
2. the **magic bytes**;
3. only if both came back empty, the extension off the **served address**.

A name is the last evidence consulted, never the first. Step 3 is reached only when there
is no header worth the word — `application/octet-stream` is IIS's default for an extension
missing from its MIME map, not a claim about the content — and no evidence in the bytes,
because the formats it rescues are the ones whose first bytes are ordinary text.

Without step 3, 2.2 GB of `.csv` on one Florida clerk's file server was collected,
classified `other`, claimed by no reader, and recorded underivable in silence. What stays
forbidden is a *supplied* filename outranking a server that declared something real.

A file read off disk has no headers at all, so `check` infers a type from the extension
**and says that it did**. Presenting a guess as a header would put a filename's opinion
where the archive expects a server's.

### One table, not five

The four questions a content kind answers — *what is this*, *what would a server have
called it*, *what should the file be named*, *is it worth fetching on its own* — are all
projections of one table.

They were five tables in three modules that no compiler related to each other, so adding a
kind meant ten edits and the compiler asked for none. The arm one of them was missing is
why every caption track landed on disk as `.bin`; the one another was missing would mean
the document at the end of a link is never fetched at all. Both failures are silent, and
both look like a site that had nothing.

The word in the record stays a **string**, because the log is append-only and a store
written by a newer build holds kinds this one has never heard of.

Next: [Reading a document](extract.md).
