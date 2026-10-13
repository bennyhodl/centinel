//! What `investigate` and `check` report about the requests they made, against a real HTTP
//! server.
//!
//! The log is only worth anything if it matches what crossed the wire — a redirect's
//! target, a refusal's status, the bytes of a body — and none of that exists in a
//! substituted fetcher, so this speaks to a socket, the same choice and for the same reason
//! as `enclosed_documents`.

use std::net::SocketAddr;

use centinel_core::op::{Cancel, Ctx, Progress};
use centinel_core::ops::{self, CheckReport, InvestigateReport};
use centinel_core::requests::{RequestRecord, Step};
use centinel_core::store::Store;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const PDF_BYTES: &[u8] = b"%PDF-1.4\nthe proclamation itself\n%%EOF";

/// A site with a front page that moved, a robots.txt that declares a sitemap, the sitemap,
/// and a page that carries a PDF.
struct Site {
    addr: SocketAddr,
}

struct Reply {
    status: &'static str,
    content_type: &'static str,
    location: Option<String>,
    body: Vec<u8>,
}

impl Reply {
    fn ok(content_type: &'static str, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: "200 OK",
            content_type,
            location: None,
            body: body.into(),
        }
    }
}

impl Site {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let _ = serve(socket, addr).await;
                });
            }
        });
        Self { addr }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }
}

fn route(path: &str, addr: SocketAddr) -> Reply {
    match path {
        "/robots.txt" => Reply::ok(
            "text/plain",
            format!("User-agent: *\nSitemap: http://{addr}/sitemap.xml\n"),
        ),
        "/sitemap.xml" => Reply::ok(
            "application/xml",
            format!(
                r#"<?xml version="1.0"?><urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
                <url><loc>http://{addr}/home</loc></url><url><loc>http://{addr}/about</loc></url></urlset>"#
            ),
        ),
        "/old" => Reply {
            status: "301 Moved Permanently",
            content_type: "text/plain",
            location: Some("/home".into()),
            body: Vec::new(),
        },
        "/home" => Reply::ok(
            "text/html; charset=utf-8",
            "<html><body><h1>Home</h1><p>The council meets on Tuesdays.</p></body></html>",
        ),
        "/proclamation" => Reply::ok(
            "text/html; charset=utf-8",
            format!(
                r#"<html><body><h1>Proclamation</h1><script>
                var pdfURL = "http://{addr}/files/irish.pdf"</script></body></html>"#
            ),
        ),
        p if p.ends_with(".pdf") => Reply::ok("application/pdf", PDF_BYTES),
        _ => Reply {
            status: "404 Not Found",
            content_type: "text/plain",
            location: None,
            body: b"gone".to_vec(),
        },
    }
}

async fn serve(mut socket: tokio::net::TcpStream, addr: SocketAddr) -> std::io::Result<()> {
    let mut request = Vec::new();
    let mut buf = [0u8; 1024];
    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
        let read = socket.read(&mut buf).await?;
        if read == 0 {
            return Ok(());
        }
        request.extend_from_slice(&buf[..read]);
    }
    let text = String::from_utf8_lossy(&request);
    let path = text.split_whitespace().nth(1).unwrap_or("/");
    let reply = route(path, addr);

    let location = reply
        .location
        .map(|l| format!("Location: {l}\r\n"))
        .unwrap_or_default();
    socket
        .write_all(
            format!(
                "HTTP/1.1 {}\r\nContent-Type: {}\r\n{location}Content-Length: {}\r\nConnection: close\r\n\r\n",
                reply.status,
                reply.content_type,
                reply.body.len()
            )
            .as_bytes(),
        )
        .await?;
    socket.write_all(&reply.body).await?;
    socket.flush().await
}

async fn ctx(dir: &tempfile::TempDir) -> Ctx {
    Ctx::new(Store::open(dir.path().join("store")).await.unwrap())
}

