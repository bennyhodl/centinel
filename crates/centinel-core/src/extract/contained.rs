//! Running a reader in a child of this program, so a deadline can stop it.
//!
//! A reader on a thread of this process cannot be stopped. A deadline there only stops the
//! waiting: the thread keeps its core, keeps its heap, and the runtime waits for it at
//! exit. Plant City's 2022 ACFR kept `pdf-inspector` busy for 8 min 52 s at 669 MB, and
//! every extract pass met it again (docs/FIELD-NOTES.md, Fault 10). In a child the same
//! reader is a [`Tool`]: it has a deadline, and past it the child is killed, which gives the
//! core and the memory back to the run.
//!
//! **The child is this program**, started with [`FLAG`], the reader's name and the blob's
//! path. The binary's `main` hands those to [`serve`] before it parses a command line or
//! opens a store, so the child loads no store and no index, only the reader. A second
//! program would have to be installed, found and kept at the same version as this one.
//! Re-running this one keeps reader and caller one build.
//!
//! **What comes back** is the reader's [`Extracted`], as JSON on stdout. That is the value
//! the reader returns in process, so [`super::derive`] treats the two the same: it
//! normalizes the text, checks it, and moves on to the next reader if there is no text.
//!
//! **A child that ran and gave no answer has produced nothing**: stopped at its deadline,
//! killed by the OOM killer, a panic in the parser, a reply that does not parse. That is a
//! verdict about the document. The reason says which, and the next reader takes it.
//!
//! **A child that could not be started is not a verdict.** Out of processes, out of file
//! descriptors, a fork refused under memory pressure: these are facts about this machine
//! for a minute, and the same minute would write them against every document left in the
//! pass. Recorded, a `.docx` would carry an Underivable it never earned and a PDF a flat
//! `pdftotext` Derivation in place of its markdown, both for good. So [`read`] returns the
//! error, the pass stops with it, nothing is written, and the next pass asks again. This is
//! the line `tool` draws between a refusal and a transport fault.
//!
//! **The cost** is one process start per document read this way, about 10 ms against reads
//! of 50 ms to minutes. The child reads the blob from its path in the pool; the bytes are
//! not copied through a pipe.
//!
//! **Under `cargo test`** this program is the test harness, which does not know [`FLAG`]: it
//! refuses it and exits 101, and the reader counts as having produced nothing. So a unit
//! test that sends a PDF through `derive` sees the reader fail and the fallback run. Two
//! tests run a real child: `crates/centinel/tests/contained_read.rs` runs the binary and
//! checks `pdf-inspector` read a PDF in it, and `crates/centinel-core/tests/deadline.rs` is
//! a test program whose `main` serves readers as the binary's does, so a reader that never
//! answers can be stopped at a deadline short enough for a test.
//!
//! **No memory limit is set on the child.** `RLIMIT_AS` is not enforced on macOS, and on
//! Linux it counts address space reserved, not memory used: jemalloc's arenas and the
//! stacks of lopdf's rayon threads, before a document is opened. Any number would bound
//! the wrong quantity. What the child does do on Linux is offer itself to the OOM killer
//! first ([`offer_to_the_oom_killer`]), so a document too heavy for the machine costs its
//! own read and not the run. A real bound belongs to the machine-wide memory budget
//! proposed under Fault 9, which can count what a child holds.

use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{Blob, Extracted, Reader};
use crate::tool::{Tool, ToolError};

/// The argument that makes this program a reader rather than a command line.
///
/// Spelled as a flag so a program that does not know it refuses it. The libtest harness is
/// the one that matters: it exits on an unknown option, where an unknown word would be
/// taken as a filter and run the tests that match it.
const FLAG: &str = "--contained-read";

/// Runs `reader` on `blob` in a child of this program, for at most `deadline`.
///
/// Only the bytes reach the child, by their path. None of the contained readers reads the
/// address or the title; one that did would need them passed here.
///
/// `Err` only when the child could not be started; see the module's note on why that is
/// not a verdict.
pub(super) async fn read(
    reader: Reader,
    blob: &Blob<'_>,
    deadline: Duration,
) -> Result<Extracted, ToolError> {
    let answer = Tool::new(this_program())
        .args([
            OsStr::new(FLAG),
            OsStr::new(reader.name()),
            blob.path.as_os_str(),
        ])
        .timeout(deadline)
        .success()
        .await;

    let e = match answer {
        Ok(stdout) => {
            return Ok(serde_json::from_slice(&stdout).unwrap_or_else(|e| {
                Extracted::Unextractable {
                    reason: format!("its process answered something unreadable: {e}"),
                }
            }));
        }
        Err(e @ (ToolError::NotFound { .. } | ToolError::Io { .. })) => return Err(e),
        Err(e) => e,
    };

    // Said once here, with the document, because the reason that reaches the record is
    // about the reader and no longer names the file.
    tracing::warn!(
        reader = reader.name(),
        path = %blob.path.display(),
        error = %e,
        "a reader in a child gave no answer; the next reader takes the document"
    );
    Ok(Extracted::Unextractable {
        reason: no_answer(&e),
    })
}

