//! Resume correctness, against a real HTTP server.
//!
//! A model pull is the one operation where interruption is expected rather than
//! exceptional, so the interesting behaviour lives in the seams: what a server does with
//! a `Range` header, what happens when it ignores one, and what a half-written `.part`
//! means afterwards. None of that is reachable through a mocked client — it is a
//! property of the conversation, not of the code on our side of it.
//!
//! Every case runs against a hand-written HTTP/1.1 server on a loopback port, with a
//! payload of a few hundred kilobytes, so the whole file exercises the same code path a
//! 1.2 GB download takes. The server is raw rather than a framework because one of the
//! cases is a **deliberately truncated response** — a correct server library will not
//! emit one, which is precisely why the client has to cope with it.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use centinel_core::models::ModelFile;
use centinel_core::models::download::{Downloader, FileJob, Outcome, Overall, part_path};
use centinel_core::op::Progress;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Big enough to span several read chunks, small enough to be instant.
const PAYLOAD_LEN: usize = 300_000;

fn payload() -> Vec<u8> {
    (0..PAYLOAD_LEN).map(|i| (i % 251) as u8).collect()
}

/// How the test server should misbehave.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Correct: honours `Range`, serves the whole payload.
    Honest,
    /// Drops the connection after this many bytes, once. The interruption under test.
    CutAfter(usize),
    /// Answers 200 with the whole body regardless of `Range` — some CDNs do this.
    IgnoreRange,
    /// Serves bytes that do not match the pinned digest.
    Corrupt,
    /// Reports a length that disagrees with the registry, as a rewritten tag would.
    WrongLength,
}

struct TestServer {
    addr: SocketAddr,
    requests: Arc<AtomicUsize>,
    ranges: Arc<std::sync::Mutex<Vec<Option<String>>>>,
}

impl TestServer {
    async fn start(mode: Mode) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let ranges = Arc::new(std::sync::Mutex::new(Vec::new()));

        let (n_counter, seen) = (Arc::clone(&requests), Arc::clone(&ranges));
        tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                let n = n_counter.fetch_add(1, Ordering::SeqCst);
                let seen = Arc::clone(&seen);
                tokio::spawn(async move {
                    let _ = handle(socket, mode, n, seen).await;
                });
            }
        });

        Self {
            addr,
            requests,
            ranges,
        }
    }

    fn url(&self) -> String {
        format!("http://{}/model.onnx", self.addr)
    }

    fn ranges(&self) -> Vec<Option<String>> {
        self.ranges.lock().unwrap().clone()
    }
}

async fn handle(
    mut socket: tokio::net::TcpStream,
    mode: Mode,
    n: usize,
    seen: Arc<std::sync::Mutex<Vec<Option<String>>>>,
) -> std::io::Result<()> {
    // Read up to the end of the request headers. The downloader never sends a body.
    let mut request = Vec::new();
    let mut buf = [0u8; 1024];
    loop {
        let read = socket.read(&mut buf).await?;
        if read == 0 {
            return Ok(());
        }
        request.extend_from_slice(&buf[..read]);
        if request.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }

    let text = String::from_utf8_lossy(&request);
    let range = text
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("range:"))
        .map(|l| l[6..].trim().to_string());
    seen.lock().unwrap().push(range.clone());

    let full = match mode {
        Mode::Corrupt => vec![b'x'; PAYLOAD_LEN],
        _ => payload(),
    };

    // `bytes=<start>-` is the only form the downloader sends.
    let start = match (&range, mode) {
        (Some(_), Mode::IgnoreRange) | (None, _) => 0,
        (Some(r), _) => r
            .strip_prefix("bytes=")
            .and_then(|s| s.split('-').next())
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0),
    };

    let body = &full[start.min(full.len())..];
    let claimed = match mode {
        Mode::WrongLength => body.len() + 999,
        _ => body.len(),
    };

    let mut head = if start > 0 && mode != Mode::IgnoreRange {
        format!(
            "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{}/{}\r\n",
            full.len() - 1,
            full.len()
        )
    } else {
        "HTTP/1.1 200 OK\r\n".to_string()
    };
    head.push_str(&format!(
        "Content-Length: {claimed}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n"
    ));
    socket.write_all(head.as_bytes()).await?;

    // Only the *first* connection is cut short, so the retry can succeed — which is what
    // proves the second attempt resumed rather than silently restarting.
    let to_write = match mode {
        Mode::CutAfter(cut) if n == 0 => &body[..cut.min(body.len())],
        Mode::WrongLength => &body[..0],
        _ => body,
    };
    socket.write_all(to_write).await?;
    socket.flush().await?;

    // Closing with fewer bytes than Content-Length promised is exactly the truncation a
    // dropped connection produces.
    Ok(())
}

