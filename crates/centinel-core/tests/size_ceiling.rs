//! The size ceiling, against a real HTTP server.
//!
//! The ceiling exists for memory: a body over it must be refused before it is all held.
//! Whether the fetcher read a body is a property of the conversation, not of the bytes it
//! returns, so it is tested against a socket, as `enclosed_documents` is.

use std::time::{Duration, Instant};

use centinel_core::acquire::Frontier;
use centinel_core::discovery::DiscoveryLimits;
use centinel_core::domain::{Refusal, Resource, Source, SourceId};
use centinel_core::op::Progress;
use centinel_core::policy::HostPolicy;
use centinel_core::sources::site::SiteSource;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const CEILING: u64 = 1024;
const SMALL: &[u8] = b"%PDF-1.4\nunder the ceiling\n%%EOF";

async fn serve() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            tokio::spawn(handle(socket));
        }
    });
    format!("http://{addr}")
}

async fn handle(mut socket: tokio::net::TcpStream) -> std::io::Result<()> {
    let mut buf = [0u8; 4096];
    let n = socket.read(&mut buf).await?;
    let request = String::from_utf8_lossy(&buf[..n]);
    let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
    match path.as_str() {
        // A length far over the ceiling, a little of the body, and then nothing: a client
        // that reads the body waits here until its timeout.
        "/declared" => {
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\n\
                      Content-Length: 1000000\r\n\r\n",
                )
                .await?;
            socket.write_all(&[b'x'; 2048]).await?;
            socket.flush().await?;
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
        // No length: the body passes the ceiling partway through.
        "/chunked" => {
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\n\
                      Transfer-Encoding: chunked\r\n\r\n",
                )
                .await?;
            for _ in 0..4 {
                socket.write_all(b"400\r\n").await?;
                socket.write_all(&[b'x'; 1024]).await?;
                socket.write_all(b"\r\n").await?;
            }
            socket.write_all(b"0\r\n\r\n").await?;
        }
        _ => {
            socket
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/pdf\r\n\
                         Content-Length: {}\r\nConnection: close\r\n\r\n",
                        SMALL.len()
                    )
                    .as_bytes(),
                )
                .await?;
            socket.write_all(SMALL).await?;
        }
    }
    socket.flush().await
}

fn site() -> SiteSource {
    let policy = HostPolicy {
        max_requests_per_second: 500.0,
        timeout: Duration::from_secs(20),
        max_bytes: CEILING,
        ..HostPolicy::default()
    };
    SiteSource::new(
        SourceId::new("hillsborough").unwrap(),
        "http://127.0.0.1",
        policy,
        DiscoveryLimits::default(),
    )
    .unwrap()
}

async fn acquire(url: String) -> Result<Vec<u8>, Refusal> {
    let site = site();
    let resource = Resource::new(site.id().clone(), url);
    site.acquire(&resource, &Frontier::default(), &Progress::none())
        .await
        .map(|mut got| got.remove(0).fetched.bytes)
}

/// The 830 MB slideshow: refused on what the server says, before the body is read.
#[tokio::test]
async fn a_declared_length_over_the_ceiling_is_refused_unread() {
    let base = serve().await;
    let started = Instant::now();
    let refusal = acquire(format!("{base}/declared")).await.unwrap_err();

    assert!(
        refusal.detail.contains("over the 1.0 KiB ceiling"),
        "{refusal}"
    );
    assert!(refusal.detail.contains("declared"), "{refusal}");
    assert_eq!(refusal.too_large, Some(1_000_000), "the size it declared");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "it waited on the body: {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn a_body_with_no_length_is_refused_once_it_passes_the_ceiling() {
    let base = serve().await;
    let refusal = acquire(format!("{base}/chunked")).await.unwrap_err();
    assert!(
        refusal.detail.contains("over the 1.0 KiB ceiling"),
        "{refusal}"
    );
    let read = refusal.too_large.expect("the size is recorded");
    assert!(read > CEILING, "as much as had arrived: {read}");
}

#[tokio::test]
async fn a_body_under_the_ceiling_arrives_whole() {
    let base = serve().await;
    let bytes = acquire(format!("{base}/small.pdf")).await.unwrap();
    assert_eq!(bytes, SMALL);
}
