//! Centinel's transcription worker.
//!
//! Reads 16 kHz mono `f32` PCM on stdin, writes one JSON transcript on stdout. That is
//! the whole contract, and it is deliberately narrow: this binary exists to keep
//! whisper.cpp's `ggml` out of the same address space as llama.cpp's (see `Cargo.toml`),
//! so the less it knows the better. Acquisition, decoding and storage all stay in
//! `centinel`; this end owns only "samples in, segments out".
//!
//! ## The defaults are not whisper's defaults
//!
//! Koenecke et al. (*Careless Whisper*, FAccT 2024) measured Whisper fabricating entire
//! sentences in ~1% of transcriptions, 38% of those carrying explicit harms, and found
//! the effect tracks **non-vocal duration**. A gavel-to-gavel council recording is close
//! to a worst case for that variable — roll call, recesses, waiting for a speaker to
//! reach the podium. An invented sentence, timestamped and filed as a public record, is
//! a worse outcome than no transcript at all.
//!
//! The mitigations are all off or weak upstream, so this binary turns them on: VAD so
//! silence never reaches the decoder, `no_context` so a hallucination cannot seed the
//! next window, and a temperature-fallback ladder so a repetition loop is retried rather
//! than emitted. See [`configure`].

use std::io::Read;

use anyhow::{Context, Result};
use clap::Parser;
use serde::Serialize;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// The only sample rate Whisper accepts. Every implementation resamples to it, so the
/// caller does the resampling once rather than us doing it badly.
const SAMPLE_RATE: u32 = 16_000;

#[derive(Parser, Debug)]
// `version` is not decoration. The two binaries are a matched pair — `centinel` finds this
// one beside itself, and an installer that replaces one and not the other leaves a pair
// that does not agree. Asking is how anything checks. It is also the cheapest proof that a
// downloaded binary runs at all on the host it landed on: the process starts, the dynamic
// linker resolves whisper.cpp's libraries, and it prints a string.
#[command(
    name = "centinel-whisper",
    version = env!("CARGO_PKG_VERSION"),
    about = "Transcribe 16kHz mono f32 PCM from stdin. Internal to centinel."
)]
struct Args {
    /// GGML weights, e.g. `ggml-large-v3-turbo-q8_0.bin`.
    #[arg(long)]
    model: String,

    /// Silero VAD weights. Strongly recommended: without it the decoder sees dead air.
    #[arg(long)]
    vad_model: Option<String>,

    /// Spoken language. Omit to auto-detect.
    #[arg(long)]
    language: Option<String>,

    /// Decode threads. Defaults to the machine's parallelism.
    #[arg(long)]
    threads: Option<u16>,

    /// Emit `progress <percent>` lines on stderr. A 3-hour meeting is tens of minutes.
    #[arg(long)]
    progress: bool,
}

#[derive(Serialize)]
struct Segment {
    start_ms: i64,
    end_ms: i64,
    text: String,
    /// Whisper's own confidence that this span is silence. Retained rather than used as
    /// a filter: a caller auditing a suspect passage should see what the model thought,
    /// and a threshold applied here would be invisible downstream.
    no_speech_prob: f32,
}

#[derive(Serialize)]
struct Transcript {
    whisper_version: String,
    /// Detected or forced, whichever applied.
    language: String,
    /// Whether VAD was actually in use. Recorded because a transcript produced without it
    /// carries a materially different hallucination risk, and provenance should say so.
    vad: bool,
    sample_count: usize,
    duration_ms: u64,
    segments: Vec<Segment>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // whisper.cpp is chatty on stderr and stderr is our progress channel; stdout must
    // stay a clean JSON document either way.
    whisper_rs::install_logging_hooks();

    let pcm = read_pcm().context("reading PCM from stdin")?;
    anyhow::ensure!(
        !pcm.is_empty(),
        "no audio on stdin — expected 16kHz mono f32 little-endian samples"
    );

    // Prove the VAD loads *before* claiming it ran. whisper.cpp accepts a corrupt or
    // empty `vad_model_path` silently, transcribes without VAD and exits 0 — verified
    // with 885 KB of `/dev/urandom` and with `/dev/null`. Reporting `vad: true` in that
    // case would put a false claim in the provenance record at exactly the moment the
    // hallucination risk it is supposed to describe is highest.
    if let Some(vad) = &args.vad_model {
        whisper_rs::WhisperVadContext::new(vad, whisper_rs::WhisperVadContextParams::default())
            .map_err(|e| {
                anyhow::anyhow!(
                    "VAD weights at {vad} could not be loaded ({e:?}). \
                     Refusing to transcribe: whisper.cpp would silently proceed without \
                     VAD, and the transcript would claim otherwise."
                )
            })?;
    }