/// A [`ModelFile`] pinned to the real digest of the test payload.
fn pinned_file() -> &'static ModelFile {
    let sha = hex::encode(Sha256::digest(payload()));
    Box::leak(Box::new(ModelFile {
        path: "onnx/model.onnx",
        size: PAYLOAD_LEN as u64,
        sha256: Box::leak(sha.into_boxed_str()),
    }))
}

fn job(server: &TestServer, dir: &std::path::Path) -> FileJob {
    FileJob {
        url: server.url(),
        dest: dir.join("onnx/model.onnx"),
        file: pinned_file(),
        bar_id: "test:model.onnx".into(),
        label: "model.onnx".into(),
    }
}

#[tokio::test]
async fn a_fresh_download_verifies_and_leaves_no_part_file() {
    let server = TestServer::start(Mode::Honest).await;
    let dir = tempfile::tempdir().unwrap();
    let job = job(&server, dir.path());

    let downloader = Downloader::new("centinel-test", false).unwrap();
    let mut overall = Overall::new(PAYLOAD_LEN as u64);
    let outcome = downloader
        .fetch(&job, &Progress::none(), &mut overall)
        .await
        .unwrap();

    assert_eq!(
        outcome,
        Outcome::Downloaded {
            bytes: PAYLOAD_LEN as u64,
            resumed_from: 0
        }
    );
    assert_eq!(std::fs::read(&job.dest).unwrap(), payload());
    assert!(
        !part_path(&job.dest).exists(),
        "the .part must be renamed away"
    );
    assert_eq!(overall.done, PAYLOAD_LEN as u64);
    assert_eq!(
        server.ranges(),
        vec![None],
        "a fresh download sends no Range"
    );
}

/// The headline case: cut the connection, then run the same pull again.
#[tokio::test]
async fn an_interrupted_transfer_resumes_from_where_it_stopped() {
    const CUT: usize = 100_000;
    let server = TestServer::start(Mode::CutAfter(CUT)).await;
    let dir = tempfile::tempdir().unwrap();
    let job = job(&server, dir.path());
    let downloader = Downloader::new("centinel-test", false).unwrap();

    let mut overall = Overall::new(PAYLOAD_LEN as u64);
    let err = downloader
        .fetch(&job, &Progress::none(), &mut overall)
        .await
        .expect_err("a dropped connection must fail loudly");
    assert!(
        err.to_string().contains("Run the pull again to resume")
            || err.to_string().contains("transfer failed"),
        "the error should tell the operator what to do: {err}"
    );

    // The partial bytes survive. That retention *is* the resume point.
    let part = part_path(&job.dest);
    let kept = std::fs::metadata(&part).expect("a .part must survive a network error");
    assert!(kept.len() > 0, "no bytes were kept");
    assert!(
        !job.dest.exists(),
        "an unverified file must not be installed"
    );

    // Second attempt: same call, no special flag.
    let mut overall = Overall::new(PAYLOAD_LEN as u64);
    let outcome = downloader
        .fetch(&job, &Progress::none(), &mut overall)
        .await
        .unwrap();

    match outcome {
        Outcome::Downloaded {
            resumed_from,
            bytes,
        } => {
            assert!(
                resumed_from > 0,
                "the second attempt restarted instead of resuming"
            );
            assert_eq!(resumed_from + bytes, PAYLOAD_LEN as u64);
        }
        other => panic!("expected a resumed download, got {other:?}"),
    }

    // The reassembled file is byte-identical — the property the digest check enforces
    // and the reason a resumed download is safe to trust.
    assert_eq!(std::fs::read(&job.dest).unwrap(), payload());

    let ranges = server.ranges();
    assert_eq!(ranges.len(), 2);
    assert!(ranges[0].is_none());
    assert!(
        ranges[1].as_deref().unwrap().starts_with("bytes="),
        "the retry must ask for a range: {ranges:?}"
    );
}

