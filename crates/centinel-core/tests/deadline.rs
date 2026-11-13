//! A reader that never answers, stopped at its deadline in a real child.
//!
//! `harness = false`, because the child is this program. `derive` runs `pdf-inspector` in a
//! child of whichever program called it, started with a flag that program's `main` hands to
//! the reader before anything else. The `centinel` binary does that. The libtest harness
//! refuses the flag and exits, which a reader that never answers cannot be tested against.
//! So this `main` does what the binary's does, then runs the test.

use std::time::Duration;

use centinel_core::content::ContentKind;
use centinel_core::extract::{self, Extracted, PDFTOTEXT};

fn main() {
    if let Some(code) = extract::serve_contained_read() {
        std::process::exit(code);
    }
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(a_reader_past_its_deadline_is_stopped_and_the_next_reader_is_asked());
    println!("test a_reader_past_its_deadline_is_stopped_and_the_next_reader_is_asked ... ok");
}

/// Plant City's 2022 ACFR held `pdf-inspector` for nine minutes, and every extract pass met
/// it again (docs/FIELD-NOTES.md, Fault 10).
///
/// This reader never answers at all. The blob's path is a FIFO nobody writes, so the child
/// blocks reading it, as that one blocked in the document's fonts. The deadline has to stop
/// it, the next reader has to be asked, and the verdict has to say why. That verdict is the
/// `Unextractable` that `extract` files as an Underivable, and a later pass skips on.
///
/// Asserts that `pdftotext` was asked, not what it found: it blocks on the same FIFO, or is
/// not installed, and which is a fact about the machine.
async fn a_reader_past_its_deadline_is_stopped_and_the_next_reader_is_asked() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("acfr.pdf");
    let made = std::process::Command::new("mkfifo")
        .arg(&path)
        .status()
        .unwrap();
    assert!(made.success(), "mkfifo: {made}");

    let deadline = Duration::from_millis(500);
    // The bytes say PDF; both readers read the path. The outer bound is for the test, so a
    // broken deadline fails here rather than hanging: the readers take two deadlines, one
    // each, and the rest is a margin for starting their children.
    let derived = tokio::time::timeout(
        Duration::from_secs(30),
        extract::derive(ContentKind::Pdf, b"%PDF-1.7\n", &path, None, None, deadline),
    )
    .await
    .expect("nothing stopped the reader")
    .expect("the reader's child started");
    let Extracted::Unextractable { reason } = derived.outcome else {
        panic!("a FIFO nobody writes holds no text: {:?}", derived.outcome);
    };
    assert!(
        reason.contains("pdf-inspector: stopped after 500ms, past its deadline"),
        "the verdict does not say the reader was stopped: {reason}"
    );
    assert!(
        reason.contains(PDFTOTEXT),
        "the next reader was never asked: {reason}"
    );
}
