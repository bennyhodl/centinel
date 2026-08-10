# Reading a result

The rule: **anything Centinel prints, Centinel takes back.**

A citation is only useful if the form on the screen is the form you can type. Printing an
identifier the tool then refuses is worse than printing nothing, because it looks like it
worked. So `search`, `read` and `open` all lead their provenance line with a **handle** —
the short blob hash — and all three accept it back by prefix, git-style.

```bash
centinel read 3f9a2c1          # the extracted text
centinel open 3f9a2c1          # the original document, in an application
```

## `read`

Prints the derived text: what an extractor made of the bytes. This is the text that was
chunked, indexed and embedded, so it is the text a search actually matched against.

Reading it back is the fastest way to answer *why did this result look like that*. If the
extracted text is a navigation menu, you are looking at a page whose content lives
somewhere the reader did not reach — see [Reading a document](../internals/extract.md).

## `open`

Hands the **original** file to an application. Configure which one per content kind:

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

`open`'s launcher is the one child process Centinel does not kill when its caller exits —
it may be somebody's editor, so it takes the terminal and waits. Every other external
program is bounded by a deadline and dies with the process that started it.

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

Next: [The machine](../operate/doctor.md).
