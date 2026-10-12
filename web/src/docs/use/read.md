# Reading a document

The rule: **anything Centinel prints, Centinel takes back.**

A citation is only useful if the form on the screen is the form you can type. Printing an
identifier the tool then refuses is worse than printing nothing, because it looks like it
worked. So every surface leads its provenance with a **handle**, the short blob hash, and
every surface that reads a document accepts it back by prefix, git-style.

## In the workspace

Click any result, in Search or in a Runs table, and the reader opens. It shows the document
as it was collected, and the text that was extracted from it, and says which tool did the
extracting.

The tabs depend on what the document is:

| Kind | Tabs | What you see |
|---|---|---|
| PDF | **Original**, **Text** | The PDF in the browser's own viewer. |
| Web page | **Page**, **Source**, **Text** | The page as served, in a frame that runs no script. Then its HTML, line by line. |
| CSV | **Table**, **Text** | A table with a row filter. The first thousand rows show; download the file for all of them. |
| Anything else | **Text** | The extracted text. Download the original to open it elsewhere. |

**Text** is what was chunked, indexed and embedded, so it is the text a search matched
against. If it is a navigation menu, you are looking at a page whose content lives somewhere
the reader did not reach. See [Reading a document](../internals/extract.md) for why, and what
the tool does about it.

At the top right, **Open source** goes to the live address, and **Download** saves the
original bytes, exactly as served. Collected HTML is another site's page, so it is sandboxed
and never runs on the workspace's origin.

The side panel is the provenance:

- **Blob**, the short hash of the original bytes. This is the handle.
- **Text extracted by**, the tool and version that made the text.
- **Collected**, when it was observed.

Two buttons act on the document:

- **Classify** asks every saved question about this one document and opens the run in
  [Runs](runs.md). It is disabled until there is at least one saved question. See
  [Classify](classify.md).
- **Restore usage** puts a document back in search after a classifier or a person excluded
  it. It records a person's word, and a later run does not take it away again.

## At a terminal

```bash
centinel read 3f9a2c1          # the extracted text
centinel open 3f9a2c1          # the original document, in an application
centinel download 3f9a2c1      # the original bytes, base64-encoded, 32 KiB at a time
```

All three take the handle by prefix. `read` returns the derived text. `download` returns the
original, which is what an agent gets from MCP. It returns 32 KiB per call by default,
because a larger answer is refused whole by most MCP clients. Page through with `--offset`,
or pass `--max-bytes 0` for the whole file. In the workspace, the reader's **Download** does
this for you.

`open` hands the **original** file to an application. Configure which one per content kind:

```toml
[open]
# Either an application name, or a command template containing {path}.
#   pdf      = "Adobe Acrobat"
#   markdown = "Obsidian"
#   html     = "Safari"
#   text     = "nvim {path}"
#
# "system" hands the file to the OS default handler.
default = "system"
```

`open` acts on the machine, so it exists only for a person at that machine. It is not an MCP
tool or an HTTP route. Its launcher is also the one child process Centinel does not kill when
its caller exits — it may be somebody's editor, so it takes the terminal and waits. Every
other external program is bounded by a deadline and dies with the process that started it.

## Original versus derived

Both are addressable, and they are not the same thing.

| | |
|---|---|
| **original blob** | the bytes as served. An Observation. Evidence. |
| **derived blob** | what an extraction or a transcription produced from them. Not an Observation — no server ever served it. |

A search result carries both hashes, because they answer different questions. `blob_sha`
is what the server gave us and what an archive is for. `derived_sha` is what the character
span indexes into — without it the span is uninterpretable, because it is an offset into
one particular extraction and nothing else in the result says which.

Both are valid targets for `read` and `open`. Resolving a derived hash means finding the
Observation it was derived *from* and saying so.

## Reading verifies

`read` and `open` fetch the whole blob and check that it still hashes to its address. An
edit in place is an error, not a silent success. That is the point of content-addressing,
and it is why classification — which only ever looks at the first few kilobytes — uses a
different, unverified read path. A partial read cannot be checked against a whole-file
digest, so anything shown to a person or written back into the record uses the whole one.

## Where the files are

The store mirrors the URLs under `current/<source>/`, so a corpus is browsable with
ordinary tools. That tree is **derived** — it is rebuildable from `blobs/` and `log/`, and
deleting it costs minutes. The evidence is the blob pool and the log.

Next: [Classify](classify.md).
