# Transcription

Speech to text, for recordings with no captions. Whisper, on the server
`[inference.transcribe]` names.

```bash
centinel transcribe
```

The work list is the usual subtraction: blobs derived by the transcriber, subtracted from
the audio blobs. A transcript an earlier release's in-process worker wrote counts — moving
to a server re-transcribes nothing.

## Captions first, audio second

A YouTube video is one address holding up to three artifacts: metadata, captions and
audio. Captions are free, already text, and usually good enough. So audio is only fetched
when there are none, which is what `audio_if_no_captions` in a source's definition controls.

About 7% of a real council channel has no captions and never will. That is also why
resumption keys on the **metadata** artifact rather than on captions — keying on captions
would re-fetch a whole catalogue every run.

## The server

The request is OpenAI's `POST {base_url}/audio/transcriptions`, a form with the audio file
and `response_format=verbose_json`, and the answer is a list of timed segments.
`whisper.cpp`'s `whisper-server` speaks it when started with
`--inference-path /v1/audio/transcriptions`; faster-whisper's servers, LocalAI and OpenAI's
own speak it out of the box. An answer without timed segments is refused, and ends the
stage: an untimed transcript would cite every passage at `00:00:00`.

| Role | Model | Notes |
|---|---|---|
| Transcription | `whisper-large-v3-turbo` Q8_0 | near-large accuracy at about 8× the speed |
| Voice activity | `silero-vad` v5.1.2 | 885 KB — keeps Whisper from inventing words over dead air |

`whisper-tiny`, at 39M parameters, is a smoke test for a server, not an archive.

**Run the server with voice activity detection** — `whisper-server --vad --vad-model …`,
or faster-whisper's `vad_filter`. It is not optional polish. Whisper hallucinates
confidently over silence, a council recording is mostly silence, and a hallucinated sentence
in a meeting transcript is exactly the kind of false record this whole system is built to
refuse. Centinel cannot see whether the server ran it, so the setup is where it is decided.

Transcription used to run in a second binary, `centinel-whisper`, because `whisper.cpp` and
`llama.cpp` each vendor a copy of ggml whose ~534 exported symbols collide when linked
together — measured, the combined binary transcribed nothing and said nothing. With no
model in Centinel, that binary is gone.

## Audio handling

`yt-dlp` fetches. `ffmpeg` decodes to a 16 kHz mono WAV — the format every server reads,
at the rate Whisper resamples to anyway, and half the upload of the source's rate. Both are
external programs, so both go through the module that owns child processes — each carries a
bound, dies with its caller, and never reads our stdin.

The WAV is a scratch file rather than a stream, because a request that fails is sent again
and a pipe can be read once. A server answers nothing until it has transcribed the whole
recording, so the request's deadline is sized by the audio: ten minutes plus three times its
length.

## What the transcript carries

A derived blob, linked to the audio blob by a `Derivation` that names the tool
(`transcription`), Centinel's version, and the **model tier**: the model the server was
asked for, and its `host:port`. Output quality varies by model — so the tier that produced
an artifact is part of its provenance, not an implementation detail.

The video's title is written into the transcript text as a heading, for the same reason
a page's title is: a recording called *"Mayor Castor 2026 Budget"* never says "Castor"
aloud, and only the text is searched.

## Not built yet

**Transcript-aware chunking.** Agenda-aligned spans and per-chunk timestamps are what turn
a search hit into a `watch?v=X&t=4271s` citation. Today a transcript is chunked like any
other text.

Next: [Chunking and the index](index.md).
