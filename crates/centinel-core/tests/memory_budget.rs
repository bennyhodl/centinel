//! Extract and fetch under a memory budget smaller than any one unit of work.
//!
//! Its own test program because the budget is the process's, set once: in the crate's unit
//! tests every other test would share the tiny one. One test, for the same reason: the
//! phases below hold the whole budget in turn, and a second test in this process would wait
//! on them.

use std::future::Future;
use std::time::Duration;

use centinel_core::domain::{Resource, SourceId};
use centinel_core::fetch::Fetcher;
use centinel_core::memory::{self, Budget, Origin};
use centinel_core::op::{Cancel, Ctx, Progress};
use centinel_core::ops::{ExtractArgs, extract};
use centinel_core::policy::HostPolicy;
use centinel_core::store::Store;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// How long a unit of work must stay waiting to count as held back. Generous, because the
/// work it holds back finishes in milliseconds when nothing holds it.
const HELD: Duration = Duration::from_millis(500);

/// Whether `work` is still waiting after [`HELD`]. Polls it in place, so it can be awaited
/// again afterwards.
async fn still_waiting<F: Future + Unpin>(work: &mut F) -> bool {
    tokio::time::timeout(HELD, work).await.is_err()
}

/// The field VM's shape at its worst: every unit of work costs more than the machine has.
///
/// One MiB is less than the Lance caches' floors, so the room left for work is the least
/// there is, and every read and every fetch asks for more than all of it. Each is cut to
/// the whole room and has to wait for the one before it.
///
/// - While something else holds the room, extract and a fetch both wait: the reservations
///   are there, and in front of the work.
/// - Given back, both finish, every document read and none dropped: a constrained machine
///   is slower, not stuck.
#[tokio::test]
async fn a_full_budget_holds_back_reads_and_fetches_until_room_is_given_back() {
    let budget = memory::install(Budget::new(1 << 20, Origin::Flag)).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("store")).await.unwrap();
    let source = SourceId::new("tampa").unwrap();
    let documents = 12;
    for n in 0..documents {
        let page = format!(
            "<html><body><h1>Agenda {n}</h1><p>Item {n}: the council hears the budget.</p>\
             </body></html>"
        );
        store
            .record_observation(
                &Resource::new(source.clone(), format!("https://tampa.gov/agenda/{n}")),
                page.as_bytes(),
                jiff::Timestamp::now(),
                Default::default(),
            )
            .await
            .unwrap();
    }
    let ctx = Ctx::new(store);

    // Extract.
    let holding = budget.reserve(u64::MAX).await;
    let mut pass = Box::pin(extract(
        &ctx,
        ExtractArgs::default(),
        &Progress::none(),
        &Cancel::none(),
    ));
    assert!(
        still_waiting(&mut pass).await,
        "extract read a document while the budget had no room"
    );
    drop(holding);
    let report = tokio::time::timeout(Duration::from_secs(60), pass)
        .await
        .expect("extract waited for room that never came back")
        .unwrap();
    assert_eq!(report.attempted, documents);
    assert_eq!(report.extracted, documents, "{report:#?}");

    // Fetch.
    let url = serve_one_page().await;
    let fetcher = Fetcher::new(&HostPolicy::default()).unwrap();
    let holding = budget.reserve(u64::MAX).await;
    let mut get = Box::pin(fetcher.get(&url));
    assert!(
        still_waiting(&mut get).await,
        "a fetch went out while the budget had no room"
    );
    drop(holding);
    let fetched = tokio::time::timeout(Duration::from_secs(30), get)
        .await
        .expect("the fetch waited for room that never came back")
        .unwrap();
    assert_eq!(fetched.bytes, b"ok");
}

/// A server that answers every request with a two-byte body.
async fn serve_one_page() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let _ = socket.read(&mut buf).await;
                let _ = socket
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                    .await;
            });
        }
    });
    format!("http://{addr}/")
}
