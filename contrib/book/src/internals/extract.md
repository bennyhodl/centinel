# Reading a document

Extraction turns collected bytes into searchable text. It dispatches on
[content kind](acquire.md#content-kinds) to an **ordered list of readers**, tried in
order.

```rust
pub fn readers_for(kind: ContentKind) -> &'static [Reader] {
    match kind {
        Html                 => &[Reader::Marked, Reader::Readability, Reader::WholePage],
        Pdf                  => &[Reader::PdfInspector, Reader::Poppler],
        Spreadsheet          => &[Reader::Spreadsheet],
        Document | ZipContainer => &[Reader::AnyDoc],
        Captions             => &[Reader::Captions],
        Text | Csv | Json | Xml => &[Reader::Passthrough],
        Markdown | Audio | Other => &[],
    }
}
```

The empty rows are deliberate. `audio` goes to [transcription](transcribe.md); `markdown`
is already derived text; `other` is bytes nothing here claims.

## The order is data

There is **one** definition of *produced nothing*, shared by every pair:

```rust
fn produced_text(outcome: &Extracted) -> bool {
    outcome.text().is_some_and(|t| !t.trim().is_empty())
}
```

That used to be three mechanisms — a `bool` for PDF, a free-text note for HTML, a re-route
for documents — which meant each pair decided for itself what failure meant. One of them
decided wrong: the code returned before the fallback whenever the primary said
`Unextractable`, which is exactly the verdict the PDF reader files for a PDF whose text
layer it cannot see. So the fallback that entry existed for was unreachable by the 168
documents it was written for.

Written once, the predicate can be wrong once. Written per pair, it is wrong per pair.

## A fallback is not a second guess

`pdf-inspector` is primary because it produces markdown, and headings become the chunk
heading path. `pdftotext` is the fallback because flat text beats none.

The reason there is a fallback at all: a page flagged `pages_needing_ocr` is a claim about
what the reader could **decode**, not about what the page **holds**. Reading the first as
the second wrote off 168 of 490 PDFs that had a text layer all along — an executive order,
signed minutes, a 315,000-character action plan.

> A fallback is not a second guess at the same question. It is the admission that the
> first tool's silence was never evidence.

`recovered_by_fallback` counts how often the second reader spoke. It counts for every kind
with a fallback, not only PDF, which is what makes the HTML pair's rate visible at all —
and it is the number to watch after any change to a primary reader.

## The marked region

The part of a page the page itself declares to be its content: `<main>`, `[role=main]`,
`#main-content`, `.main-content`, `<article>` — widest first.

It is the **first** reader for HTML, ahead of readability, because readability is a *guess*
about where the content is and this is the page's own answer. 298 of 300 measured
documents carry a marker.

That is a rule about HTML, not about a vendor, so it is a `Reader` in the list and not a
registry in front of it. It had its own registry for one commit, and that registry opted
out of every invariant the list holds: `recovered_by_fallback` was hardcoded false for the
reader that handles 99% of HTML, a recognised-but-empty read left no note, and two code
paths returned different text for the same bytes because only one of them consulted it.

A reader that answers nothing falls through — the same contract every other reader is held
to, and it needed no new mechanism to state.

## The title

A document's own name is in `<title>`, `og:title` and `<h1>`, and **nowhere in the body**.

So it is written into the extracted text as an `# H1`, not merely recorded beside it. Only
the text is searched, and as a heading it enters every chunk's heading path.

The caption extractor already followed the same rule for the same reason: a recording
titled *"Mayor Castor 2026 Budget"* never says "Castor" aloud, and a proclamation page
never says what it proclaims.

## Enclosures

A page that carries its document at a separate address is handled during
[acquisition](acquire.md#enclosures), not here. By the time extraction runs, the enclosure
is its own artifact with its own Observation, and it reads like any other PDF.

## What "nothing" is recorded as

An extraction that was attempted and produced nothing is an **Underivable**, carrying the
tool, the version, the reason and the pipeline version.

It is not the empty blob recorded as derived text. That would file the verdict as a
`Derivation` — beyond the reach of the version mechanism that exists to revisit it — and
an append-only log cannot un-write the 490 a past run already recorded. So "no bytes" is
turned into an `Underivable` at the **write site**, because every reader can get it wrong
the same way.

## Checking one document

```bash
centinel check https://host/some/document.pdf
centinel check ./local-file.docx
```

It runs the same dispatch and prints what came out. Nothing is stored.

This is the fastest way to answer *why did this page index as a navigation menu*. A file
read off disk has no `content-type`, so `check` infers one from the extension and says
that it did.

Next: [Transcription](transcribe.md).
