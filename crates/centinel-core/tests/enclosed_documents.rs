//! A page that carries a document, against a real HTTP server.
//!
//! The unit tests in `enclosure` prove the addresses are *found*. What they cannot show is
//! the part that matters to a corpus: that acquiring one address produces two artifacts,
//! each with its own bytes and its own resource, and that a document which refuses does
//! not take the page down with it. That is a property of the conversation, so it is tested
//! against a socket — the same choice, for the same reason, as `download_resume`.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use centinel_core::discovery::DiscoveryLimits;
use centinel_core::domain::{Resource, Source, SourceId};
use centinel_core::op::Progress;
use centinel_core::policy::HostPolicy;
use centinel_core::sources::site::SiteSource;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const PDF_BYTES: &[u8] = b"%PDF-1.4\nthe proclamation itself\n%%EOF";

/// How the server answers for the document.
#[derive(Clone, Copy)]
enum Document {
    Served,
    Missing,
}

struct TestServer {
    addr: SocketAddr,
    requests: Arc<AtomicUsize>,
}

impl TestServer {
    async fn start(document: Document) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let requests = Arc::new(AtomicUsize::new(0));

        let counter = Arc::clone(&requests);
        tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                counter.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(async move {
                    let _ = handle(socket, document, addr).await;
                });
            }
        });

        Self { addr, requests }
    }

    fn page_url(&self) -> String {
        format!("http://{}/proclamation/irish", self.addr)
    }

    fn pdf_url(&self) -> String {
        format!("http://{}/files/2022/irish.pdf", self.addr)
    }
}

/// The page is a PDFObject wrapper: no `<embed>` anywhere, the address only in a script.
/// This is the served shape of a real `tampa.gov` proclamation.
fn page(addr: SocketAddr) -> String {
    format!(
        r#"<html><head><title>Irish American Heritage Month | City</title></head><body>
        <h1>Irish American Heritage Month</h1>
        <div class="field__label">Date Added</div><time>Tuesday, March 1, 2022</time>
        <div class='pdf-reader'><div id="pdf_reader"></div>
        <script>
        var options = {{ height: "980px", PDFJS_URL: "/libraries/pdfjs-full/web/viewer.html" }};
        var pdfURL = "http://{addr}/files/2022/irish.pdf#view=Fit&amp;toolbar=1"
        </script></div>
        <p>Was this page helpful?</p></body></html>"#
    )
}

async fn handle(
    mut socket: tokio::net::TcpStream,
    document: Document,
    addr: SocketAddr,
) -> std::io::Result<()> {
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
    let path = text.split_whitespace().nth(1).unwrap_or("/").to_string();

    let (status, content_type, body): (&str, &str, Vec<u8>) = match path.as_str() {
        p if p.ends_with(".pdf") => match document {
            Document::Served => ("200 OK", "application/pdf", PDF_BYTES.to_vec()),
            Document::Missing => ("404 Not Found", "text/plain", b"gone".to_vec()),
        },
        _ => (
            "200 OK",
            "text/html; charset=utf-8",
            page(addr).into_bytes(),
        ),
    };

    socket
        .write_all(
            format!(
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .as_bytes(),
        )
        .await?;
    socket.write_all(&body).await?;
    socket.flush().await
}

fn source() -> SiteSource {
    let policy = HostPolicy {
        // The pacing is exercised, not measured; one request per second would only make
        // the suite slower.
        max_requests_per_second: 500.0,
        ..HostPolicy::default()
    };
    SiteSource::new(
        SourceId::new("tampa").unwrap(),
        "http://127.0.0.1",
        policy,
        DiscoveryLimits::default(),
    )
    .unwrap()
}

/// The whole point: one address in, the page **and** its document out.
#[tokio::test]
async fn a_page_yields_its_document_as_a_second_artifact() {
    let server = TestServer::start(Document::Served).await;
    let site = source();
    let resource = Resource::new(site.id().clone(), server.page_url());

    let got = site
        .acquire(&resource, &Progress::none())
        .await
        .expect("the page was served");

    assert_eq!(got.len(), 2, "a page and the document it encloses");

    assert_eq!(got[0].resource, resource, "the page keeps its own address");
    assert!(
        String::from_utf8_lossy(&got[0].fetched.bytes).contains("Irish American Heritage Month")
    );

    assert_eq!(
        got[1].resource.natural_key,
        server.pdf_url(),
        "the document is at its own address, with the viewer's fragment dropped"
    );
    assert_eq!(
        got[1].fetched.bytes, PDF_BYTES,
        "and carries the document's own bytes, not the page's"
    );
    assert_eq!(server.requests.load(Ordering::SeqCst), 2);
}

/// A page is acquired when the page is stored. A document that 404s is evidence about the
/// document — recording it as a failure of the page would make every run re-fetch a page
/// that was served perfectly well.
#[tokio::test]
async fn a_document_that_refuses_does_not_take_the_page_with_it() {
    let server = TestServer::start(Document::Missing).await;
    let site = source();
    let resource = Resource::new(site.id().clone(), server.page_url());

    let got = site
        .acquire(&resource, &Progress::none())
        .await
        .expect("the page was served, whatever the document did");

    assert_eq!(got.len(), 1, "the page alone");
    assert_eq!(got[0].resource, resource);

    let remarks = site.remarks(&Default::default(), 1);
    assert!(
        remarks.iter().any(|n| n.detail.contains("404")),
        "the run says which document refused: {remarks:?}"
    );
}

/// The marker is the page, so resumption never waits on a document that may not exist —
/// the same rule that stops a video's missing captions from re-fetching a catalogue.
#[tokio::test]
async fn the_marker_stays_the_page() {
    let site = source();
    let resource = Resource::new(site.id().clone(), "http://127.0.0.1/a");
    assert_eq!(site.marker(&resource), resource);
}

/// A document is not markup, and must never be scanned as though it were.
#[tokio::test]
async fn a_document_does_not_enclose_further_documents() {
    let server = TestServer::start(Document::Served).await;
    let site = source();
    let resource = Resource::new(site.id().clone(), server.pdf_url());

    let got = site.acquire(&resource, &Progress::none()).await.unwrap();

    assert_eq!(got.len(), 1, "one level only");
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);
}
