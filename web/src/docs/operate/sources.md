# Sources

A **Source** is one thing you collect: a website, or a YouTube channel. It is a trait in
the code, not an entity with a `kind` field, and that shows up here as one key in its
definition.

The store owns the list. Each source is defined by `log/<id>/source.toml`, beside the log
it feeds, so every surface — the CLI, the web's Sources page, MCP, the scheduler — reads
one list:

```toml
# ~/.centinel/log/agartha-council/source.toml
id                   = "agartha-council"
channel              = "https://www.youtube.com/@CityofAgartha"
audio_if_no_captions = true
```

`site` versus `channel` is the whole difference. Everything downstream — extraction,
chunking, indexing, search — is a shared model, because acquisition is the only place the
two genuinely differ.

For a website, `site` is the seed. `robots.txt` and any sitemap are read from its host,
so a city's sitemap is the whole city whichever page you give. When there is no sitemap to
read, Centinel follows links from the seed instead, and that walk stays under the seed's
path — so `https://linktr.ee/TampaDSA` is that one page and not Linktree. A feed or
calendar address works as a seed too: `https://dsa-pinellas.medium.com/feed`, or a Google
Calendar's public `…/basic.ics`. Each article or event it carries is stored as its own
entry; see [Acquisition](../internals/acquire.md#feeds).

## Adding one

```bash
centinel source add agartha --site https://www.agartha.gov
centinel source add agartha-council --channel https://www.youtube.com/@CityofAgartha
centinel source list
centinel source disable agartha     # a bare `centinel run` skips it; naming it still runs it
centinel source enable agartha
```

`source add` defines the source and stops; `--run` collects it straight away. Before adding
a host you have not collected before, investigate it — see
[Investigate and check](investigate.md). Nothing here is expensive to undo except the hour
a wrong strategy spends.

## Editing collection settings

On a source's page, **Settings** edits its request rate, URL substring filters and
discovery strategy. Channels also have a caption and transcription language and an
audio fallback for videos without captions. Empty overrides inherit the defaults;
empty filters collect every URL. Changes apply to the next run.

The CLI replaces the same collection settings:

```bash
centinel source update agartha --rps 0.5 --match /agenda/ --strategy listing
centinel source update agartha-council --lang es --audio-if-no-captions true
```

Include every override you want to keep. Omitted options remove their overrides, and
omitting `--match` clears the filters. The source id, website or channel address,
enabled state and existing yt-dlp arguments stay intact. Invalid settings change nothing.

## From the web

The **Sources** page does the same, for an administrator signed in to the workspace:

- **Investigate an address** asks which collection method recognises it and shows what
  extraction makes of the page, without adding anything. It opens a page that says, in one
  sentence, whether the site will collect and why not if it will not, then lists in order
  every request made (status, type, size, time, where it redirected) beside each method's
  decision: recognised, with its evidence, or declined. The report is filed under
  `investigations/` in the store, and **Copy report for an agent** puts all of it on the
  clipboard as Markdown, for an agent to improve the scraping from. If something recognised
  it, adding it is one click.
- Each source's page shows what its log holds, the addresses that are not live, its runs,
  and its crumbs, each with an **Investigate** button. **Run** takes the store's one run lane, the
  same one a schedule takes: while a run holds it, a second is refused rather than queued.
  The run is a job under its journal id — the page opens the live drawer on it as it
  starts, and **Stop** ends it at its next page, journaled `interrupted`. A run started
  by a schedule can be watched and stopped the same way.
- The list reads each source's address count from its [summary](../internals/store.md#summary),
  so it answers in milliseconds however large the logs are. Crumbs come from the ledger
  `collect` writes; a source collected before the ledger existed shows how many of its
  pages have no record yet, and **Record its crumbs** reads them once.

An agent cannot reach any of this. Those routes answer a browser session only, never an
MCP token, so a model can read the record but never decide what it collects.

## Configs that still hold `[[source]]` blocks

Sources used to be `[[source]]` blocks in `centinel.toml`, and the store kept its own list
in `log/`. The two drifted: a source collected by hand was missing from the config, and a
block removed from the config left its documents searchable. Now, at the start of every
command, Centinel writes a definition for each block the store does not yet hold, and one
for each log that predates definitions, read back out of what it collected. Nothing is
overwritten. After that the blocks are no longer read, and every command says so until
you delete them.

## Where the config lives

The config holds what is true of **this machine**: where the store is, the defaults below,
how files open, and the [schedules](schedules.md) it fires. Schedules stay here on purpose:
a store handed to someone else lists its sources and collects nothing until its new owner
runs a command. Nearest answer wins:

1. `$CENTINEL_CONFIG`
2. `./centinel.toml`
3. `~/.centinel/centinel.toml`
4. `~/.config/centinel/config.toml`

`centinel doctor` prints which file was used and which store root it named. There is a
starting point at [`contrib/centinel.toml.example`](https://github.com/bennyhodl/centinel/blob/master/contrib/centinel.toml.example).

## Defaults

```toml
[defaults]
rps = 1.0                                  # requests per second, per host
lang = "en"                                # caption and transcription language
```

`rps` is deliberately slow. Politeness is per host, which is also why acquisition runs per
source rather than corpus-wide.

Which models embed, rerank and transcribe, and where they run, is `[inference]` — see
[Inference servers](../reference/inference.md). Earlier releases kept `embed_model`,
`embed_batch` and `transcribe_model` here; a config that still has them is refused with what
to write instead.

## Where the store lives

`~/.centinel`, unless something says otherwise. Nearest answer wins:

| | |
|---|---|
| `--root DIR`, or `$CENTINEL_ROOT` | somebody typed a path — an instruction |
| `root = "~/corpora/agartha"` in `centinel.toml` | the standing preference; `~/` is expanded |
| `~/.centinel` | the default |

It is in `$HOME` because a store is a corpus you keep, not an artefact of the directory
you were standing in. This defaulted to `.centinel` in the **working directory** once, and
the result was that every shell got its own corpus: a separate blob pool, a separate log,
a separate index, none of them answering a search against the others, and none of it
visible until a search from one directory up came back empty.

## One-off addresses

For URLs outside any discovery run:

```bash
centinel ingest https://example.gov/some/document.pdf
```

It fetches into the content-addressed store like any other acquisition. The document then
extracts, indexes and embeds with everything else.

Next: [The run](run.md).