    let ctx = WhisperContext::new_with_params(&args.model, WhisperContextParameters::default())
        .with_context(|| format!("loading whisper weights from {}", args.model))?;
    let mut state = ctx.create_state().context("creating whisper state")?;

    let params = configure(&args);
    state
        .full(params, &pcm)
        .context("whisper inference failed")?;

    let mut segments = Vec::new();
    for i in 0..state.full_n_segments() {
        let Some(seg) = state.get_segment(i) else {
            continue;
        };
        let text = seg.to_str_lossy().unwrap_or_default().trim().to_string();
        if text.is_empty() {
            continue;
        }
        segments.push(Segment {
            // whisper.cpp reports timestamps in centiseconds.
            start_ms: seg.start_timestamp() * 10,
            end_ms: seg.end_timestamp() * 10,
            text,
            no_speech_prob: seg.no_speech_probability(),
        });
    }

    let language = whisper_rs::get_lang_str(state.full_lang_id_from_state())
        .unwrap_or("unknown")
        .to_string();

    let transcript = Transcript {
        whisper_version: whisper_rs::get_whisper_version().to_string(),
        language,
        vad: args.vad_model.is_some(),
        sample_count: pcm.len(),
        duration_ms: (pcm.len() as u64 * 1000) / SAMPLE_RATE as u64,
        segments,
    };

    println!("{}", serde_json::to_string(&transcript)?);
    Ok(())
}

/// Decoding parameters, chosen against hallucination rather than against benchmarks.
///
/// Each departure from upstream defaults is deliberate:
///
/// | setting | upstream | here | why |
/// |---|---|---|---|
/// | VAD | off | on | silence never reaches the decoder — the measured cause |
/// | `no_context` | false | **true** | a hallucination cannot seed the next window |
/// | `temperature_inc` | 0.2 | 0.2 | keeps the fallback ladder that retries a repetition loop |
/// | `no_speech_thold` | 0.6 | 0.6 | upstream's is already right; pinned so a bump is visible |
///
/// `no_context` is the one with a real cost: windows lose the previous window's text as
/// a prompt, which slightly hurts consistency of proper nouns across a boundary. WhisperX
/// takes the same trade by default, and for a public record a less fluent transcript
/// beats a more fluent invented one.
fn configure(args: &Args) -> FullParams<'static, 'static> {
    // Greedy over beam search: whisper.cpp's beam path is materially slower and the
    // quality difference is small next to the effect of VAD.
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });

    let threads = args.threads.map(i32::from).unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(|n| n.get() as i32)
            .unwrap_or(4)
    });
    params.set_n_threads(threads);

    // Leaked so the params can hold a `&'static str`. One allocation for the process
    // lifetime, which is the honest shape for a one-shot worker.
    if let Some(lang) = &args.language {
        params.set_language(Some(Box::leak(lang.clone().into_boxed_str())));
    } else {
        params.set_detect_language(true);
    }

    params.set_no_context(true);
    params.set_no_speech_thold(0.6);
    params.set_temperature_inc(0.2);
    params.set_suppress_blank(true);

    // Never translate. A Spanish public comment must be archived as Spanish; translating
    // it silently would put words in a speaker's mouth in the record.
    params.set_translate(false);

    // whisper.cpp prints its own transcript otherwise, which would corrupt stdout.
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_special(false);
    params.set_print_timestamps(false);

    if let Some(vad) = &args.vad_model {
        params.set_vad_model_path(Some(Box::leak(vad.clone().into_boxed_str())));
        // Order matters: `enable_vad` panics unless the path is already set.
        params.enable_vad(true);

        let mut vad_params = whisper_rs::WhisperVadParams::default();
        // Upstream's 100 ms is tuned for conversation. A council chamber pauses far
        // longer than that between speakers, and splitting on every such pause would
        // shatter a statement into fragments too small to retrieve.
        vad_params.set_min_silence_duration(500);
        // Keep the leading consonant of the first word after a pause.
        vad_params.set_speech_pad(100);
        params.set_vad_params(vad_params);
    }

    if args.progress {
        params.set_progress_callback_safe(|percent: i32| {
            eprintln!("progress {percent}");
        });
    }

    params
}

/// Reads raw little-endian `f32` samples from stdin.
///
/// A trailing partial sample means the producer was cut off mid-write, which would
/// otherwise show up as a plausible transcript of subtly wrong audio.
fn read_pcm() -> Result<Vec<f32>> {
    let mut bytes = Vec::new();
    std::io::stdin().lock().read_to_end(&mut bytes)?;

    anyhow::ensure!(
        bytes.len() % 4 == 0,
        "got {} bytes, which is not a whole number of f32 samples — \
         the audio stream was truncated",
        bytes.len()
    );

    Ok(bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect())
}