async fn investigate(ctx: &Ctx, url: &str) -> InvestigateReport {
    ops::investigate(
        ctx,
        serde_json::from_value(serde_json::json!({ "target": url })).unwrap(),
        &Progress::none(),
        &Cancel::none(),
    )
    .await
    .unwrap()
}

fn steps(records: &[RequestRecord]) -> Vec<String> {
    records.iter().map(|r| r.step.to_string()).collect()
}

/// The front page redirects, robots.txt is read, and the sitemap it declares is walked.
/// Each of those is a line in the log with what the server said — not what a warning made
/// of it afterwards.
#[tokio::test]
async fn investigate_logs_each_request_with_what_the_server_returned() {
    let site = Site::start().await;
    let dir = tempfile::tempdir().unwrap();
    let report = investigate(&ctx(&dir).await, &site.url("/old")).await;

    let log = &report.requests;
    assert_eq!(
        steps(log),
        ["robots.txt", "seed", "probe (sitemap)"],
        "in the order they were sent: {log:#?}"
    );

    let robots = &log[0];
    assert_eq!(robots.status, Some(200));
    assert_eq!(robots.content_type.as_deref(), Some("text/plain"));
    assert_eq!(robots.url, site.url("/robots.txt"));

    let seed = &log[1];
    assert_eq!(seed.method, "GET");
    assert_eq!(seed.url, site.url("/old"));
    assert_eq!(
        seed.redirected_to.as_deref(),
        Some(site.url("/home").as_str()),
        "the address asked for is not the address that answered"
    );
    assert_eq!(seed.status, Some(200), "the status is the final hop's");
    assert!(seed.bytes > 0 && seed.ok());

    let sitemap = &log[2];
    assert_eq!(
        sitemap.step,
        Step::Probe {
            strategy: "sitemap".into()
        }
    );
    assert_eq!(sitemap.url, site.url("/sitemap.xml"));
    assert_eq!(sitemap.content_type.as_deref(), Some("application/xml"));
    assert!(sitemap.at_ms >= seed.at_ms, "offsets must be orderable");

    // And the report's own answer agrees with the log's: two addresses walked.
    assert_eq!(report.probe.as_ref().map(|p| p.addresses), Some(2));
}

/// A refusal is a record too, and says so: the status the server gave, and no type or size
/// because the fetcher reads no body from a refusal.
#[tokio::test]
async fn a_refused_address_is_logged_with_its_status_and_no_body() {
    let site = Site::start().await;
    let dir = tempfile::tempdir().unwrap();
    let report = investigate(&ctx(&dir).await, &site.url("/missing")).await;

    let seed = report
        .requests
        .iter()
        .find(|r| r.step == Step::Seed)
        .expect("the seed was asked for");
    assert_eq!(seed.status, Some(404));
    assert!(!seed.ok());
    assert_eq!(seed.content_type, None);
    assert_eq!(seed.bytes, 0);
    assert!(
        seed.error.as_deref().is_some_and(|e| e.contains("404")),
        "{seed:?}"
    );
}

/// `check` is a different op with its own sources, and its requests land in the same kind of
/// log: the page, then the document the page carries.
#[tokio::test]
async fn check_logs_the_page_and_the_document_it_followed() {
    let site = Site::start().await;
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("files");
    let report: CheckReport = ops::check(
        &ctx(&dir).await,
        serde_json::from_value(serde_json::json!({
            "target": site.url("/proclamation"),
            "out": out,
        }))
        .unwrap(),
        &Progress::none(),
        &Cancel::none(),
    )
    .await
    .unwrap();

    assert_eq!(steps(&report.requests), ["page", "enclosure"]);
    let document = &report.requests[1];
    assert_eq!(document.url, site.url("/files/irish.pdf"));
    assert_eq!(document.content_type.as_deref(), Some("application/pdf"));
    assert_eq!(document.bytes, PDF_BYTES.len() as u64);
}
