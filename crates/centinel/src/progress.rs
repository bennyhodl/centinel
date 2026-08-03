//! Rendering [`ProgressEvent`]s for a human.
//!
//! The op never learns who invoked it — it emits events and this module decides what a
//! terminal should look like. That separation is why a 1.2 GB download can grow a
//! multi-bar display without a single line changing in
//! [`centinel_core::models::download`].
//!
//! ## Two renderers, chosen by whether stderr is a terminal
//!
//! Bars are for people. Piped into a file or a CI log, `indicatif` would either emit
//! nothing (its non-terminal draw target is silent) or thousands of redraw lines. So a
//! non-terminal stderr gets the plain line-per-event renderer instead, and the byte-level
//! updates are dropped from it — 613 MB at one event per 512 KiB is 1,200 lines nobody
//! wants in a log, and the only interesting ones are the completions.
//!
//! ## Reading the event stream
//!
//! | Event | Meaning | Rendering |
//! |---|---|---|
//! | no `id` | a log line | printed above the bars |
//! | `id` + `total` | a track | one bar per `id` |
//! | `id`, `done == total` | that track finished | bar cleared, a `✓` line printed |
//!
//! `id` is what makes a multi-file download legible: the aggregate bar and the current
//! file's bar are different tracks, so neither has to be inferred from message text.

use std::collections::HashMap;
use std::io::IsTerminal;

use centinel_core::op::{ProgressEvent, TOTAL_TRACK, Unit};
use indicatif::{HumanBytes, MultiProgress, ProgressBar, ProgressStyle};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::task::JoinHandle;

/// Only emitted when stderr is a terminal, so escape codes can never reach a pipe.
const GREEN_CHECK: &str = "\x1b[32m✓\x1b[0m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

/// How much of a long label to keep. Wide enough for `qwen3-embedding-0.6b tokenizer.json`.
const LABEL_WIDTH: usize = 42;

/// Drains progress events until the sender is dropped.
///
/// Returns a handle rather than blocking: the op runs concurrently, and the renderer
/// terminates on its own when the op drops its [`centinel_core::op::Progress`].
pub fn spawn(rx: UnboundedReceiver<ProgressEvent>) -> JoinHandle<()> {
    if std::io::stderr().is_terminal() {
        tokio::spawn(render_bars(rx))
    } else {
        tokio::spawn(render_lines(rx))
    }
}

/// The track an op with no `id` gets. `collect` and `extract` predate ids and emit a
/// bare counter; they deserve a bar, not a thousand log lines.
const IMPLICIT_TRACK: &str = "";

async fn render_bars(mut rx: UnboundedReceiver<ProgressEvent>) {
    let multi = MultiProgress::new();
    let mut bars: HashMap<String, ProgressBar> = HashMap::new();

    while let Some(event) = rx.recv().await {
        let Some(total) = event.total else {
            let _ = multi.println(format!("{DIM}·{RESET} {}", event.message));
            continue;
        };
        let named = event.id.is_some();
        let id = event.id.as_deref().unwrap_or(IMPLICIT_TRACK);
        let done = event.done.unwrap_or(0);
        let is_total = id == TOTAL_TRACK;

        let bar = bars.entry(id.to_string()).or_insert_with(|| {
            // The aggregate sits at the *bottom*, under whatever it is summing — the
            // order `cargo` has trained people to read. It is created lazily like any
            // other bar, so pushing every other track above it keeps that order stable
            // no matter which bar happened to appear first.
            let bar = if is_total {
                multi.add(ProgressBar::new(total))
            } else {
                multi.insert(0, ProgressBar::new(total))
            };
            bar.set_style(style_for(event.unit, is_total));
            bar
        });

        // Set rather than increment: events are throttled and lossy by design, so an
        // absolute position is the only one that survives a dropped update.
        bar.set_length(total);
        bar.set_position(done);
        bar.set_prefix(if is_total {
            "total".to_string()
        } else {
            truncate(&event.message, LABEL_WIDTH)
        });

        // A finished *named* track becomes a static line, so a ten-file pull does not
        // grow ten live bars. The implicit track is one long counter with nothing after
        // it, and the aggregate outlives every file — neither should vanish here.
        if done >= total && named && !is_total {
            bar.finish_and_clear();
            let size = match event.unit {
                Unit::Bytes => HumanBytes(total).to_string(),
                Unit::Count => total.to_string(),
            };
            let _ = multi.println(format!(
                "{GREEN_CHECK} {}  {DIM}{size}{RESET}",
                truncate(&event.message, LABEL_WIDTH),
            ));
            bars.remove(id);
        }
    }

    for (_, bar) in bars {
        bar.finish();
    }
}

