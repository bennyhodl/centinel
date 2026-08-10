# Sources

A **Source** is one thing you collect: a website, or a YouTube channel. It is a trait in
the code, not an entity with a `kind` field, and that shows up here as one key in the
config.

```toml
[[source]]
id   = "agartha"
site = "https://www.agartha.gov"

[[source]]
id                   = "agartha-council"
channel              = "https://www.youtube.com/@CityofAgartha"
audio_if_no_captions = true
```

`site` versus `channel` is the whole difference. Everything downstream — extraction,
chunking, indexing, search — is a shared model, because acquisition is the only place the
two genuinely differ.

For a website, `site` is any URL on it; only the origin is used.

## Adding one

```bash
centinel source add agartha --site https://www.agartha.gov
centinel source add agartha-council --channel https://www.youtube.com/@CityofAgartha
centinel source list
centinel source remove agartha
```

`source add` writes into whichever config file was found, and into
`~/.centinel/centinel.toml` when none was — beside the store the same command collects
into.

Before adding a host you have not collected before, investigate it — see
[Investigate and check](investigate.md). Nothing here is expensive to undo except the hour
a wrong strategy spends.

## Where the config lives

Nearest answer wins:

1. `$CENTINEL_CONFIG`
2. `./centinel.toml`
3. `~/.centinel/centinel.toml`
4. `~/.config/centinel/config.toml`

A per-project `centinel.toml` still wins, so a checkout travels with its own sources.
`centinel doctor` prints which file was used and which store root it named.

There is a starting point at [`contrib/centinel.toml.example`](https://github.com/bennyhodl/centinel/blob/master/contrib/centinel.toml.example).

## Defaults

```toml
[defaults]
rps = 1.0                                  # requests per second, per host
embed_model = "qwen3-embedding-4b"
transcribe_model = "whisper-large-v3-turbo"
lang = "en"
```

`rps` is deliberately slow. Politeness is per host, which is also why acquisition runs per
source rather than corpus-wide.

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

## The config is intent; the store is fact

They can disagree, and that disagreement is a real state you will hit.

Running `centinel discover --source valhalla --site …` by hand collects a source the
config never named. `run` then ignores it — correctly, because nothing declared it. Left
alone, that is an invisible corpus: collected, indexed, searchable, and never refreshed.

So `source list` reports the **union** and marks what the config does not name:

```console
$ centinel source list
   source    kind  resources             target
✓  agartha   site      1,847             https://www.agartha.gov
   valhalla  site        412  untracked  https://www.valhallacounty.org

1 source is in the store but not in the config — `centinel run` skips it.
  centinel source adopt
```

Those addresses are **read back out of the log, not guessed.** A `DiscoveryRun` records
its method — `sitemap`, `playlist` — and the resources say where from. A channel is the
interesting case: the log records the videos, never the channel they were listed from, but
the archived `yt-dlp -J` document beside each recording carries `uploader_url`.

```bash
centinel source adopt          # write every recoverable one into the config
centinel source add valhalla    # the same, for one, with no --site needed
```

A source whose address cannot be recovered is **named and skipped**, rather than written
as a block that would fail on the next run.

## One-off addresses

For URLs outside any discovery run:

```bash
centinel ingest https://example.gov/some/document.pdf
```

It fetches into the content-addressed store like any other acquisition. The document then
extracts, indexes and embeds with everything else.

Next: [The run](run.md).
