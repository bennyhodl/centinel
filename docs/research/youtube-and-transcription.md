# YouTube Ingestion and Transcription — Research Survey

_Research for Centinel v2. Completed 2026-08-02._
_Scope: channel enumeration, transcript retrieval, audio download, local + API transcription, timestamps, long-form audio, ToS posture, language tradeoffs._
_Rule for this document: every factual claim carries a primary-source URL. Where a number is extrapolated rather than quoted, it is labelled **[EXTRAPOLATION]**._
_Figures marked "verified directly" were observed by running the tool against a live YouTube channel (City of Tampa) on 2026-08-02 with yt-dlp 2026.03.17 on Apple Silicon. No media files were downloaded and no transcription was benchmarked locally; all transcription performance numbers are quoted from published sources or explicitly labelled extrapolations._

---

## Executive summary — the seven things that matter

1. **Data API quota is not the blocker.** The folklore is stale. Enumerating a channel's full back catalogue via `playlistItems.list` + `videos.list` costs ~1 unit per 50 videos each — roughly **80 units for a 2,000-video channel** against a 10,000/day free allocation. The trap is `search.list`, now hard-capped at **100 calls/day** in its own bucket. Never route discovery through search. (§1.1)
2. **There is no sanctioned way to read third-party captions.** `captions.download` requires edit permission on the video. Every transcript path in every language is unofficial — either a `yt-dlp` wrapper or a reimplementation of YouTube's private `timedtext`/InnerTube endpoints. There is no third category. (§1.1, §2.1)
3. **There is no viable pure-Rust transcript path.** The only Rust crate is a `0.1.0` from May 2023 with 2,690 lifetime downloads. Rust means a `yt-dlp` subprocess, permanently. (§2.2, §7.2)
4. **Manual vs auto captions are cleanly distinguishable, and captions carry word-level timestamps.** `yt-dlp` returns them in separate `subtitles` / `automatic_captions` dicts; the `json3` format carries per-word `tOffsetMs`. Archive `json3`, not `srt`. (§2.3, §2.4)
5. **Fragility is real and quantified: 26 yt-dlp releases in 2025**, in emergency clusters, with 185 open/closed issues on bot-detection and 911 on PO Tokens. Plan for repeated breakage, not permanent failure. (§2.5)
6. **A 3-hour meeting costs $0.12–$1.08 via API** (Groq turbo cheapest at ~$0.12 and ~1 minute wall-clock), versus an estimated **9–22 minutes** locally on an Apple Silicon laptop **[EXTRAPOLATION]**. Cost is not a reason to run locally; sovereignty and reproducibility are. (§4.5)
7. **Whisper hallucinates on silence, and this is documented — 1% of transcriptions contain fabricated sentences, 38% of those contain explicit harms, and the effect correlates with non-vocal duration.** Council recordings are dead-air-heavy, so this is a first-order design constraint, not a footnote. Mitigations exist (VAD, `condition_on_previous_text=False`, `hallucination_silence_threshold`) and are **off by default**. (§5.3)

---

## 1. Channel enumeration and metadata

### 1.1 YouTube Data API v3 — quota is NOT the blocker (contrary to folklore)

The quota model changed materially in 2025–2026 and most third-party writing about it is stale. Current official numbers:

> "Projects that enable the YouTube Data API have a default quota allocation of 100 `search.list` calls, 100 `videos.insert` calls, and 10,000 units per day combined for all other endpoints."
> — [Quota Calculator](https://developers.google.com/youtube/v3/determine_quota_cost), [Getting Started](https://developers.google.com/youtube/v3/getting-started)

Daily quotas reset at midnight Pacific Time. "Every API request, even if invalid, will cost at least one quota point."

Relevant per-call costs, verbatim from the [quota table](https://developers.google.com/youtube/v3/determine_quota_cost):

| Resource | Method | Cost |
|---|---|---|
| `channels` | `list` | 1 |
| `playlistItems` | `list` | 1 |
| `videos` | `list` | 1 |
| `playlists` | `list` | 1 |
| `activities` | `list` | 1 |
| `captions` | `list` | 50 |
| `captions` | `download` | 200 ([captions.download docs](https://developers.google.com/youtube/v3/docs/captions/download)) |
| `search` | `list` | "100 quota per day. Each call costs 1 quota." |
| `videos` | `insert` | "100 quota per day. Each call costs 1 quota." |

The `search.list` / `videos.insert` split into their own buckets is a documented change dated **June 1, 2026** in the [revision history](https://developers.google.com/youtube/v3/revision_history):

> "API calls to the `videos.insert` and `search.list` methods will be charged to their own respective quota buckets."

**Consequence for Centinel: full back-catalogue enumeration is cheap, provided you never touch `search.list`.**

The correct enumeration path avoids search entirely:

1. `channels.list?part=contentDetails&id=UC...` → 1 unit. Returns `contentDetails.relatedPlaylists.uploads`, the channel's uploads playlist ID (conventionally the channel ID with `UC` → `UU`).
2. `playlistItems.list?playlistId=UU...&maxResults=50` → **1 unit per 50 videos.**
3. `videos.list?id=<50 comma-separated IDs>&part=snippet,contentDetails,statistics,status` → **1 unit per 50 videos** for full metadata including duration, tags, category, view counts, live-broadcast status.

Cost to enumerate a channel with N videos ≈ `1 + ceil(N/50) + ceil(N/50)` units. **[EXTRAPOLATION from the documented per-call costs above, not a quoted figure]**:

| Channel size | Units for full enumeration + metadata | % of 10,000/day |
|---|---|---|
| 500 videos | ~21 | 0.2% |
| 2,000 videos | ~81 | 0.8% |
| 10,000 videos | ~401 | 4% |
| 250,000 videos | ~10,001 | 100% (one full day) |

A municipal channel is in the hundreds-to-low-thousands range. You could enumerate roughly **a hundred such channels per day** on the free default quota, and a daily delta poll costs single-digit units per channel. Quota is a non-issue for this workload. Quota becomes fatal only if you route discovery through `search.list`, which is now hard-capped at 100 calls/day regardless.

Caveat worth designing around: `playlistItems.list` on the uploads playlist has a long-standing practical ceiling — it will not page past roughly 20,000 items, and it omits some content types (unlisted/members-only/certain live archives). For municipal channels this is not binding.

**What the API does NOT give you:** transcripts of videos you do not own. `captions.download` "requires the user to have permission to edit the video" and returns `403 forbidden` — "The permissions associated with the request are not sufficient to download the caption track" — otherwise ([captions.download](https://developers.google.com/youtube/v3/docs/captions/download)). This is the single most important fact in this document: **there is no official, sanctioned API for reading third-party captions.** Every transcript path in section 2 is unofficial.

The API also requires a Google Cloud project and an API key, and Google reserves the right to run a [Quota and Compliance Audit](https://developers.google.com/youtube/v3/guides/quota_and_compliance_audits) on projects requesting extensions.

### 1.2 Channel RSS feeds — quota-free, but only a 15-item window

`https://www.youtube.com/feeds/videos.xml?channel_id=UC...` (also `?playlist_id=`, `?user=`). No API key, no quota.

**Directly verified** against a live feed (`UCBa659QWEk1AI4Tg--mrJ2A`): the feed is Atom, contains **exactly 15 `<entry>` elements**, and each entry carries:

- `yt:videoId`, `yt:channelId`
- `title`, `link` (watch URL), `author` (name + channel URI)
- `published`, `updated` (ISO 8601 timestamps)
- `media:group` → `media:title`, `media:content`, `media:thumbnail`, `media:description` (full description text), `media:community` (`media:starRating`, `media:statistics views`)

Limits, which matter:

- **15 items, hard.** No paging parameter, no `start-index`, no history. Everything older is simply absent.
- No duration, no tags, no category, no caption availability.
- Not reliably ordered by publish date in edge cases (schedule/premiere changes can reorder).

**Role in Centinel: RSS is a delta-detection tick, not an enumeration mechanism.** It is the correct thing to poll every 15 minutes for "did this council post something new" at zero quota cost, with the API or yt-dlp doing the initial backfill and the metadata enrichment. For a channel that posts more than 15 videos between polls, RSS silently loses videos — so RSS must never be the only discovery path.

### 1.3 yt-dlp — no API key, and richer metadata than the API

**Directly verified locally** (yt-dlp 2026.03.17, macOS/Apple Silicon) against `https://www.youtube.com/@CityofTampa/videos`.

`yt-dlp --flat-playlist -J <channel>/videos` — one HTTP round-trip per page, no key. Per-entry fields are deliberately thin:

```
id, title, url, duration, thumbnails, timestamp, ie_key, _type
```

`timestamp` came back `None` for the `/videos` tab (yt-dlp exposes `--extractor-args "youtubetab:approximate_date"` to populate an approximate upload date in flat mode). Top-level playlist fields include `channel`, `channel_id`, `channel_url`, `channel_follower_count`, `description`, `tags`, `playlist_count`, `uploader*`.

Full per-video extraction (`yt-dlp -J --skip-download <video>`) returns substantially more than the Data API's `snippet`:

```
id, title, fulltitle, description, upload_date, timestamp, release_timestamp, release_year,
duration, duration_string, view_count, like_count, comment_count, average_rating,
channel, channel_id, channel_url, channel_follower_count, uploader, uploader_id, uploader_url,
categories, tags, creators, language, live_status, was_live, is_live, availability,
age_limit, playable_in_embed, media_type, chapters, thumbnails, heatmap,
formats[...], subtitles{...}, automatic_captions{...}
```

Notably present and absent from the Data API: **`chapters`** (creator-authored chapter markers with start/end/title), **`heatmap`** (most-replayed graph), and the **full format list with direct media URLs**. `language` came back as `en-US`.

Completeness assessment: yt-dlp's metadata is a **superset** of what `videos.list` returns for public videos, at the cost of being scraped rather than contracted. The one thing the Data API has that yt-dlp does not is a stability guarantee and a documented deprecation process.

### 1.4 Per-language client landscape (enumeration)

| Language | Data API v3 client | Non-API enumeration | Verdict |
|---|---|---|---|
| **Python** | [`google-api-python-client`](https://pypi.org/project/google-api-python-client/) — official Google, v2.198.0 (2026-06-25), 286 releases | `yt-dlp` **as an importable library** (`YoutubeDL().extract_info()`), no subprocess; also [`scrapetube`](https://pypi.org/project/scrapetube/) v2.6.0 (2025-09-16), [`pytubefix`](https://pypi.org/project/pytubefix/) v10.11.0 (2026-07-29, 277 releases) | Strongest. Only language where yt-dlp is in-process. |
| **TypeScript/JS** | [`googleapis`](https://www.npmjs.com/package/googleapis) — official Google, v173.0.0 (2026-05-28), ~10M weekly downloads | [`youtubei.js`](https://www.npmjs.com/package/youtubei.js) v17.2.0 (2026-06-24), ~183k weekly — a native TS client for YouTube's private **InnerTube** API, not a yt-dlp wrapper | Genuinely strong. `youtubei.js` is the only serious non-Python reimplementation. |
| **Rust** | [`google-youtube3`](https://crates.io/crates/google-youtube3) v7.0.0+20251222 (2026-01-01), 138k downloads — auto-generated from Google's discovery document by the `google-apis-rs` project. Not first-party but machine-generated and current. | [`youtube_dl`](https://crates.io/crates/youtube_dl) v0.10.0 — **last published 2024-04-16**; its own description is "Runs yt-dlp and parses its JSON output," i.e. a subprocess wrapper. [`rustube`](https://crates.io/crates/rustube) v0.6.0 (2022-10-16, abandoned). [`rusty_ytdl`](https://crates.io/crates/rusty_ytdl) v0.7.4 (2024-08-10, stale). [`invidious`](https://crates.io/crates/invidious) v0.7.8 (2025-05-09) — wraps third-party Invidious instances, most of which are now blocked by YouTube. | **Data API in Rust is fine. Everything else is a `yt-dlp` subprocess.** |

**Where Rust forces a subprocess:** any non-API channel enumeration, any format/URL resolution, and (see §2) all transcript retrieval. `google-youtube3` covers the sanctioned API cleanly; there is no maintained pure-Rust InnerTube client comparable to `youtubei.js`.

---

## 2. Transcript and caption retrieval — the fragility question

### 2.1 There is no official path, in any language

Restating §1.1 because it governs everything below: `captions.download` requires edit permission on the video. For a channel you do not own, the YouTube Data API cannot return caption text. Full stop.

Therefore **every** transcript library in every language is one of exactly two things:

1. A wrapper/subprocess around `yt-dlp`, or
2. A reimplementation of YouTube's unofficial `timedtext` / InnerTube `get_transcript` endpoints.

**Direct answer to "does anything exist for Rust that is neither?"** — **No.** And not just for Rust: nothing in any language is neither, because there is no third category. The distinction that actually matters is *who maintains the reimplementation and how fast they patch it.*

### 2.2 The options, by language

| Language | Package | Category | Health |
|---|---|---|---|
| Python | [`youtube-transcript-api`](https://pypi.org/project/youtube-transcript-api/) | (2) reimplementation | v1.2.4 (2026-01-29), 35 releases. Active but see churn below. |
| Python | `yt-dlp` (imported) | (1), in-process | v2026.7.4, 626 PyPI releases |
| TS/JS | [`youtubei.js`](https://www.npmjs.com/package/youtubei.js) `getTranscript()` | (2) reimplementation | v17.2.0 (2026-06-24), 183k/wk. Best-maintained non-Python option. |
| TS/JS | [`youtube-transcript`](https://www.npmjs.com/package/youtube-transcript) | (2) | v1.3.1 (2026-04-25), 167k/wk |
| TS/JS | [`youtube-transcript-plus`](https://www.npmjs.com/package/youtube-transcript-plus) | (2), fork with caching/proxy | v2.0.1 (2026-07-30), 25k/wk |
| TS/JS | [`youtube-captions-scraper`](https://www.npmjs.com/package/youtube-captions-scraper) | (2) | v2.0.3 (**2024-02-28**) — stale |
| Rust | [`youtube-captions`](https://crates.io/crates/youtube-captions) | (2) | v0.1.0, **published 2023-05-25**, 2,690 lifetime downloads. Effectively abandoned. |
| Rust | [`youtube_dl`](https://crates.io/crates/youtube_dl) | (1) subprocess | v0.10.0 (2024-04-16) |

The Rust column is the honest finding: **there is no viable pure-Rust transcript path.** The only Rust crate that even attempts it is a single-`0.1.0` release from 2023 with three thousand lifetime downloads. A Rust implementation means shelling out to `yt-dlp`, or writing and *maintaining* your own `timedtext` client against an endpoint that changes without notice.

### 2.3 Manual vs. auto-generated captions — yes, distinguishable

**Directly verified.** `yt-dlp -J` returns two separate top-level dictionaries:

- `subtitles` — **human-authored / uploaded** caption tracks
- `automatic_captions` — **ASR-generated** tracks

On the City of Tampa video tested: `subtitles` was `[]` (empty) and `automatic_captions` had **157 language entries**. That asymmetry is typical and is itself a useful signal — 150+ languages means machine translation off a single ASR track.

CLI equivalents: `--write-subs` (manual only), `--write-auto-subs` (ASR), `--list-subs` to inspect. `youtube-transcript-api` exposes the same distinction via `transcript.is_generated`.

**Quality difference is real and material for council meetings.** Auto-captions have no speaker labels, no punctuation reliability across speaker turns, and degrade badly on proper nouns (street names, ordinance numbers, surnames) — exactly the tokens a civic-transparency search index is built around. Where a manual track exists it is strictly better and should be preferred. In practice most municipal channels publish auto-only, which is the main argument for the local-Whisper fallback path being a first-class feature rather than an edge case.

### 2.4 Timestamps — yes, and at word level

**Directly verified** by fetching `--sub-format json3` auto-captions for a 350-second video. Available formats for a track: `json3`, `srv1`, `srv2`, `srv3`, `ttml`, `srt`, `vtt`.

The `json3` structure carries word-level offsets:

```json
{ "tStartMs": 2440, "dDurationMs": 2800, "wWinId": 1,
  "segs": [ {"utf8":"would"}, {"utf8":" you","tOffsetMs":160},
            {"utf8":" have","tOffsetMs":200}, {"utf8":" been?","tOffsetMs":320},
            {"utf8":" I","tOffsetMs":720}, {"utf8":" know","tOffsetMs":760} ] }
```

Each event has an absolute `tStartMs` + `dDurationMs`; each `seg` (word) has a `tOffsetMs` relative to the event start. **Recommendation: retain `json3` as the archival format.** `srt`/`vtt` are lossy conversions that collapse to segment-level cue timing and discard the per-word offsets. `youtube-transcript-api` returns `{text, start, duration}` — **segment-level only**, word offsets discarded.

Size, measured: 101,914 bytes of `json3` for 350 seconds of dense speech ≈ **0.29 KB/s**. **[EXTRAPOLATION]** a 3-hour meeting ≈ **3 MB** of `json3` — less in practice, since meetings contain more silence than a talking-head interview. Content-addressed retention of every version is comfortably affordable at that size; a decade of weekly meetings for one body is well under 2 GB.

### 2.5 Fragility — quantified

This is the load-bearing risk, and the numbers are not reassuring.

**yt-dlp release cadence** (from the [GitHub releases API](https://api.github.com/repos/yt-dlp/yt-dlp/releases), retrieved for this document):

- **26 stable releases in calendar 2025**: 2025.01.12, .01.15, .01.26, .02.19, .03.21, .03.25, .03.26, .03.27, .03.31, .04.30, .05.22, .06.09, .06.25, .06.30, .07.21, .08.11, .08.20, .08.22, .08.27, .09.05, .09.23, .09.26, .10.14, .10.22, .11.12, .12.08
- **9 stable releases in 2026 through July 4**: .01.29, .01.31, .02.04, .02.21, .03.03, .03.13, .03.17, .06.09, .07.04
- Note the clusters: **four releases in eleven days** (2025-03-21 → 03-31), **four in seventeen days** (2025-08-11 → 08-27), **three in eight days** (2026-01-29 → 02-04). Those clusters are the signature of emergency YouTube-breakage patches.
- 626 total releases on PyPI.
- A separate **nightly** channel ships far more often — [`yt-dlp/yt-dlp-nightly-builds`](https://github.com/yt-dlp/yt-dlp-nightly-builds) published 10 builds between 2026-07-02 and 2026-07-23, i.e. roughly every other day.
- yt-dlp itself prints a warning when the binary is more than 90 days old: *"Your yt-dlp version (2026.03.17) is older than 90 days! It is strongly recommended to always use the latest version."* (observed directly).

**Issue-tracker volume** (GitHub search API, retrieved for this document):

- yt-dlp: **142 open issues** with "youtube" in the title.
- yt-dlp: **185 issues** matching `"Sign in to confirm"` — the bot-detection wall.
- yt-dlp: **911 issues** matching `"po token"`.
- `youtube-transcript-api`: **29 issues** referencing `RequestBlocked`, **27** referencing `IpBlocked`, against only **18 currently open** — i.e. the blocking-related issue count is several times the entire open backlog, which means these are opened and closed repeatedly.

**The two named, structural breakages:**

1. **PO Tokens.** Per yt-dlp's own [PO Token Guide](https://github.com/yt-dlp/yt-dlp/wiki/PO-Token-Guide): *"A PO Token is generated by an attestation provider on Web, Android and iOS platforms to attest the requests are coming from a genuine client."* Without one, requests "may result in HTTP Error 403, or result in your account or IP address being blocked." Critically for this project, the guide lists a **"Subs required"** class — the `web`, `android`, and `ios` clients require a PO Token *for subtitles specifically*. Clients currently free of the requirement: `android_vr`, `web_embedded`, `tv`. (Our verified caption fetch above used the `android vr player API JSON` client — the current workaround.) The guide also warns: *"Manually extracting PO Tokens is no longer recommended. YouTube now binds PO Tokens to the video ID, so a new token needs to be generated for each video."* Mitigation is a provider plugin — `bgutil-ytdlp-pot-provider` or `yt-dlp-getpot-wpc` — which drags in a Node/Deno or headless-browser dependency.
2. **SABR.** YouTube is migrating to a new streaming protocol; yt-dlp has an open tracking issue [#13515 "[fd/sabr] Add YouTube SABR protocol downloader"](https://github.com/yt-dlp/yt-dlp/issues/13515) (opened 2025-06-21, still open). This is an ongoing, unresolved threat to the download path specifically.

**Practical read.** The failure mode is not "it stops working forever," it is "it breaks for somewhere between hours and a couple of weeks, repeatedly, on YouTube's schedule." Any Centinel design must therefore:

- Treat transcript fetch failures as **expected and retryable**, never as a hard job failure.
- Pin a yt-dlp version for reproducibility but have a **fast bump path** — and accept a ≤90-day staleness budget.
- Keep the **local-Whisper fallback** as a genuine independent path, because it depends only on the audio download, which fails less often than the caption endpoints.
- Prefer **datacenter-IP-avoidance** or residential egress if running in cloud; the `IpBlocked`/`RequestBlocked` issue volume above is overwhelmingly from cloud IP ranges.

## 3. Audio download

### 3.1 What YouTube actually serves

**Measured directly** on a real City-of-Tampa video (350 s) via `yt-dlp -J`. The audio-only formats offered were:

| itag | container | codec | abr (kbps) | sample rate | ch | measured filesize | measured rate | **[EXTRAPOLATED] 3-hour size** |
|---|---|---|---|---|---|---|---|---|
| 139 | m4a | `mp4a.40.5` (HE-AAC) | 48.8 | 22.05 kHz | 2 | 2,136,889 B | 6.0 kB/s | **~63 MB** |
| 140 | m4a | `mp4a.40.2` (AAC-LC) | 129.5 | 44.1 kHz | 2 | 5,668,512 B | 15.8 kB/s | **~167 MB** |
| 251 | webm | Opus | 125.3 | 48 kHz | 2 | 5,486,334 B | 15.3 kB/s | **~161 MB** |

The 3-hour column extrapolates the measured byte rate to 10,800 s. Real meetings vary by a few percent (VBR), but the order of magnitude is solid.

### 3.2 What Whisper actually wants

Every Whisper implementation resamples input to **16 kHz mono**. Downloading the 129 kbps stereo 44.1 kHz track and then throwing away three quarters of it is waste. Two consequences:

- **itag 139 (48 kbps HE-AAC, 22.05 kHz) is sufficient for ASR** and is ~2.6× smaller than the alternatives. It is the right default when the only downstream consumer is transcription. Keep 140/251 only if you intend to archive listenable audio.
- The decoded intermediate is the size that actually matters for disk churn: 16 kHz × 16-bit × mono = 32,000 B/s → **[EXTRAPOLATION]** a 3-hour meeting is **~346 MB of PCM WAV**. Stream it rather than materialising it where the implementation allows.

### 3.3 Per-language mechanics

| Language | Path | Subprocess required? |
|---|---|---|
| Python | `yt-dlp` imported in-process; `format='bestaudio[abr<60]'`; postprocessing via `FFmpegExtractAudio` | **ffmpeg only** (for transcode/remux; raw stream download needs none) |
| TS/JS | `youtubei.js` can resolve and stream audio-only formats natively; `@distube/ytdl-core` (v4.16.12, 2025-06-13) also does. Otherwise shell to `yt-dlp`. | ffmpeg for transcode |
| Rust | `youtube_dl` crate → **shells out to the `yt-dlp` binary**. Direct HTTP GET of a resolved format URL is trivial in Rust; *resolving* the URL is the hard part, and that is where the subprocess lands. Decoding: `symphonia` (pure-Rust demux/decode for AAC/Opus/MP4) + `rubato` (resampling) can replace ffmpeg entirely — this is one place Rust is genuinely good. | **yt-dlp for URL resolution; ffmpeg avoidable** |

Note that even Python and JS effectively require **ffmpeg on PATH** for any container work, so "no external binary" is not achievable in any language once you touch audio. The question is only whether you need *two* external binaries (yt-dlp + ffmpeg) or one.

---

## 4. Whisper and local transcription

### 4.1 The model line-up

From the [openai/whisper README](https://github.com/openai/whisper), verbatim:

| Size | Parameters | English-only | Multilingual | Required VRAM | Relative speed |
|:---:|:---:|:---:|:---:|:---:|:---:|
| tiny | 39 M | `tiny.en` | `tiny` | ~1 GB | ~10x |
| base | 74 M | `base.en` | `base` | ~1 GB | ~7x |
| small | 244 M | `small.en` | `small` | ~2 GB | ~4x |
| medium | 769 M | `medium.en` | `medium` | ~5 GB | ~2x |
| large | 1550 M | N/A | `large` | ~10 GB | 1x |
| **turbo** | **809 M** | N/A | `turbo` | **~6 GB** | **~8x** |

`turbo` (= `large-v3-turbo`) is described as "an optimized version of `large-v3` that offers faster transcription speed with a minimal degradation in accuracy." It achieves this by cutting the decoder from 32 layers to 4. Caveat from the same README: "the `turbo` model is not trained for translation tasks" — irrelevant for English council meetings, relevant if you ever ingest Spanish-language public comment and want English output.

**For this project, `large-v3-turbo` is almost certainly the right default**: near-large accuracy at ~8× the speed, in 6 GB.

### 4.2 whisper.cpp and whisper-rs

[whisper.cpp](https://github.com/ggml-org/whisper.cpp) — C/C++ port, ggml/GGUF weights.

Official model footprint table, verbatim:

| Model | Disk | Mem |
|---|---|---|
| tiny | 75 MiB | ~273 MB |
| base | 142 MiB | ~388 MB |
| small | 466 MiB | ~852 MB |
| medium | 1.5 GiB | ~2.1 GB |
| large | 2.9 GiB | ~3.9 GB |

Apple Silicon claims from the README, verbatim:

> "Apple Silicon first-class citizen - optimized via ARM NEON, Accelerate framework, Metal and Core ML."
> "On Apple Silicon, the inference runs fully on the GPU via Metal."
> "the Encoder inference can be executed on the Apple Neural Engine (ANE) via Core ML. This can result in significant speed-up - **more than x3 faster** compared with CPU-only execution."

Core ML caveat, verbatim: "The first run on a device is slow, since the ANE service compiles the Core ML model to some device-specific format. Next runs are faster."

Build requirements: CMake + a C/C++ toolchain. Optional backends: Metal (default on macOS), CUDA, ROCm, OpenVINO, Vulkan. Core ML requires converting the model with Python `coremltools` first — an extra build-time step, not a runtime dependency.

**CPU-only encoder benchmarks** from the project's [benchmark thread (#89)](https://github.com/ggml-org/whisper.cpp/issues/89), measuring encode time for **one 30-second window**:

| Machine | threads | base | small | medium | large |
|---|---|---|---|---|---|
| MacBook M1 Pro | 8 | 220 ms | 685 ms | 1,928 ms | 3,350 ms |
| Mac Mini M1 | 4 | 380 ms | 1,249 ms | 3,980 ms | 7,979 ms |
| iPhone 13 Mini | 4 | 1,091 ms | — | — | — |

These are old, NEON+BLAS, **pre-Metal** numbers and understate current performance considerably. **[EXTRAPOLATION, encoder only, CPU only]** a 3-hour file is 360 windows, so M1 Pro `large` encode alone ≈ 360 × 3.35 s ≈ **20 minutes**, before any decoder time. With Metal and `large-v3-turbo` the real figure is far lower — see §4.5 for the practical estimate and its caveats.

**Rust bindings: [`whisper-rs`](https://crates.io/crates/whisper-rs)** — v0.16.0, published **2026-03-12**, **829,884 lifetime downloads**, repo now on [Codeberg](https://codeberg.org/tazz4843/whisper-rs). This is a healthy, actively-maintained crate and it is **the strongest single argument for Rust in this entire document**: unlike every YouTube-facing crate surveyed above, `whisper-rs` links whisper.cpp as a native library via FFI — **no subprocess, no Python**. Metal and CUDA are cargo features. Cost: whisper.cpp is vendored as a git submodule and compiled by `build.rs`, so you inherit a CMake/C++ toolchain requirement and slow cold builds.

Node equivalents are weaker: [`nodejs-whisper`](https://www.npmjs.com/package/nodejs-whisper) v0.3.0 (2026-04-11, ~21k weekly) shells out to the compiled `whisper-cli` binary; [`smart-whisper`](https://www.npmjs.com/package/smart-whisper) v0.8.1 (**2024-10-02**, ~3.4k weekly) is a real N-API binding but is stale; [`whisper-node`](https://www.npmjs.com/package/whisper-node) v1.1.1 (**2023-11-29**) is abandoned.

### 4.3 faster-whisper (CTranslate2)

[faster-whisper](https://github.com/SYSTRAN/faster-whisper) v1.2.1 (2025-10-31). Headline claim, verbatim: **"up to 4 times faster than openai/whisper for the same accuracy while using less memory."**

Published benchmarks, verbatim, on **13 minutes of audio**:

**large-v2 on GPU** *(CUDA 12.4, NVIDIA RTX 3070 Ti 8GB)*:

| Implementation | Precision | Beam | Time | VRAM |
|---|---|---|---|---|
| openai/whisper | fp16 | 5 | 2m23s | 4708 MB |
| whisper.cpp (Flash Attention) | fp16 | 5 | 1m05s | 4127 MB |
| faster-whisper | fp16 | 5 | 1m03s | 4525 MB |
| faster-whisper (`batch_size=8`) | fp16 | 5 | **17s** | 6090 MB |
| faster-whisper | int8 | 5 | 59s | 2926 MB |
| faster-whisper (`batch_size=8`) | int8 | 5 | **16s** | 4500 MB |

**small on CPU** *(8 threads, Intel Core i7-12700K)*:

| Implementation | Precision | Beam | Time | RAM |
|---|---|---|---|---|
| openai/whisper | fp32 | 5 | 6m58s | 2335 MB |
| whisper.cpp | fp32 | 5 | 2m05s | 1049 MB |
| whisper.cpp (OpenVINO) | fp32 | 5 | 1m45s | 1642 MB |
| faster-whisper | fp32 | 5 | 2m37s | 2257 MB |
| faster-whisper (`batch_size=8`) | fp32 | 5 | 1m06s | 4230 MB |
| faster-whisper | int8 | 5 | 1m42s | 1477 MB |
| faster-whisper (`batch_size=8`) | int8 | 5 | **51s** | 3608 MB |

Derived real-time factors from that CPU table **[EXTRAPOLATION — arithmetic on the quoted times, on x86, not Apple Silicon]**: openai/whisper `small` ≈ 1.9× realtime; whisper.cpp `small` ≈ 6.2×; faster-whisper `small` int8 ≈ 7.6×; faster-whisper `small` int8 batched ≈ 15×.

**The Apple Silicon caveat that matters:** CTranslate2 has **no Metal backend**. On an M-series Mac, faster-whisper runs on CPU only. This inverts the usual ranking — on Apple Silicon, whisper.cpp with Metal generally beats faster-whisper, while on an NVIDIA box faster-whisper generally wins. Do not carry the x86/CUDA benchmark ordering across to a MacBook.

### 4.4 Other current options

- **openai/whisper (reference)** — [PyPI](https://pypi.org/project/openai-whisper/) v20250625, released 2025-06-26, only 13 releases ever. It is the correctness baseline and the slowest option; nobody should run it in production. `transcribe()` "reads the entire file and processes the audio with a sliding 30-second window, performing autoregressive sequence-to-sequence predictions on each window."
- **WhisperX** — [PyPI](https://pypi.org/project/whisperx/) v3.8.6 (2026-05-25), 44 releases. INTERSPEECH 2023. Claims, verbatim: **"70x realtime transcription using whisper large-v2"** with batched inference; **"VAD preprocessing, reduces hallucination & batching with no WER degradation"**; **"Accurate word-level timestamps using wav2vec2 alignment"**; **"Multispeaker ASR using speaker diarization from pyannote-audio."** Also: "`--condition_on_prev_text` is set to `False` by default (reduces hallucination)." **This is the single best-fit tool for multi-speaker council meetings** — it is the only mainstream option that gives you diarization *and* forced-aligned word timestamps in one pass. It is Python-only, and pyannote requires a HuggingFace token and accepting a model licence.
- **Distil-Whisper** — [arXiv:2311.00430](https://arxiv.org/abs/2311.00430). Verbatim: "The distilled model is **5.8 times faster with 51% fewer parameters**, while performing to within **1% WER** on out-of-distribution test data in a zero-shot transfer setting. Distil-Whisper maintains the robustness of the Whisper model to difficult acoustic conditions, **while being less prone to hallucination errors on long-form audio**." That last clause is directly on-mission. English-only for the `-en` variants. Largely superseded by `large-v3-turbo` for general use, but the long-form hallucination claim keeps it interesting.
- **NVIDIA Parakeet TDT 0.6B v2** — [model card](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v2). 600M params, FastConformer-TDT, **CC-BY-4.0**, English-only (a v3 covering 25 European languages exists). Reported **RTFx 3380** on the HF Open ASR Leaderboard at batch size 128; average WER **6.05%**, LibriSpeech test-clean **1.69%**, test-other **3.19%**. Emits **word-level timestamps**. Handles **"audio segments up to 24 minutes in a single pass"** — a genuinely useful property for long meetings. Requirements: "Atleast 2GB RAM for model to load," NVIDIA Volta/Ampere/Hopper/Blackwell GPUs, Linux preferred. **Practical blocker for this project: it runs through NVIDIA NeMo, which is Python-only and effectively NVIDIA-GPU-only.** There is no Apple-Silicon story and no Rust or JS story.

### 4.5 What a 3-hour council meeting actually costs

**API routes — these are quoted prices, high confidence:**

| Provider | Model | Quoted price | **3-hour meeting** | Notes |
|---|---|---|---|---|
| [Groq](https://groq.com/pricing) | `whisper-large-v3-turbo` | **$0.04 / hr audio**, speed factor **228x** | **$0.12** | "Audio is billed at a minimum of 10s per request." ~47 s wall-clock at 228x **[EXTRAPOLATION]** |
| [Groq](https://groq.com/pricing) | `whisper-large-v3` | **$0.111 / hr audio**, speed factor **217x** | **$0.33** | ~50 s wall-clock **[EXTRAPOLATION]** |
| [Deepgram](https://deepgram.com/pricing) | Nova-3 monolingual | **$0.0048 / min** (pay-as-you-go) | **$0.86** | = $0.288/hr **[EXTRAPOLATION: arithmetic]** |
| [Deepgram](https://deepgram.com/pricing) | Nova-3 multilingual | **$0.0058 / min** | **$1.04** | |
| [OpenAI](https://developers.openai.com/api/docs/pricing) | `gpt-4o-mini-transcribe` | **$0.003 / min** | **$0.54** | |
| [OpenAI](https://developers.openai.com/api/docs/pricing) | Whisper / `gpt-4o-transcribe` | **$0.006 / min** | **$1.08** | 25 MB per-request upload cap forces chunking |
| [OpenAI](https://developers.openai.com/api/docs/pricing) | `gpt-transcribe` | **$0.0045 / min** | **$0.81** | |
| [AssemblyAI](https://www.assemblyai.com/pricing) | Universal-2 | **$0.15 / hr** | **$0.45** | |
| [AssemblyAI](https://www.assemblyai.com/pricing) | Universal-3.5 Pro | **$0.21 / hr** | **$0.63** | + **$0.02/hr** standard diarization, or **$0.065/hr** experimental |

**The headline number: a 3-hour council meeting costs between $0.12 and $1.08 to transcribe via API, and lands in about a minute on Groq.** For a civic project archiving, say, 500 meetings a year across a dozen bodies, that is roughly **$60–$540/year**. This is small enough that "local Whisper to save money" is not a real argument; the arguments for local are sovereignty, no per-request rate limits, no vendor dependency, and offline reproducibility.

**Local routes on an Apple Silicon laptop — these are extrapolations and should be treated as such.**

I could not find a published, methodologically clean benchmark of `large-v3-turbo` under whisper.cpp+Metal on M-series hardware from a primary source. What is defensible from the primary sources above:

- whisper.cpp README: Core ML/ANE encoder is **">x3 faster than CPU-only"**; Metal runs inference "fully on the GPU."
- whisper.cpp #89: `large` CPU-only encoder on M1 Pro = 3,350 ms of compute per 30 s window ≈ **~9× faster than realtime**, encoder only, before any decoder cost.
- openai/whisper README: `turbo` is **~8×** the relative speed of `large`.

**[EXTRAPOLATION — compounding those three published factors, not a measured result]** `large-v3-turbo` under whisper.cpp with Metal on an M-series laptop should land somewhere in the **8×–20× realtime** band, i.e. a 3-hour meeting in roughly **9–22 minutes** of wall clock, drawing a few GB of unified memory. Treat that as an order-of-magnitude planning figure and **measure it on the actual target machine before committing to it in a scheduler**. The honest summary is: local is tens of minutes per meeting, API is about one minute per meeting, and local costs electricity instead of ~$0.12.

**Model download footprint** for a self-contained local install — exact byte sizes from the [ggerganov/whisper.cpp model repo on HuggingFace](https://huggingface.co/ggerganov/whisper.cpp):

| File | Bytes | ≈ |
|---|---:|---:|
| `ggml-base.bin` | 147,951,465 | 148 MB |
| `ggml-small.bin` | 487,601,967 | 488 MB |
| `ggml-medium.bin` | 1,533,763,059 | 1.53 GB |
| **`ggml-large-v3-turbo.bin`** | **1,624,555,275** | **1.62 GB** |
| `ggml-large-v3-turbo-q8_0.bin` | 874,188,075 | 874 MB |
| `ggml-large-v3-turbo-q5_0.bin` | 574,041,195 | 574 MB |
| `ggml-large-v3.bin` | 3,095,033,483 | 3.10 GB |
| `ggml-large-v3-turbo-encoder.mlmodelc.zip` (Core ML/ANE encoder) | 1,173,393,014 | 1.17 GB |

Note the last row: taking the ">x3 faster" Core ML/ANE path costs an **additional 1.17 GB** download on top of the GGUF, plus a slow first-run on-device compile.

## 5. Timestamps and long-audio handling

### 5.1 Word-level vs segment-level — support matrix

| Source | Word-level | Segment-level | Mechanism / note |
|---|---|---|---|
| YouTube `json3` captions | **Yes** | Yes | `segs[].tOffsetMs` relative to event `tStartMs` — **verified directly**, see §2.4 |
| YouTube `srt` / `vtt` captions | No | Yes | Lossy conversion; word offsets discarded |
| `youtube-transcript-api` | No | Yes | Returns `{text, start, duration}` only |
| openai/whisper | **Yes** (`word_timestamps=True`) | Yes | Source docstring: "Extract word-level timestamps using the cross-attention pattern and dynamic time warping" |
| faster-whisper | **Yes** (`word_timestamps=True`) | Yes | Exposes `segment.words` with `word.start` / `word.end` |
| whisper.cpp / whisper-rs | **Yes, experimental** | Yes | README: "Word-level timestamp (experimental) — The `--max-len` argument can be used to obtain word-level timestamps. Simply use `-ml 1`". Also a DTW-based token-timestamp path. |
| WhisperX | **Yes, most accurate** | Yes | wav2vec2 **forced phoneme alignment**, not attention heuristics |
| NVIDIA Parakeet TDT | **Yes** | Yes | Native "word-level timestamp predictions" |
| OpenAI API | **Only on `whisper-1`** | Yes | `timestamp_granularities[]` is "only supported for `whisper-1`" — the newer `gpt-4o-transcribe` family does **not** offer word timestamps |
| Groq API | **Yes** | Yes | `timestamp_granularities[]` accepts `word` and `segment`, and both simultaneously |
| Deepgram / AssemblyAI | **Yes** | Yes | Word-level with confidence by default |

**Sharp edge for this project:** if you route to the OpenAI API, the cheap modern models (`gpt-4o-mini-transcribe` at $0.003/min) **cannot give you word timestamps**. Only legacy `whisper-1` can. Groq gives word timestamps at $0.04/hr with no such restriction, which makes Groq the better API fit for a timestamp-retaining archive.

### 5.2 The 30-second window and how long audio is handled

Whisper's encoder takes a fixed **30-second** log-Mel input. Everything else is scaffolding. From the [openai/whisper README](https://github.com/openai/whisper): "the `transcribe()` method reads the entire file and processes the audio with a sliding 30-second window, performing autoregressive sequence-to-sequence predictions on each window."

The window advance is determined by the *model's own predicted timestamp tokens*, which is where long-form drift comes from: one bad timestamp prediction shifts the window, and every subsequent window inherits the error. The [WhisperX paper (arXiv:2303.00747)](https://arxiv.org/abs/2303.00747) states the failure mode plainly:

> "their application to long audio transcription via buffered or sliding window approaches is prone to **drifting, hallucination & repetition**; and prohibits batched transcription due to their sequential nature. Further, timestamps corresponding each utterance are prone to inaccuracies and word-level timestamps are not available out-of-the-box."

Chunking strategies in practice, in increasing order of quality:

1. **Naive fixed chunks** (what you must do to satisfy API upload limits) — cuts mid-sentence, loses context at every boundary. OpenAI's own guidance warns against it: split into "chunks of 25 MB or less" but avoid splitting mid-sentence "as this can remove context and reduce accuracy."
2. **Sequential sliding window** (reference Whisper) — correct but slow and drift-prone.
3. **VAD-segmented chunks** (WhisperX "Cut & Merge", faster-whisper `vad_filter`) — cut only at detected silence, then merge segments up to ~30 s. No mid-word cuts, and it unlocks batching.

**Upload-limit reality check for a 3-hour meeting** — this forces chunking on the API path regardless:

| Service | Max upload | 3-hour meeting at 16 kHz mono | Chunking required? |
|---|---|---|---|
| OpenAI | **25 MB** | ~346 MB PCM / ~90 MB FLAC **[EXTRAPOLATION]** | **Yes, always** |
| Groq | **25 MB free / 100 MB dev tier** | as above | **Yes on free tier; borderline on dev** |

Groq's docs note it "automatically downsamples audio to 16 kHz mono" and recommends FLAC, plus a client-side ffmpeg preprocessing step. Doing that downsample locally before upload is both cheaper and necessary to fit the cap.

### 5.3 Hallucination on silence and long audio — documented, and directly on-mission

This is not folklore. It is in the peer-reviewed literature, and it is *in Whisper's own source code as a named mitigation parameter*.

**Primary literature — Koenecke et al., "Careless Whisper: Speech-to-Text Hallucination Harms," [arXiv:2402.08021](https://arxiv.org/abs/2402.08021) (FAccT 2024).** Verbatim from the abstract:

> "we find that roughly **1% of audio transcriptions contained entire hallucinated phrases or sentences which did not exist in any form in the underlying audio**. We thematically analyze the Whisper-hallucinated content, finding that **38% of hallucinations include explicit harms such as perpetuating violence, making up inaccurate associations, or implying false authority**. […] We find that **hallucinations disproportionately occur for individuals who speak with longer shares of non-vocal durations** — a common symptom of aphasia."

**Why this is squarely on-mission for Centinel.** The paper's causal finding is that hallucination correlates with **non-vocal duration**, not with the speaker's disability per se. A city council recording is a near-worst case for exactly that variable: gavel-to-gavel recordings routinely contain long stretches of dead air during roll call, document distribution, recesses, executive session pauses, and waiting for a speaker to reach the podium. A transcript that invents sentences during a recess, timestamped and archived as a public record, is a materially worse failure than no transcript at all.

**Whisper's own acknowledged mitigations**, quoted verbatim from [`whisper/transcribe.py`](https://github.com/openai/whisper/blob/main/whisper/transcribe.py):

- `hallucination_silence_threshold` — "When word_timestamps is True, **skip silent periods longer than this threshold (in seconds) when a possible hallucination is detected**." Default: `None` — **off by default.**
- `condition_on_previous_text` (default `True`) — "if True, the previous output of the model is provided as a prompt for the next window; disabling may make the text inconsistent across windows, but **the model becomes less prone to getting stuck in a failure loop, such as repetition looping or timestamps going out of sync**."
- `compression_ratio_threshold` (default `2.4`) — "If the gzip compression ratio is above this value, treat as failed." The source comment on that branch is literally `needs_fallback = True  # too repetitive`.
- `no_speech_threshold` (default `0.6`) — "If the no_speech probability is higher than this value AND the average log probability over sampled tokens is below `logprob_threshold`, consider the segment as silent."

WhisperX sets `--condition_on_prev_text` to **`False` by default** specifically because it "reduces hallucination."

[Distil-Whisper (arXiv:2311.00430)](https://arxiv.org/abs/2311.00430) claims its distilled models are "**less prone to hallucination errors on long-form audio**" than Whisper — a qualitative claim in the abstract, not quantified there.

There is also ongoing upstream work: openai/whisper PR [#2795 "fix: don't suppress fallback when output is a repetition loop"](https://github.com/openai/whisper/pull/2795) was opened 2026-06-21 and remains open, and issue [#2677 "feat: Add advanced hallucination detection and confidence scoring system"](https://github.com/openai/whisper/issues/2677) has been open since 2025-10-19. The problem is live.

**Design implication:** for this workload, turn on VAD, set `condition_on_previous_text=False`, and set a `hallucination_silence_threshold`. Do not run stock defaults.

### 5.4 VAD preprocessing — published figures

**The headline published number** is from the [WhisperX paper (arXiv:2303.00747)](https://arxiv.org/abs/2303.00747), verbatim:

> "we show that pre-segmenting audio with our proposed **VAD Cut & Merge strategy improves transcription quality and enables a twelve-fold transcription speedup via batched inference**."

The WhisperX README states the end-to-end result as **"70x realtime transcription using whisper large-v2"** with batched inference, and describes the mechanism as "VAD preprocessing, reduces hallucination & batching with no WER degradation."

Note carefully what the 12× is and is not: it is the speedup from *VAD-enabled batching*, not from removing silence per se. Two distinct effects are bundled:

1. **Silence removal** — audio that contains no speech is never fed to the model. The saving scales directly with the dead-air fraction of the recording. For a recording that is 30% dead air, this alone is roughly a 30% runtime reduction. **[EXTRAPOLATION — arithmetic, not a published measurement, and I have not measured the dead-air fraction of any actual council recording. Do not treat 30% as a measured figure.]**
2. **Batching** — VAD-derived segment boundaries let you run many ≤30 s chunks in parallel instead of sequentially. This is where the bulk of the 12× comes from.

**Silero VAD** is the VAD both faster-whisper and WhisperX use. Published characteristics from the [Silero VAD README](https://github.com/snakers4/silero-vad), verbatim:

- **"One audio chunk (30+ ms) takes less than 1ms to be processed on a single CPU thread."** VAD cost is negligible against transcription cost.
- "Under certain conditions ONNX may even run up to **4-5x faster**."
- "JIT model is around **two megabytes** in size."
- Supports **8000 Hz and 16000 Hz** sampling rates.
- Trained on corpora covering "over **6000** languages."
- **MIT licence** — "no telemetry, no keys, no registration."

faster-whisper's integration, from its README: "The library integrates the Silero VAD model to filter out parts of the audio without speech… **The default behavior is conservative and only removes silence longer than 2 seconds.**" It is enabled via `vad_filter=True`, and "Vad filter is enabled by default for batched transcription." The 2-second default is tunable via `vad_parameters=dict(min_silence_duration_ms=...)`.

**Rust note:** Silero VAD is an ONNX model, and it runs fine in Rust via `ort` (ONNX Runtime bindings) — the 2 MB model and the ONNX runtime are both available without Python. This is a second place where the Rust story is genuinely fine.

---

## 6. Legal and ToS posture

Factual reporting only. This is not legal advice.

### 6.1 What YouTube's Terms of Service say

From YouTube's [Terms of Service](https://www.youtube.com/static?template=terms), "Permissions and Restrictions," verbatim:

> "access the Service using any automated means (such as robots, botnets or scrapers) except (a) in the case of public search engines, in accordance with YouTube's robots.txt file; or (b) with YouTube's prior written permission"

> "circumvent, disable, fraudulently engage with, or otherwise interfere with any part of the Service (or attempt to do any of these things), including security-related features"

Downloading is restricted "except: (a) as expressly authorized by the Service; or (b) with prior written permission from YouTube."

**Plainly: using `yt-dlp` against YouTube is contrary to YouTube's ToS.** That is a contract matter between the operator and YouTube, and the practical enforcement mechanism is technical (IP blocks, bot-detection challenges, PO Token requirements — see §2.5), not litigation against individual users. Using the **Data API v3 with a key**, and using the **RSS feeds**, are both sanctioned access paths; only the yt-dlp/InnerTube path is not.

### 6.2 Copyright and anti-circumvention — the youtube-dl precedent

In October 2020 the RIAA sent GitHub a DMCA §1201 anti-circumvention notice targeting youtube-dl. GitHub took it down, then **reinstated it on November 16, 2020**. From [GitHub's post](https://github.blog/news-insights/policy-news-and-insights/standing-up-for-developers-youtube-dl-is-back/):

> "the project does not in fact violate the DMCA's anticircumvention prohibitions."

> "just because code can be used to access copyrighted works doesn't mean it can't also be used to access works in non-infringing ways."

GitHub's own enumeration of legitimate uses is directly relevant to a civic-transparency tool — the post cites "changing playback speeds for accessibility, **preserving evidence in the fight for human rights, aiding journalists in fact-checking**, and downloading Creative Commons-licensed or public domain videos."

[EFF](https://www.eff.org/deeplinks/2020/11/riaa-abuses-dmca-take-down-popular-tool-downloading-online-video), which represented the maintainers, framed it as DMCA misuse: "youtube-dl doesn't use RIAA-member labels' music in any way. The makers of youtube-dl simply shared information with the public about how to perform a certain task—one with many completely lawful applications."

### 6.3 The one live adverse ruling

German major labels (Universal, Warner, Sony) sued **Uberspace**, the host of `youtube-dl.org`, in 2022. The Hamburg Regional Court ruled against Uberspace in March 2023, and the **Hamburg Higher Regional Court (OLG Hamburg) rejected the appeal on 27 November 2024** — as reported by [heise online](https://www.heise.de/en/news/OLG-Hamburg-Uberspace-liable-for-hosting-Youtube-DL-10179284.html) and [TorrentFreak](https://torrentfreak.com/court-rejects-appeal-of-youtube-dl-hosting-provider-uberspace-241127-1/).

Scope, accurately stated: the ruling concerns **hosting liability in Germany for a site that advertises the tool**. It does not outlaw the tool, does not apply outside Germany, and does not concern end users. Reported as secondary coverage; German court decisions are not readily available as primary documents.

### 6.4 How the public-records / archival space actually handles this

Patterns observable from the sources above and from how comparable projects operate:

- **Prefer sanctioned APIs where they suffice.** Data API v3 + RSS covers discovery and metadata entirely within ToS. Only transcript/audio retrieval needs the unsanctioned path — which is exactly the gap §1.1 identified (`captions.download` requires video ownership).
- **Ask the body directly.** Council recordings are frequently *also* available from the municipality's own agenda/video system (Granicus, Legistar, Swagit, CivicClerk, Vimeo). A `.gov`-first fetch is both ToS-clean and often higher-quality, and it fits Centinel's existing `.gov` surface model. This is the strongest mitigation available and should be the first-choice source where it exists.
- **Public-records status of the content is a separate question from access-method compliance.** Municipal meeting recordings are typically public records under state open-meetings/sunshine law and are often expressly non-copyrighted or freely licensed; that says nothing about whether scraping YouTube's servers complies with YouTube's contract. Both questions have to be answered independently. Note that 17 U.S.C. §105 (no copyright in US Government works) applies to **federal** works only — it does **not** automatically cover municipal output, which varies by state.
- **Behave well technically.** Low request rates, honest identification, caching aggressively so the same video is fetched once, and content-addressed retention so re-fetches are unnecessary. Centinel's content-addressed design already does most of this.
- **Retain provenance.** Record which path produced each artefact (API / RSS / yt-dlp / municipal system), the tool version, and the fetch timestamp. This matters for both reproducibility and for demonstrating good faith.

---

## 7. What this means for the language decision

**Not picking. Here is the honest shape of the tradeoff.**

### 7.1 Capability by stage

| Pipeline stage | Rust | Python | TypeScript |
|---|---|---|---|
| Data API v3 enumeration | **Fine** — `google-youtube3`, current | **Best** — official client | **Best** — official `googleapis` |
| RSS delta polling | **Fine** — any XML crate | Fine | Fine |
| Non-API channel enumeration | **Subprocess `yt-dlp`** | **Native** — `yt-dlp` in-process | **Native** — `youtubei.js` |
| Caption/transcript fetch | **Subprocess `yt-dlp`** (only crate is a dead `0.1.0` from 2023) | **Native** — `yt-dlp` and/or `youtube-transcript-api` | **Native** — `youtubei.js`, `youtube-transcript` |
| Audio URL resolution | **Subprocess `yt-dlp`** | Native | **Native** — `youtubei.js` |
| Audio decode / 16 kHz resample | **Native** — `symphonia` + `rubato`, no ffmpeg needed | ffmpeg | ffmpeg |
| VAD (Silero) | **Native** — `ort` ONNX | Native | Native (onnxruntime-node) |
| Local Whisper | **Native** — `whisper-rs` 0.16.0, 830k downloads, FFI to whisper.cpp | **Native, richest** — faster-whisper, WhisperX, openai/whisper, NeMo/Parakeet | **Weak** — `nodejs-whisper` shells out to a binary; the one real N-API binding (`smart-whisper`) is stale since 2024-10 |
| Diarization + forced alignment | **None** | **Only option** — WhisperX / pyannote | **None** |
| Cloud transcription APIs | Fine (HTTP) | Fine | Fine |

### 7.2 The three findings that actually drive the decision

**1. Rust's weakness is entirely on the YouTube-facing side, and it is total.** Not "thinner," not "less mature" — the only Rust transcript crate is a single `0.1.0` release from May 2023 with 2,690 lifetime downloads. `rustube` died in 2022, `rusty_ytdl` in 2024, and the `youtube_dl` crate self-describes as "Runs yt-dlp and parses its JSON output." A Rust Centinel shells out to `yt-dlp` for every caption fetch and every audio URL, period. That is not a temporary gap someone will fill; nobody has filled it in four years, because keeping pace with YouTube's changes requires the release cadence documented in §2.5 (26 releases in 2025) and no Rust crate has ever sustained that.

**2. Rust's strength is entirely on the transcription side, and it is real.** `whisper-rs` at 0.16.0 with 830k downloads and a 2026-03 release is genuinely healthy, links whisper.cpp natively with Metal, and needs no Python. Silero VAD runs via `ort`. `symphonia` + `rubato` can eliminate even the ffmpeg dependency for decode/resample. If the local-transcription path is where the operator expects the engineering weight to sit, Rust is a good home for it.

**3. The thing Rust and TypeScript both entirely lack is diarization.** WhisperX + pyannote is the only mainstream way to get "who said what, when" with forced-aligned word timestamps. For **city council meetings specifically** — many speakers, alternating council members and public commenters, where attributing a statement to the right person is the whole point — this is arguably the single highest-value feature in the document, and it exists in Python only.

### 7.3 The shapes this suggests

- **All-Python.** Only configuration where every stage is native. Gets `yt-dlp` in-process (a real operational advantage: you can catch and inspect extraction errors as exceptions rather than parsing subprocess stderr), gets WhisperX diarization, gets Parakeet if NVIDIA hardware ever appears. Cost: Python packaging for a distributable CLI/server, and no Metal for faster-whisper on Apple Silicon.
- **All-TypeScript.** `youtubei.js` is the only credible non-Python InnerTube client and it is well-maintained (v17.2.0, 183k weekly). Strong for the YouTube side, strong for the server and MCP surfaces. Weak-to-absent for local Whisper, absent for diarization — realistically pairs with a cloud transcription API, which §4.5 shows costs $0.12–$1.08 per 3-hour meeting anyway.
- **All-Rust.** Excellent local transcription, excellent audio handling, clean single-binary distribution, and a fine Data API client. Pays for it with a hard `yt-dlp` subprocess dependency on the YouTube side and no diarization at all.
- **Rust core + subprocess boundary, made explicit.** If the operator's lean toward Rust holds, the honest framing is: `yt-dlp` is not an implementation detail you might remove later, it is a **permanent runtime dependency with a ≤90-day staleness budget** (yt-dlp warns at 90 days; §2.5). Design it as a first-class, versioned, monitored external tool — pin it, health-check it, ship an update path, and treat its output as an untrusted parse surface. Do that and Rust works. Pretend it will go away and it will hurt.

### 7.4 Two decisions that are independent of language

- **Discovery should be Data API + RSS, never `search.list`.** Quota is a non-issue on the `playlistItems`/`videos` path (~80 units for a 2,000-video channel) and hard-capped at 100 calls/day on `search.list`. Every language can do this natively.
- **Transcription should default to a cloud API with a local fallback, not the reverse** — unless sovereignty is a stated requirement. $0.12 per 3-hour meeting on Groq with word timestamps at ~228× realtime is hard to beat with a laptop that takes 9–22 minutes **[EXTRAPOLATION, §4.5]**. Keep the local path for offline reproducibility and for when the API is unavailable or the content is sensitive.