/// Some CDNs answer a ranged request with the whole file. Appending to the `.part` then
/// would silently produce a file with a duplicated prefix, which is exactly the kind of
/// corruption a size check alone would not catch.
#[tokio::test]
async fn a_server_that_ignores_range_restarts_cleanly() {
    let server = TestServer::start(Mode::IgnoreRange).await;
    let dir = tempfile::tempdir().unwrap();
    let job = job(&server, dir.path());

    std::fs::create_dir_all(job.dest.parent().unwrap()).unwrap();
    std::fs::write(part_path(&job.dest), vec![7u8; 50_000]).unwrap();

    let downloader = Downloader::new("centinel-test", false).unwrap();
    let mut overall = Overall::new(PAYLOAD_LEN as u64);
    downloader
        .fetch(&job, &Progress::none(), &mut overall)
        .await
        .unwrap();

    assert_eq!(std::fs::read(&job.dest).unwrap(), payload());
}

/// Known-bad bytes are the one thing that gets deleted. Keeping them would make every
/// future pull fail the same way with nothing the operator could do about it.
#[tokio::test]
async fn a_digest_mismatch_discards_the_partial_download() {
    let server = TestServer::start(Mode::Corrupt).await;
    let dir = tempfile::tempdir().unwrap();
    let job = job(&server, dir.path());

    let downloader = Downloader::new("centinel-test", false).unwrap();
    let mut overall = Overall::new(PAYLOAD_LEN as u64);
    let err = downloader
        .fetch(&job, &Progress::none(), &mut overall)
        .await
        .unwrap_err()
        .to_string();

    assert!(err.contains("digest mismatch"), "{err}");
    assert!(!job.dest.exists(), "corrupt bytes must never be installed");
    assert!(
        !part_path(&job.dest).exists(),
        "known-bad bytes must not become a resume point"
    );
}

/// Fail before the transfer, not after a gigabyte of it.
#[tokio::test]
async fn a_length_that_disagrees_with_the_pin_fails_immediately() {
    let server = TestServer::start(Mode::WrongLength).await;
    let dir = tempfile::tempdir().unwrap();
    let job = job(&server, dir.path());

    let downloader = Downloader::new("centinel-test", false).unwrap();
    let mut overall = Overall::new(PAYLOAD_LEN as u64);
    let err = downloader
        .fetch(&job, &Progress::none(), &mut overall)
        .await
        .unwrap_err()
        .to_string();

    assert!(err.contains("the registry pins"), "{err}");
    assert!(
        err.contains("rewritten"),
        "the message should name the likely cause: {err}"
    );
    assert!(
        !part_path(&job.dest).exists(),
        "nothing should have been written"
    );
}

#[tokio::test]
async fn an_installed_file_is_not_fetched_again() {
    let server = TestServer::start(Mode::Honest).await;
    let dir = tempfile::tempdir().unwrap();
    let job = job(&server, dir.path());

    std::fs::create_dir_all(job.dest.parent().unwrap()).unwrap();
    std::fs::write(&job.dest, payload()).unwrap();

    let downloader = Downloader::new("centinel-test", false).unwrap();
    let mut overall = Overall::new(PAYLOAD_LEN as u64);
    let outcome = downloader
        .fetch(&job, &Progress::none(), &mut overall)
        .await
        .unwrap();

    assert_eq!(outcome, Outcome::Present);
    assert_eq!(
        server.requests.load(Ordering::SeqCst),
        0,
        "no request should be made"
    );
    // A skipped file still counts toward the aggregate, or the total would stall.
    assert_eq!(overall.done, PAYLOAD_LEN as u64);
}