/// The pre-existing renderer: one line per event, no cursor tricks.
async fn render_lines(mut rx: UnboundedReceiver<ProgressEvent>) {
    while let Some(event) = rx.recv().await {
        match (event.id.as_deref(), event.done, event.total) {
            // Tracked work is high-frequency; only its completion is worth a log line.
            (Some(_), Some(done), Some(total)) if done < total => {}
            (Some(_), Some(done), Some(total)) => {
                eprintln!("[{done}/{total}] {}", event.message)
            }
            (_, Some(done), Some(total)) => eprintln!("[{done}/{total}] {}", event.message),
            _ => eprintln!("… {}", event.message),
        }
    }
}

fn style_for(unit: Unit, is_total: bool) -> ProgressStyle {
    let template = match (unit, is_total) {
        (Unit::Bytes, false) => {
            "  {spinner:.cyan} {prefix:.bold} {wide_bar:.cyan/blue} \
             {bytes:>10}/{total_bytes:<10} {binary_bytes_per_sec:>11} eta {eta:>4}"
        }
        (Unit::Bytes, true) => {
            "  {prefix:.dim} {wide_bar:.green/dim} {bytes:>10}/{total_bytes:<10} eta {eta:>4}"
        }
        (Unit::Count, _) => {
            "  {spinner:.cyan} {prefix:.bold} {wide_bar:.cyan/blue} {pos:>7}/{len:<7}"
        }
    };
    ProgressStyle::with_template(template)
        .expect("templates are literals, checked by the tests below")
        .progress_chars("━━╾")
}

/// Keeps the tail of an over-long label — the filename is what distinguishes it.
fn truncate(text: &str, width: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= width {
        return text.to_string();
    }
    format!(
        "…{}",
        chars[chars.len() - (width - 1)..]
            .iter()
            .collect::<String>()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_style_template_parses() {
        // A bad template panics at render time, which on a terminal means a corrupted
        // display mid-download. Cheaper to find here.
        for unit in [Unit::Bytes, Unit::Count] {
            for is_total in [true, false] {
                let _ = style_for(unit, is_total);
            }
        }
    }

    #[test]
    fn short_labels_are_untouched() {
        assert_eq!(truncate("tokenizer.json", 42), "tokenizer.json");
    }

    #[test]
    fn long_labels_keep_their_tail() {
        let label = "qwen3-embedding-4b Qwen3-Embedding-4B-Q8_0.gguf";
        let out = truncate(label, 20);
        assert_eq!(out.chars().count(), 20);
        assert!(out.starts_with('…'));
        assert!(
            out.ends_with("Q8_0.gguf"),
            "the filename must survive: {out}"
        );
    }

    #[test]
    fn truncation_counts_characters_not_bytes() {
        // A multi-byte label must not be sliced through a codepoint.
        let out = truncate("café-café-café-café", 10);
        assert_eq!(out.chars().count(), 10);
    }

    #[tokio::test]
    async fn the_renderer_terminates_when_the_sender_is_dropped() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = tokio::spawn(render_lines(rx));
        tx.send(ProgressEvent {
            message: "hello".into(),
            ..Default::default()
        })
        .unwrap();
        drop(tx);
        handle.await.expect("renderer must not hang or panic");
    }

    /// `collect` and `extract` emit `step()` with no id. Losing their counter to a log
    /// line would be a regression, so the bar renderer gives them an implicit track.
    #[tokio::test]
    async fn unnamed_counted_events_still_get_a_bar() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = tokio::spawn(render_bars(rx));
        for done in 1..=3u64 {
            tx.send(ProgressEvent {
                message: format!("collected {done}"),
                done: Some(done),
                total: Some(3),
                ..Default::default()
            })
            .unwrap();
        }
        drop(tx);
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn the_line_renderer_survives_byte_tracks() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = tokio::spawn(render_lines(rx));
        for done in [0u64, 512, 1024] {
            tx.send(ProgressEvent {
                message: "model.onnx".into(),
                done: Some(done),
                total: Some(1024),
                id: Some("m:model.onnx".into()),
                unit: Unit::Bytes,
            })
            .unwrap();
        }
        drop(tx);
        handle.await.unwrap();
    }
}
