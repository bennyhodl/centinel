//! A run typed at a terminal, watched and stopped from another process on the same store.
//!
//! The server's page reads jobs off its own board, and a `centinel run` in a terminal is a
//! different process with a board of its own. What joins them is the store, so this runs
//! the real binary against a real socket and plays the server from the test's process:
//! nothing substituted could show that the two processes actually meet there.

use std::net::SocketAddr;
use std::process::Stdio;
use std::time::Duration;

use centinel_core::jobs::{JobState, Outcome as JobOutcome};
use centinel_core::journal::{Journal, Outcome, RunLock, Trigger};
use centinel_core::op::Ctx;
use centinel_core::store::Store;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

/// Pages enough that a run is still collecting when it is stopped.
const PAGES: usize = 40;

/// A site with a sitemap of [`PAGES`] pages, each answered slowly.
async fn slow_site() -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut request = vec![0u8; 4096];
                let n = socket.read(&mut request).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&request[..n]);
                let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
                let (status, kind, body) = match path.as_str() {
                    "/sitemap.xml" => (
                        "200 OK",
                        "application/xml",
                        format!(
                            r#"<?xml version="1.0"?><urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">{}</urlset>"#,
                            (0..PAGES)
                                .map(|i| format!("<url><loc>http://{addr}/p/{i}</loc></url>"))
                                .collect::<String>()
                        ),
                    ),
                    p if p.starts_with("/p/") => {
                        tokio::time::sleep(Duration::from_millis(300)).await;
                        (
                            "200 OK",
                            "text/html; charset=utf-8",
                            format!(
                                "<html><body><h1>{p}</h1><p>The council meets on Tuesdays.</p></body></html>"
                            ),
                        )
                    }
                    _ => ("404 Not Found", "text/plain", String::new()),
                };
                let reply = format!(
                    "HTTP/1.1 {status}\r\ncontent-type: {kind}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    addr
}

/// The binary, on this store and nothing else: no config of the person running the tests.
fn centinel(root: &std::path::Path, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_centinel"));
    command
        .arg("--root")
        .arg(root)
        .args(args)
        .current_dir(root)
        .env("HOME", root)
        .env("XDG_CONFIG_HOME", root)
        .env_remove("CENTINEL_ROOT")
        .env_remove("RUST_LOG")
        .stdin(Stdio::null());
    command
}

async fn eventually(ctx: &Ctx, what: &str, ready: impl Fn(&JobState) -> bool) -> JobState {
    for _ in 0..200 {
        if let Some(job) = ctx.jobs.snapshot().into_iter().find(|j| ready(j)) {
            return job;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("{what}: {:#?}", ctx.jobs.snapshot());
}

/// The bug this series fixes, end to end: a run typed at a terminal holds the lane, is on
/// the board of another process on the store — the server's — while it collects, stops
/// when that process asks, and is journaled `interrupted` under the id the board showed.
#[tokio::test(flavor = "multi_thread")]
async fn a_run_typed_at_a_terminal_is_watched_and_stopped_from_another_process() {
    let site = slow_site().await;
    let root = tempfile::tempdir().unwrap();
    let added = centinel(
        root.path(),
        &[
            "source",
            "add",
            "slow",
            "--site",
            &format!("http://{site}/"),
        ],
    )
    .output()
    .await
    .unwrap();
    assert!(added.status.success(), "{added:?}");

    let terminal = centinel(root.path(), &["run", "--source", "slow"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let terminal_pid = terminal.id().unwrap();

    // This process is the server: its own context on the same store.
    let server = Ctx::new(Store::open(root.path()).await.unwrap());
    let job = eventually(&server, "the terminal's run on the server's board", |j| {
        j.kind == "run" && j.outcome.is_none() && j.ok > 0
    })
    .await;
    assert_eq!(job.label, "slow");
    assert!(job.stoppable, "the page offers Stop for it");
    assert!(
        job.step.as_deref().is_some_and(|s| s.contains("collect")),
        "{job:?}"
    );
    assert_eq!(
        RunLock::current(&server.store).map(|h| (h.pid, h.trigger)),
        Some((terminal_pid, Trigger::Manual)),
        "the run holds the lane"
    );

    assert!(
        server.jobs.stop(&job.id),
        "Stop reaches another process's run"
    );
    let out = tokio::time::timeout(Duration::from_secs(30), terminal.wait_with_output())
        .await
        .expect("the terminal's run stops when asked")
        .unwrap();
    assert_stopped(&out);

    let attempts = Journal::new(&server.store).read().await.unwrap();
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].outcome, Outcome::Interrupted);
    assert_eq!(
        attempts[0].run_id, job.id,
        "one id: the board's and the journal's"
    );
    assert!(
        RunLock::current(&server.store).is_none(),
        "the lane is released"
    );

    let ended = eventually(&server, "the run's ending on the server's board", |j| {
        j.id == job.id && j.outcome.is_some()
    })
    .await;
    assert_eq!(ended.outcome, Some(JobOutcome::Cancelled));
    assert!(
        ended.ok < PAGES as u64,
        "stopped before it collected everything"
    );
}

/// Ctrl-C at the terminal ends a run the way Stop does: at an item boundary, journaled
/// `interrupted`, the lane released — where it used to kill the process and leave no
/// record at all.
#[tokio::test(flavor = "multi_thread")]
async fn ctrl_c_at_the_terminal_stops_a_run_and_records_it() {
    let site = slow_site().await;
    let root = tempfile::tempdir().unwrap();
    let added = centinel(
        root.path(),
        &[
            "source",
            "add",
            "slow",
            "--site",
            &format!("http://{site}/"),
        ],
    )
    .output()
    .await
    .unwrap();
    assert!(added.status.success(), "{added:?}");

    let terminal = centinel(root.path(), &["run", "--source", "slow"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = terminal.id().unwrap();
    let store = Store::open(root.path()).await.unwrap();
    for _ in 0..200 {
        if RunLock::current(&store).is_some_and(|h| h.pid == pid) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(RunLock::current(&store).is_some(), "the run took the lane");

    let sent = std::process::Command::new("kill")
        .args(["-INT", &pid.to_string()])
        .status()
        .unwrap();
    assert!(sent.success());
    let out = tokio::time::timeout(Duration::from_secs(30), terminal.wait_with_output())
        .await
        .expect("ctrl-C stops the run")
        .unwrap();
    assert_stopped(&out);

    let attempts = Journal::new(&store).read().await.unwrap();
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].outcome, Outcome::Interrupted);
    assert_eq!(attempts[0].trigger, Trigger::Manual);
    assert!(RunLock::current(&store).is_none(), "the lane is released");
}

/// A run stopped as asked: the shell's code for ctrl-C, and a line that says so rather
/// than an `Error:`.
fn assert_stopped(out: &std::process::Output) {
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(130), "{stderr}");
    assert!(stderr.contains("journaled `interrupted`"), "{stderr}");
    assert!(!stderr.contains("Error:"), "{stderr}");
}