#[tokio::test]
async fn force_refetches_and_discards_any_partial() {
    let server = TestServer::start(Mode::Honest).await;
    let dir = tempfile::tempdir().unwrap();
    let job = job(&server, dir.path());

    std::fs::create_dir_all(job.dest.parent().unwrap()).unwrap();
    std::fs::write(&job.dest, payload()).unwrap();
    std::fs::write(part_path(&job.dest), vec![9u8; 1000]).unwrap();

    let downloader = Downloader::new("centinel-test", true).unwrap();
    let mut overall = Overall::new(PAYLOAD_LEN as u64);
    let outcome = downloader
        .fetch(&job, &Progress::none(), &mut overall)
        .await
        .unwrap();

    assert_eq!(
        outcome,
        Outcome::Downloaded {
            bytes: PAYLOAD_LEN as u64,
            resumed_from: 0
        },
        "--force must ignore both the installed file and the stale .part"
    );
    assert_eq!(
        server.ranges(),
        vec![None],
        "--force must not resume from bytes it was told to distrust"
    );
}

/// A `.part` at or past the full size would draw a 416. Discard it instead of surfacing
/// a range error the operator cannot act on.
#[tokio::test]
async fn an_oversized_part_file_is_discarded_rather_than_resumed() {
    let server = TestServer::start(Mode::Honest).await;
    let dir = tempfile::tempdir().unwrap();
    let job = job(&server, dir.path());

    std::fs::create_dir_all(job.dest.parent().unwrap()).unwrap();
    std::fs::write(part_path(&job.dest), vec![3u8; PAYLOAD_LEN + 10]).unwrap();

    let downloader = Downloader::new("centinel-test", false).unwrap();
    let mut overall = Overall::new(PAYLOAD_LEN as u64);
    downloader
        .fetch(&job, &Progress::none(), &mut overall)
        .await
        .unwrap();

    assert_eq!(std::fs::read(&job.dest).unwrap(), payload());
    assert_eq!(
        server.ranges(),
        vec![None],
        "an unusable .part must not be resumed"
    );
}

/// Progress must be absolute, not incremental: events are throttled and lossy, and a
/// resumed download starts partway along. A bar driven by deltas would be wrong on both.
#[tokio::test]
async fn progress_reports_absolute_positions_and_ends_at_the_total() {
    let server = TestServer::start(Mode::Honest).await;
    let dir = tempfile::tempdir().unwrap();
    let job = job(&server, dir.path());

    let (progress, mut rx) = Progress::channel();
    let downloader = Downloader::new("centinel-test", false).unwrap();
    let mut overall = Overall::new(PAYLOAD_LEN as u64);
    downloader
        .fetch(&job, &progress, &mut overall)
        .await
        .unwrap();
    drop(progress);

    let mut file_positions = Vec::new();
    while let Some(event) = rx.recv().await {
        if event.id.as_deref() == Some("test:model.onnx") {
            file_positions.push(event.done.unwrap());
            assert_eq!(event.total, Some(PAYLOAD_LEN as u64));
            assert_eq!(event.unit, centinel_core::op::Unit::Bytes);
        }
    }

    assert!(
        !file_positions.is_empty(),
        "a download must report progress"
    );
    assert!(
        file_positions.windows(2).all(|w| w[0] <= w[1]),
        "positions must be monotonic: {file_positions:?}"
    );
    assert_eq!(
        file_positions.last().copied(),
        Some(PAYLOAD_LEN as u64),
        "the final event must complete the bar, or it never clears"
    );
}