/// Why a child that ran gave no answer, in the words a verdict keeps.
fn no_answer(e: &ToolError) -> String {
    match e {
        ToolError::TimedOut { after, .. } => {
            format!("stopped after {after:?}, past its deadline")
        }
        // A panic's message, or nothing at all from a process the OOM killer took. The
        // last line that is not a `note:`, because a panic opens with the thread's name and
        // closes with a hint about backtraces, and the reason sits between them.
        ToolError::Failed { status, stderr, .. } => match stderr
            .lines()
            .rfind(|l| !l.trim().is_empty() && !l.starts_with("note:"))
        {
            Some(said) => format!("its process ended without an answer, {status}: {said}"),
            None => format!("its process ended without an answer, {status}"),
        },
        // [`read`] returns the others before asking.
        other => other.to_string(),
    }
}

/// This program, to start again as a reader.
///
/// On Linux, `/proc/self/exe` rather than the path it names. `centinel update` replaces the
/// binary while `serve` keeps running, and the path then names a file that is gone, or a
/// newer build. `/proc/self/exe` is still the running one.
fn this_program() -> PathBuf {
    if cfg!(target_os = "linux") {
        return PathBuf::from("/proc/self/exe");
    }
    // Falls back to the name, so a lookup that fails is reported by the spawn that uses it,
    // as a child that could not be started, rather than as a second kind of failure here.
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("centinel"))
}

/// The child's side. `None` unless this process was started as a reader by [`read`];
/// otherwise the reader has run, its answer is on stdout, and the value is the exit code.
///
/// **A program that calls [`super::derive`] calls this first in `main`** and exits with the
/// code it returns; without it every contained reader's child refuses the flag and the
/// document falls to the next reader.
pub fn serve() -> Option<i32> {
    let mut args = std::env::args_os().skip(1);
    if args.next()? != FLAG {
        return None;
    }
    offer_to_the_oom_killer();

    let (Some(name), Some(path)) = (args.next(), args.next()) else {
        return Some(refuse("expected a reader's name and a path"));
    };
    let Some(reader) = name.to_str().and_then(Reader::named) else {
        return Some(refuse(&format!("no reader is named {name:?}")));
    };
    let path = Path::new(&path);
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) => return Some(refuse(&format!("reading {}: {e}", path.display()))),
    };

    let outcome = reader.read_in_process(&Blob {
        bytes: &bytes,
        path,
        url: None,
        title: None,
    });
    // Flushed here: the caller exits at once, and the answer is the whole point of the
    // process.
    let mut out = std::io::stdout().lock();
    let written = serde_json::to_writer(&mut out, &outcome)
        .map_err(std::io::Error::from)
        .and_then(|()| out.flush());
    match written {
        Ok(()) => Some(0),
        Err(e) => Some(refuse(&format!("writing the answer: {e}"))),
    }
}

/// Says why on stderr, where [`no_answer`] finds it, and gives the exit code.
fn refuse(why: &str) -> i32 {
    eprintln!("{why}");
    2
}

/// Asks Linux's OOM killer to take this process before any other.
///
/// The child holds the document's whole parse, which is the memory that grows. Without
/// this the killer picks by size and can take the run instead, as it did in Fault 9, where
/// it took the tmux server with it. Raising one's own score needs no privilege. Elsewhere
/// there is no such knob, and on failure nothing changes but the order.
///
/// The score makes a reader the first choice **even when something else ate the memory**,
/// and that kill is then recorded against the document like any reader that ended without
/// an answer. Accepted: the other way round, the run dies, nothing is recorded, and the
/// next pass meets the same document and dies again. `serve`'s unit says
/// `OOMPolicy=continue` for the same reason, so systemd does not stop the server because a
/// child of it was killed.
fn offer_to_the_oom_killer() {
    #[cfg(target_os = "linux")]
    let _ = std::fs::write("/proc/self/oom_score_adj", "1000");
}
