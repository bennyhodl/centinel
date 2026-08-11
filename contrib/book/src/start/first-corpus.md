# Your first corpus

Ten minutes, one city, and a search that returns something.

## 1. Check the machine

```bash
centinel doctor
```

It prints the store root it opened, the config file that named it, which binaries are
present, and which model weights each pipeline stage is waiting on. Fix what it names
before going further. A missing model does not stop collection — the stage is skipped and
resumes on a later run — but a missing `yt-dlp` stops a channel dead.

## 2. Look before you collect

Point `investigate` at a host and it will tell you whether anything recognises it, and on
what evidence.

```bash
centinel investigate https://www.valhallaclerk.com/
```

Nothing is stored. It is a question, and it costs a couple of dozen requests. The last line
it prints is the `source add` command, already filled in. A full transcript, read block by
block, is on [Operator](operator.md).

Three answers are possible: a strategy with its evidence; a set of **crumbs**, meaning the
system you want is on another host; or nothing, said plainly. All three are useful. See
[Strategies](../internals/strategies.md) for what recognition is and why the evidence is
printed rather than the verdict alone.

To ask a narrower question — *what would extraction make of this one document* — use
`centinel check <url>`. It also stores nothing.

## 3. Name a source

```bash
centinel source add agartha --site https://www.agartha.gov
```

That writes a `[[source]]` block into your config file. A YouTube channel is the same
command with a different key:

```bash
centinel source add agartha-council --channel https://www.youtube.com/@CityofAgartha
```

`site` versus `channel` is the **whole** of the website/YouTube difference. The two kinds
are peers that differ only in how they are acquired, so there is no `centinel youtube`
verb and adding a third kind would add no verb either.

## 4. Try it small

```bash
centinel run --limit 50
```

`--limit` bounds collection, not discovery. The sitemap walk still runs in full, because a
truncated snapshot of a source's address set looks exactly like a source that shrank —
that is a fact the archive must not record falsely. Fifty documents is enough to see
whether the text coming out is the page's content or the page's navigation menu.

Look at what came back:

```bash
centinel status
centinel list
centinel search "budget"
```

`status` counts what is stored, by source and content kind, and what it takes on disk — the
fifty above should be fifty documents across `html` and `pdf`. A column you did not expect,
or a size that is a hundred kilobytes when you asked for fifty PDFs, is worth a look before
you collect ten thousand more of them.

If the extracted text is a cookie banner and a menu, stop and read
[Reading a document](../internals/extract.md). Collecting ten thousand copies of a
navigation bar is the failure this project has spent the most time on.

## 5. Commit to it

```bash
centinel run
```

Every source, every stage, resumable. Interrupt it and re-run; it starts where it stopped.
Run it a second time on an unchanged corpus and it says `nothing new` in one line.

`embed` is the one stage that takes real time — on a 400,000-chunk corpus, about a day,
once. `centinel run --skip embed` stops before it, and `centinel embed` picks it up later.
The corpus is keyword-searchable long before it is embedded.

## 6. Ask it something

```console
$ centinel search "stormwater drainage fee"
```

Each result carries the passage, the address it came from, when it was observed, which
tool derived the text, and a **handle** — the short hash of the original bytes.

```bash
centinel read 3f9a2c1          # the extracted text
centinel open 3f9a2c1          # the original document, in an application
```

Both take the handle by prefix. See [Reading a result](../use/read.md).

## 7. Keep it running

```bash
centinel schedule set agartha --cron "0 3 * * *"
centinel serve
```

`serve` runs the HTTP and MCP surfaces and fires the configured schedules. Or skip the
scheduler entirely and put `centinel run` in cron — the incremental behaviour is the same
either way, because it comes from the store rather than from the runner.

---

Next: [Searching](../use/search.md) — what to put in the query, and how to read what comes
back.
