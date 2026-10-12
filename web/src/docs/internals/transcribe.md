# Transcription

Speech to text, for recordings with no captions. Local Whisper, in a separate process.

```bash
centinel transcribe
```

The work list is the usual subtraction: blobs derived by the transcriber, subtracted from
the audio blobs.

## Captions first, audio second

A YouTube video is one address holding up to three artifacts: metadata, captions and
audio. Captions are free, already text, and usually good enough. So audio is only fetched
when there are none, which is what `audio_if_no_captions` in a `[[source]]` block controls.

About 7% of a real council channel has no captions and never will. That is also why
resumption keys on the **metadata** artifact rather than on captions — keying on captions
would re-fetch a whole catalogue every run.

## Why it is a separate binary

`whisper.cpp` and `llama.cpp` each vendor their own copy of **ggml**, and both export the
same ~534 `ggml_*` symbols. Linked into one binary, the linker keeps one copy and silently
resolves the other library's calls to it. The two versions are not the same.

Measured on identical audio and model, the linked crates the only variable:

| binary | result |
|---|---|
| `whisper-rs` alone | 2 segments — *"The council meeting will come to order."* |
| `whisper-rs` + `llama-cpp-2` | **0 segments**, every token at `p=0.000` |

It links without a warning, runs without a crash, and transcribes nothing. There is no
error to catch.

So `centinel` links `llama.cpp`, `centinel-whisper` links `whisper.cpp`, and the two meet
over a pipe. `centinel` finds the worker beside itself first, then
`$CENTINEL_WHISPER_BIN`, then `PATH`.

This is not a preference. It is the reason there are two binaries at all, and installing
only one leaves the pipeline silently broken in a way that produces no error.

## The models

| Role | Model | Notes |
|---|---|---|
| Transcription | `whisper-large-v3-turbo` Q8_0 | near-large accuracy at about 8× the speed |
| Voice activity | `silero-vad` v5.1.2 | 885 KB — keeps Whisper from inventing words over dead air |

`whisper-tiny` is also in the registry, at 39M parameters. It is a smoke test for the
pipeline, not an archive.

Q8_0 over f16 here for a different reason than the embedder's: near-lossless at half the
download.

Voice activity detection is not optional polish. Whisper hallucinates confidently over
silence, and a hallucinated sentence in a meeting transcript is exactly the kind of false
record this whole system is built to refuse.

## Audio handling

`ffmpeg` decodes to 16 kHz mono PCM, which is what Whisper wants. `yt-dlp` fetches. Both
are external programs, so both go through the module that owns child processes — each
carries a bound, dies with its caller, and never reads our stdin.

Transcription is the job that needs a **stall timeout** rather than a deadline. Its honest
duration is hours, so bounding total time would kill working runs. Bounding *silence*
works: a run still reporting progress after four hours is fine; one that has said nothing
for ten minutes is wedged.

The worker's stderr is both its diagnostics and its heartbeat, which is why the stall
timer resets on any line rather than only on a progress report.

## What the transcript carries

A derived blob, linked to the audio blob by a `Derivation` that names the tool, its
version and the **model tier**. Because everything runs locally, output quality varies by
machine — so the tier that produced an artifact is part of its provenance, not an
implementation detail.

The video's title is written into the transcript text as a heading, for the same reason
a page's title is: a recording called *"Mayor Castor 2026 Budget"* never says "Castor"
aloud, and only the text is searched.

## Not built yet

**Transcript-aware chunking.** Agenda-aligned spans and per-chunk timestamps are what turn
a search hit into a `watch?v=X&t=4271s` citation. Today a transcript is chunked like any
other text.

Next: [Chunking and the index](index.md).
