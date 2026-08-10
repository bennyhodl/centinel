# Centinel

*A civic transparency toolkit — built on the warnings of a Pennsylvania watchman.*

Centinel collects the public record of a city — website maps, documents, transcripts, and
the changes to all of them over time — and keeps it in a form nobody can quietly edit.

Everything runs on your machine. No document, no transcript, no page leaves it.

---

## Who this book is for

**You want to search a corpus somebody else collected.** Read
[What it does](start/tldr.md), then [Searching](use/search.md) and
[Reading a result](use/read.md). Two pages.

**You want to collect a city.** Read [Install](start/install.md),
[Your first corpus](start/first-corpus.md), then the *Operate it* part. That is the
operator's path: name a source, run it, keep it running, know what broke.

**You want to change the code, or trust it.** The *How it works* part walks the pipeline
one stage at a time, from an address to a cited passage. Each page says what the stage
does, what it refuses to do, and why.

**You want the settled reasoning.** This book is a guide. The specifications it was
written from are in [`docs/`](reference/further-reading.md) and go much deeper.

---

## The three principles

**Documents over promises.** Every byte is content-addressed. The hash covers the raw
bytes as served — not a summary, not a re-render, not a cleaned-up copy. Reading a
document back verifies that hash, so an edit in place is an error rather than a silent
success.

**Never trust memory.** Files on disk are the only truth. Every index, every database,
every embedding is derived and rebuildable. Nothing in this system answers from recall,
because there is nothing to recall from. There is only the record, read again.

**Notice what disappears.** Every version is kept, and every collection run is a full
snapshot — so a page that vanishes is a fact the archive holds, not a gap it forgets. A
page that *starts refusing you* is a different fact, recorded differently.

---

> *"The federal government will... necessarily absorb the state legislatures."*
> — Centinel, 1787

Samuel Bryan wrote twenty-four essays under that name, warning that distance from the
people is itself a form of tyranny. He was right. He had the wrong scale. The government
that answers to no one is the one across the street, because no one is watching.

You are the freeman now. The watchman's seat is empty. This is what fills it.
