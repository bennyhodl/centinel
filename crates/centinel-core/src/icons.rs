//! A site's favicon, found once and kept, so the web can draw it beside every source and host.
//!
//! Not part of collection. A run never fetches an icon and an icon is never an Observation:
//! the page asks (`GET /workspace/icons/{host}`), and the first ask for a host does the
//! work. The answer is kept in `icons/` and the second ask is a file read.
//!
//! ## Cheapest evidence first
//!
//! 1. **What the store already holds.** A source's front page is usually collected, so the
//!    `<link rel="icon">` tags are in a blob we own. That costs no request.
//! 2. **One request for the front page**, when it was not collected, to read those tags.
//! 3. **`/favicon.ico`**, which is where a browser looks when a page declares nothing.
//!
//! The icon the page declares is tried first and `/favicon.ico` second, so a host is asked
//! for two or three things, once. Among the declared icons the best one is taken: an SVG
//! (sharp at any size), else the smallest PNG that is at least 32 px (a 16 px slot on a
//! 2x screen), else an `.ico`. See [`best`].
//!
//! ## Derived, and safe to delete
//!
//! `icons/<host>.<ext>` is the image and `icons/<host>.json` says where it came from. A
//! host with no icon has only the record: *none, checked at T*, and it is not asked again
//! for [`MISS_DAYS`] days, so a page of fifty icon-less hosts does not hammer anyone on
//! every load. Deleting `icons/` loses nothing: the next request finds each icon again.
//! The record is written after the image, so a crash leaves an image nobody reads rather
//! than a record naming one that is not there.
//!
//! ## Which hosts may be fetched
//!
//! Only hosts the store already knows: the host of a defined source ([`Sources`]) or one a
//! defined source's pages link to ([`Linkages`]). Anything else is refused without a
//! request. That is the whole defence of a route that makes the server fetch what a browser
//! names — it cannot be pointed at an address nobody put in the archive. Crumbs come from pages we collected, so
//! a crumb host must also be a public name: an IP literal or a bare `localhost` in a link
//! is a page trying to aim the server inward. (A name that *resolves* inward is not caught
//! here; the redirect rule below covers the other half only.)
//!
//! No third-party icon service is asked. It would learn every host the archive follows.
//!
//! ## What a fetched icon must be
//!
//! The bytes decide, not the header: servers answer an `.ico` as `text/plain` and a missing
//! one as `200 text/html`. [`sniff`] accepts the image formats a browser draws in an
//! `<img>` and nothing else, and the type it names is the one served. An icon over
//! [`MAX_BYTES`] is refused rather than truncated, since half an image is not one.
//!
//! ## One fetch per host, four at a time
//!
//! The graph may ask for hundreds of hosts at once. Two asks for the same uncached host
//! share one fetch (the second waits on the first's lock, then reads what it wrote), and
//! at most [`CONCURRENT_FETCHES`] requests are outbound across all hosts.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, Semaphore};
use url::Url;

use crate::catalog::Sources;
use crate::config::SourceConfig;
use crate::domain::SourceId;
use crate::graph::Linkages;
use crate::html::Scan;
use crate::policy::HostPolicy;
use crate::store::{Store, write_atomically};

/// How long "this host has no icon" stands before it is asked again.
pub const MISS_DAYS: i64 = 7;
/// The most bytes read from any one response, page or icon.
pub const MAX_BYTES: usize = 256 * 1024;
/// Outbound requests in flight across every host.
pub const CONCURRENT_FETCHES: usize = 4;
/// An icon is a small file from a front door; ten seconds is generous.
const TIMEOUT: Duration = Duration::from_secs(10);
const MAX_REDIRECTS: usize = 3;
/// The edge a declared PNG has to reach to count as sharp in a 16 px slot on a 2x screen.
const SHARP_PX: u32 = 32;

/// An icon and what to serve it as.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Icon {
    pub content_type: String,
    pub bytes: Vec<u8>,
}

/// The favicon service for one store. Cheap to clone; clones share their locks.
#[derive(Clone, Debug)]
pub struct Icons {
    store: Store,
    shared: Arc<Shared>,
}

#[derive(Debug)]
struct Shared {
    /// One lock per host. Holding it is what makes a second ask wait for the first.
    hosts: StdMutex<HashMap<String, Arc<Mutex<()>>>>,
    fetches: Semaphore,
}

/// What `icons/<host>.json` holds.
#[derive(Debug, Serialize, Deserialize)]
struct Record {
    checked: Timestamp,
    /// Absent is the miss.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    icon: Option<Kept>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Kept {
    ext: String,
    content_type: String,
    /// Where the bytes came from, for whoever wonders why a host has the icon it has.
    from: String,
}

/// What the cache says about a host.
enum Cached {
    Hit(Icon),
    /// Looked recently and found nothing.
    Missed,
    /// Never looked, or the miss is old enough to look again.
    Unknown,
}

/// A host the store knows, and the address to ask it at.
#[derive(Debug)]
struct Address {
    front: Url,
    /// The source whose log may already hold the front page.
    source: Option<SourceId>,
}

impl Icons {
    pub fn new(store: Store) -> Self {
        Self {
            store,
            shared: Arc::new(Shared {
                hosts: StdMutex::new(HashMap::new()),
                fetches: Semaphore::new(CONCURRENT_FETCHES),
            }),
        }
    }

    /// The icon for a host, found and kept if this is the first ask. `None` means the
    /// host has none, or is not one the store knows.
    pub async fn get(&self, host: &str) -> anyhow::Result<Option<Icon>> {
        let Some(host) = clean(host) else {
            return Ok(None);
        };
        match self.cached(&host).await {
            Cached::Hit(icon) => return Ok(Some(icon)),
            Cached::Missed => return Ok(None),
            Cached::Unknown => {}
        }

        let lock = self.lock_for(&host);
        let _only_one = lock.lock().await;
        // Whoever held the lock before us may have just answered.
        match self.cached(&host).await {
            Cached::Hit(icon) => return Ok(Some(icon)),
            Cached::Missed => return Ok(None),
            Cached::Unknown => {}
        }

        let Some(address) = self.address_of(&host).await? else {
            return Ok(None);
        };
        let found = self.find(&address).await;
        self.keep(&host, found.as_ref()).await?;
        Ok(found.map(|(_, icon)| icon))
    }

    fn lock_for(&self, host: &str) -> Arc<Mutex<()>> {
        let mut hosts = self
            .shared
            .hosts
            .lock()
            .expect("a lock map is never poisoned");
        Arc::clone(hosts.entry(host.to_string()).or_default())
    }

    // ---- which hosts the store knows ---------------------------------------------------

    async fn address_of(&self, host: &str) -> anyhow::Result<Option<Address>> {
        let defined = Sources::new(&self.store).list().await?;
        for source in &defined {
            if source.host().as_deref() == Some(host) {
                let front = front_page(source.acquisition()?.target())?;
                return Ok(Some(Address {
                    front,
                    source: SourceId::new(source.id.clone()).ok(),
                }));
            }
        }
        if is_public_name(host) && self.linked_from(&defined, host).await? {
            return Ok(Some(Address {
                front: Url::parse(&format!("https://{host}/"))?,
                source: None,
            }));
        }
        Ok(None)
    }

    /// Whether any defined source's pages link to this host. Read from each source's
    /// [`crate::graph::Linkage`], which the graph keeps current against its ledger, so a hundred hosts
    /// asked at once cost a hundred small reads and no ledger is walked.
    async fn linked_from(&self, defined: &[SourceConfig], host: &str) -> anyhow::Result<bool> {
        let linkages = Linkages::new(&self.store);
        for source in defined {
            if linkages
                .get(&SourceId::new(source.id.clone())?)
                .await?
                .hosts
                .contains_key(host)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    // ---- finding the icon --------------------------------------------------------------

    /// The icon this host serves, and the record of where it came from.
    async fn find(&self, address: &Address) -> Option<(Kept, Icon)> {
        let (page, base) = match self.stored_front_page(address).await {
            Some(stored) => stored,
            // A front door that will not open may still serve its icon.
            None => match self.fetch(&address.front).await {
                Some(fetched) => (
                    String::from_utf8_lossy(&fetched.bytes).into_owned(),
                    fetched.url,
                ),
                None => (String::new(), address.front.clone()),
            },
        };
        // The page's own choice first, then where a browser looks when a page says nothing.
        let mut tries: Vec<Url> = best(&page, &base).into_iter().collect();
        if let Ok(favicon) = base.join("/favicon.ico")
            && !tries.contains(&favicon)
        {
            tries.push(favicon);
        }
        for url in tries {
            if let Some(found) = self.fetch_icon(url).await {
                return Some(found);
            }
        }
        None
    }

    /// The front page as the log holds it, with the address it resolves links against.
    /// `None` when it was never collected, or its blob is not in this store.
    async fn stored_front_page(&self, address: &Address) -> Option<(String, Url)> {
        let replay = self.store.replay(address.source.as_ref()?).await.ok()?;
        let front = address.front.as_str().trim_end_matches('/');
        let observation = replay
            .latest_observations()
            .into_values()
            .find(|o| o.resource.natural_key.trim_end_matches('/') == front)?;
        let bytes = self.store.get_blob(&observation.blob_sha).await.ok()?;
        // A page served after a redirect resolves its links from where it ended up.
        let base = observation
            .meta
            .get("final_url")
            .and_then(|url| Url::parse(url).ok())
            .unwrap_or_else(|| address.front.clone());
        Some((String::from_utf8_lossy(&bytes).into_owned(), base))
    }

    async fn fetch_icon(&self, url: Url) -> Option<(Kept, Icon)> {
        let body = self.fetch(&url).await?;
        if body.truncated {
            return None;
        }
        let (ext, content_type) = sniff(&body.bytes)?;
        Some((
            Kept {
                ext: ext.to_string(),
                content_type: content_type.to_string(),
                from: body.url.to_string(),
            },
            Icon {
                content_type: content_type.to_string(),
                bytes: body.bytes,
            },
        ))
    }

    /// One outbound request, within the global cap. A failure is a miss, not an error:
    /// a host that is down has no icon today.
    async fn fetch(&self, url: &Url) -> Option<Body> {
        let _slot = self.shared.fetches.acquire().await.ok()?;
        match get(url).await {
            Ok(body) => Some(body),
            Err(error) => {
                tracing::debug!(%url, %error, "icon fetch failed");
                None
            }
        }
    }

    // ---- the cache ---------------------------------------------------------------------

    async fn cached(&self, host: &str) -> Cached {
        let Ok(text) = tokio::fs::read(self.store.icon_record_path(host)).await else {
            return Cached::Unknown;
        };
        // A record this build cannot read is one to write again, which is what derived means.
        let Ok(record) = serde_json::from_slice::<Record>(&text) else {
            return Cached::Unknown;
        };
        match record.icon {
            Some(kept) => match tokio::fs::read(self.store.icon_path(host, &kept.ext)).await {
                Ok(bytes) => Cached::Hit(Icon {
                    content_type: kept.content_type,
                    bytes,
                }),
                Err(_) => Cached::Unknown,
            },
            None if Timestamp::now().as_second() - record.checked.as_second()
                < MISS_DAYS * 86_400 =>
            {
                Cached::Missed
            }
            None => Cached::Unknown,
        }
    }

    /// Image first, record second: the record is what makes the image count.
    async fn keep(&self, host: &str, found: Option<&(Kept, Icon)>) -> anyhow::Result<()> {
        if let Some((kept, icon)) = found {
            write_atomically(&self.store.icon_path(host, &kept.ext), &icon.bytes)?;
        }
        let record = Record {
            checked: Timestamp::now(),
            icon: found.map(|(kept, _)| kept.clone()),
        };
        Ok(write_atomically(
            &self.store.icon_record_path(host),
            serde_json::to_vec(&record)?,
        )?)
    }
}

// ---- hosts -----------------------------------------------------------------------------

/// A bare lowercase hostname that is safe to put in a file name, or `None`.
///
/// Read as every host the operator hands over is ([`crate::crumbs::as_host`]), then held to
/// what a file name allows. Checked before anything touches the disk: the host arrives in
/// a URL path.
fn clean(host: &str) -> Option<String> {
    let host = crate::crumbs::as_host(host).ok()?;
    let first = host.chars().next()?;
    (host.len() <= 253
        && first.is_ascii_alphanumeric()
        && !host.contains("..")
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-'))
    .then_some(host)
}

/// Whether a name could be a site on the internet rather than a machine on this network.
fn is_public_name(host: &str) -> bool {
    host.parse::<IpAddr>().is_err()
        && host.contains('.')
        && !["local", "localhost", "internal"]
            .iter()
            .any(|tld| host.rsplit('.').next() == Some(tld))
}

/// `https://tampa.gov/a/b?c` → `https://tampa.gov/`.
fn front_page(address: &str) -> anyhow::Result<Url> {
    let mut url = Url::parse(address)?;
    anyhow::ensure!(
        matches!(url.scheme(), "http" | "https") && url.host_str().is_some(),
        "{address} is not a web address"
    );
    url.set_path("/");
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

// ---- the request -----------------------------------------------------------------------

struct Body {
    /// Where the redirects ended.
    url: Url,
    bytes: Vec<u8>,
    /// More was waiting than [`MAX_BYTES`].
    truncated: bool,
}

/// GETs a URL under the project's usual manners (its User-Agent) and reads at most
/// [`MAX_BYTES`] of the decoded body.
async fn get(url: &Url) -> anyhow::Result<Body> {
    let policy = HostPolicy::default();
    let mut response = reqwest::Client::builder()
        .user_agent(&policy.user_agent)
        .timeout(TIMEOUT)
        .redirect(redirects())
        .build()?
        .get(url.clone())
        .send()
        .await?;
    anyhow::ensure!(response.status().is_success(), "HTTP {}", response.status());

    let mut bytes = Vec::new();
    let mut truncated = false;
    while let Some(chunk) = response.chunk().await? {
        bytes.extend_from_slice(&chunk);
        if bytes.len() > MAX_BYTES {
            bytes.truncate(MAX_BYTES);
            truncated = true;
            break;
        }
    }
    Ok(Body {
        url: response.url().clone(),
        bytes,
        truncated,
    })
}

/// A few redirects, and none that move from where we asked to a machine on this network.
fn redirects() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        let asked = attempt.previous().first().and_then(|url| url.host_str());
        let to = attempt.url().host_str().unwrap_or_default();
        if attempt.previous().len() > MAX_REDIRECTS || (Some(to) != asked && !is_public_name(to)) {
            attempt.stop()
        } else {
            attempt.follow()
        }
    })
}

// ---- reading the page ------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    Svg,
    Png,
    Ico,
    /// A raster the browser draws but that says nothing about its size.
    Other,
}

#[derive(Debug)]
struct Candidate {
    url: Url,
    format: Format,
    /// The longest edge the page declares, if it declares one.
    px: Option<u32>,
    /// `mask-icon`: a one-colour silhouette for Safari's pinned tabs.
    mask: bool,
}

impl Candidate {
    /// Lower is better: SVG, then the smallest sharp PNG, then a PNG of unknown size,
    /// then `.ico`, then the small and odd, and a mask last.
    fn rank(&self) -> (u8, u32) {
        match (self.mask, self.format, self.px) {
            (true, ..) => (5, 0),
            (_, Format::Svg, _) => (0, 0),
            (_, Format::Png, Some(px)) if px >= SHARP_PX => (1, px),
            (_, Format::Png, None) => (2, 0),
            (_, Format::Ico, _) => (3, 0),
            (_, _, px) => (4, u32::MAX - px.unwrap_or(0)),
        }
    }
}

/// The icon a page would most like to be drawn with, its link resolved against `base`.
fn best(html: &str, base: &Url) -> Option<Url> {
    candidates(html, base)
        .into_iter()
        .min_by_key(Candidate::rank)
        .map(|candidate| candidate.url)
}

fn candidates(html: &str, base: &Url) -> Vec<Candidate> {
    let scan = Scan::new(html);
    let mut out = Vec::new();
    for tag in scan.tags(&["link"]) {
        let rel = tag.attr("rel").unwrap_or_default().to_ascii_lowercase();
        let rels: Vec<&str> = rel.split_whitespace().collect();
        let mask = rels.contains(&"mask-icon");
        if !mask
            && !rels.iter().any(|r| {
                matches!(
                    *r,
                    "icon" | "apple-touch-icon" | "apple-touch-icon-precomposed"
                )
            })
        {
            continue;
        }
        let Some(url) = tag
            .attr("href")
            .and_then(|href| base.join(crate::html::unescape(href).trim()).ok())
            .filter(|url| matches!(url.scheme(), "http" | "https"))
        else {
            continue;
        };
        let declared = tag.attr("type").unwrap_or_default().to_ascii_lowercase();
        let path = url.path().to_ascii_lowercase();
        let is = |word: &str, ext: &str| declared.contains(word) || path.ends_with(ext);
        let format = if is("svg", ".svg") {
            Format::Svg
        } else if is("png", ".png") {
            Format::Png
        } else if is("icon", ".ico") {
            Format::Ico
        } else {
            Format::Other
        };
        // `16x16 32x32` names several; `any` names none, which is right for a vector.
        let px = tag
            .attr("sizes")
            .unwrap_or_default()
            .split_whitespace()
            .filter_map(|size| {
                size.to_ascii_lowercase()
                    .split_once('x')?
                    .0
                    .parse::<u32>()
                    .ok()
            })
            .max();
        out.push(Candidate {
            url,
            format,
            px,
            mask,
        });
    }
    out
}

/// The image format these bytes are, as a file extension and a content type, or `None`
/// for anything a browser would not draw in an `<img>`.
fn sniff(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    match bytes {
        [0x89, b'P', b'N', b'G', ..] => Some(("png", "image/png")),
        [0, 0, 1, 0, ..] => Some(("ico", "image/x-icon")),
        [b'G', b'I', b'F', b'8', ..] => Some(("gif", "image/gif")),
        [0xFF, 0xD8, 0xFF, ..] => Some(("jpg", "image/jpeg")),
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => Some(("webp", "image/webp")),
        _ => {
            let head =
                String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]).to_ascii_lowercase();
            (head.contains("<svg") && !head.contains("<html")).then_some(("svg", "image/svg+xml"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SourceConfig;
    use std::net::SocketAddr;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nrest of a png";
    const ICO: &[u8] = b"\0\0\x01\0rest of an ico";
    const SVG: &[u8] = b"<?xml version=\"1.0\"?><svg xmlns=\"http://www.w3.org/2000/svg\"/>";

    /// A web server on a socket that records every path it was asked for. `routes` maps a
    /// path to a status, the type the server *claims*, and the body. A slow server holds
    /// each answer so that two asks overlap.
    struct Site {
        addr: SocketAddr,
        asked: Arc<StdMutex<Vec<String>>>,
    }

    impl Site {
        async fn start(routes: Vec<(&'static str, &'static str, &'static str, Vec<u8>)>) -> Self {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let asked = Arc::new(StdMutex::new(Vec::new()));
            let log = Arc::clone(&asked);
            tokio::spawn(async move {
                while let Ok((mut socket, _)) = listener.accept().await {
                    let routes = routes.clone();
                    let log = Arc::clone(&log);
                    tokio::spawn(async move {
                        let mut request = vec![0u8; 4096];
                        let read = socket.read(&mut request).await.unwrap_or(0);
                        let text = String::from_utf8_lossy(&request[..read]).into_owned();
                        let path = text.split_whitespace().nth(1).unwrap_or("/").to_string();
                        log.lock().unwrap().push(path.clone());
                        tokio::time::sleep(Duration::from_millis(60)).await;
                        let (status, content_type, body) = routes
                            .iter()
                            .find(|route| route.0 == path)
                            .map(|route| (route.1, route.2, route.3.clone()))
                            .unwrap_or(("404 Not Found", "text/plain", b"no".to_vec()));
                        let head = format!(
                            "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        );
                        let _ = socket.write_all(head.as_bytes()).await;
                        let _ = socket.write_all(&body).await;
                    });
                }
            });
            Self { addr, asked }
        }

        fn asked(&self) -> Vec<String> {
            self.asked.lock().unwrap().clone()
        }

        fn url(&self) -> String {
            format!("http://{}", self.addr)
        }
    }

    fn ok(
        path: &'static str,
        content_type: &'static str,
        body: &[u8],
    ) -> (&'static str, &'static str, &'static str, Vec<u8>) {
        (path, "200 OK", content_type, body.to_vec())
    }

    /// A store with one source collecting `site`, and the icons service over it.
    async fn serving(site: &Site) -> (tempfile::TempDir, Store, Icons) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        Sources::new(&store)
            .add(&SourceConfig::site("town", site.url()))
            .await
            .unwrap();
        let icons = Icons::new(store.clone());
        (dir, store, icons)
    }

    fn page_declaring(icon: &str) -> String {
        format!(r#"<html><head><link rel="icon" href="{icon}"></head><body>hi</body></html>"#)
    }

    /// The common case: the front page is already in the store, so its `<link>` is read
    /// from our own blob. The page is never requested; only the icon is.
    #[tokio::test]
    async fn the_icon_link_is_read_from_the_collected_front_page_with_no_request_for_it() {
        let site = Site::start(vec![ok("/assets/mark.png", "image/png", PNG)]).await;
        let (_dir, store, icons) = serving(&site).await;
        let resource = crate::domain::Resource::new(
            SourceId::new("town").unwrap(),
            format!("{}/", site.url()),
        );
        store
            .record_observation(
                &resource,
                page_declaring("/assets/mark.png").as_bytes(),
                Timestamp::now(),
                Default::default(),
            )
            .await
            .unwrap();

        let icon = icons
            .get("127.0.0.1")
            .await
            .unwrap()
            .expect("the page declares an icon");

        assert_eq!(icon.content_type, "image/png");
        assert_eq!(icon.bytes, PNG);
        assert_eq!(
            site.asked(),
            ["/assets/mark.png"],
            "the page itself came from the store"
        );
    }

    /// Nothing collected and nothing declared: one request for the page, then the place a
    /// browser looks. The server calls the `.ico` `text/plain`; its bytes say what it is.
    /// The answer is then kept, and the second ask is a file read.
    #[tokio::test]
    async fn without_a_collected_page_it_asks_once_then_falls_back_to_favicon_ico() {
        let site = Site::start(vec![
            ok(
                "/",
                "text/html",
                b"<html><head><title>x</title></head></html>",
            ),
            ok("/favicon.ico", "text/plain", ICO),
        ])
        .await;
        let (_dir, store, icons) = serving(&site).await;

        let icon = icons
            .get("127.0.0.1")
            .await
            .unwrap()
            .expect("favicon.ico is served");

        assert_eq!(icon.content_type, "image/x-icon");
        assert_eq!(site.asked(), ["/", "/favicon.ico"]);
        assert!(store.icon_path("127.0.0.1", "ico").exists());

        assert_eq!(icons.get("127.0.0.1").await.unwrap(), Some(icon));
        assert_eq!(site.asked().len(), 2, "kept, so not asked again");
    }

    /// A source whose log names the front page but whose blob this store does not hold
    /// (a log copied without `blobs/`) falls through to the network rather than failing.
    #[tokio::test]
    async fn a_front_page_whose_blob_is_missing_is_fetched_instead() {
        let site = Site::start(vec![
            ok("/", "text/html", page_declaring("/m.svg").as_bytes()),
            ok("/m.svg", "image/svg+xml", SVG),
        ])
        .await;
        let (_dir, store, icons) = serving(&site).await;
        let resource = crate::domain::Resource::new(
            SourceId::new("town").unwrap(),
            format!("{}/", site.url()),
        );
        let obs = store
            .record_observation(
                &resource,
                b"collected once",
                Timestamp::now(),
                Default::default(),
            )
            .await
            .unwrap();
        std::fs::remove_file(store.blob_path_of(&obs.blob_sha)).unwrap();

        let icon = icons
            .get("127.0.0.1")
            .await
            .unwrap()
            .expect("the live page declares one");

        assert_eq!(icon.content_type, "image/svg+xml");
        assert_eq!(site.asked(), ["/", "/m.svg"]);
    }

    #[test]
    fn an_svg_beats_a_sharp_png_beats_an_ico() {
        let base = Url::parse("https://city.gov/news/").unwrap();
        let tags = |links: &[&str]| {
            links
                .iter()
                .map(|l| format!("<link {l}>"))
                .collect::<String>()
        };
        let ico = r#"rel="shortcut icon" href="/favicon.ico""#;
        let small = r#"rel="icon" type="image/png" sizes="16x16" href="s16.png""#;
        let big = r#"rel="icon" type="image/png" sizes="96x96" href="/b96.png""#;
        let medium = r#"rel="apple-touch-icon" sizes="32x32" href="/m32.png""#;
        let svg = r#"rel="icon" type="image/svg+xml" href="/mark.svg""#;
        let mask = r#"rel="mask-icon" href="/pin.svg" color="black""#;
        let chosen = |links: &[&str]| best(&tags(links), &base).unwrap().to_string();

        assert_eq!(
            chosen(&[ico, small, big, svg, mask]),
            "https://city.gov/mark.svg"
        );
        assert_eq!(
            chosen(&[ico, small, big, medium]),
            "https://city.gov/m32.png",
            "the smallest that is sharp"
        );
        assert_eq!(
            chosen(&[ico, small]),
            "https://city.gov/favicon.ico",
            "a 16 px PNG is blurrier than the ico"
        );
        assert_eq!(
            chosen(&[small, mask]),
            "https://city.gov/news/s16.png",
            "relative to the page; a mask is last"
        );
        assert!(
            best(
                "<html><link rel=\"stylesheet\" href=\"/a.css\"></html>",
                &base
            )
            .is_none()
        );
    }

    /// Fifty icon-less hosts on a page must not mean fifty requests per load.
    #[tokio::test]
    async fn a_miss_is_remembered_for_a_week_and_then_asked_again() {
        let site = Site::start(vec![]).await;
        let (_dir, store, icons) = serving(&site).await;

        assert_eq!(icons.get("127.0.0.1").await.unwrap(), None);
        let first = site.asked();
        assert_eq!(first, ["/", "/favicon.ico"]);
        assert_eq!(icons.get("127.0.0.1").await.unwrap(), None);
        assert_eq!(site.asked(), first, "a fresh miss costs no request");

        let eight_days_ago = Timestamp::now() - jiff::SignedDuration::from_hours(8 * 24);
        let stale = serde_json::to_vec(&Record {
            checked: eight_days_ago,
            icon: None,
        })
        .unwrap();
        std::fs::write(store.icon_record_path("127.0.0.1"), stale).unwrap();
        assert_eq!(icons.get("127.0.0.1").await.unwrap(), None);
        assert_eq!(site.asked().len(), 4, "a week-old miss is looked at again");
    }

    /// The route makes this server fetch what a browser names, so the host has to be one
    /// the archive already holds. Nothing else is asked, and nothing is written for it.
    #[tokio::test]
    async fn a_host_the_store_does_not_know_is_refused_without_a_request() {
        let site = Site::start(vec![ok("/favicon.ico", "image/x-icon", ICO)]).await;
        let (_dir, store, icons) = serving(&site).await;

        for host in [
            "localhost",
            "example.gov",
            "127.0.0.2",
            "../etc/passwd",
            "a/b",
            "",
        ] {
            assert_eq!(icons.get(host).await.unwrap(), None, "{host:?}");
        }

        assert!(site.asked().is_empty());
        assert!(!store.icons_dir().exists(), "a refusal leaves no record");
    }

    /// A host a defined source's crumbs name is known; one no crumb names is not. A crumb that points at an
    /// address on this network is a page aiming the server inward, and is not.
    #[tokio::test]
    async fn a_host_a_crumb_names_is_known_and_an_internal_one_is_not() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        Sources::new(&store)
            .add(&SourceConfig::site("town", "https://town.gov"))
            .await
            .unwrap();
        let ledger = crate::crumbs::Ledger::new(&store, SourceId::new("town").unwrap());
        let page = crate::crumbs::scan_page(
            r#"<a href="https://portal.vendor.com/x">a</a><a href="http://169.254.169.254/latest">b</a>"#,
            crate::crumbs::Carrier::at_address("https://town.gov/"),
        );
        ledger.append(&page).await.unwrap();
        let icons = Icons::new(store);

        let known = icons
            .address_of("portal.vendor.com")
            .await
            .unwrap()
            .expect("named by a crumb");
        assert_eq!(known.front.as_str(), "https://portal.vendor.com/");
        assert!(
            icons
                .address_of("other.vendor.com")
                .await
                .unwrap()
                .is_none()
        );
        assert!(icons.address_of("169.254.169.254").await.unwrap().is_none());
    }

    /// Only an image is kept: a soft 404 served as `200 text/html` and a file too large to
    /// be a favicon are both misses, however the server labels them.
    #[tokio::test]
    async fn only_an_image_within_the_cap_is_kept() {
        let mut huge = PNG.to_vec();
        huge.resize(MAX_BYTES + 1, 0);
        let site = Site::start(vec![
            ok("/", "text/html", page_declaring("/huge.png").as_bytes()),
            ok("/huge.png", "image/png", &huge),
            ok(
                "/favicon.ico",
                "image/x-icon",
                b"<html>Not found, honest</html>",
            ),
        ])
        .await;
        let (_dir, _store, icons) = serving(&site).await;

        assert_eq!(icons.get("127.0.0.1").await.unwrap(), None);
        assert_eq!(site.asked(), ["/", "/huge.png", "/favicon.ico"]);
    }

    #[tokio::test]
    async fn two_asks_for_one_uncached_host_share_one_fetch() {
        let site = Site::start(vec![
            ok("/", "text/html", b"<html></html>"),
            ok("/favicon.ico", "image/x-icon", ICO),
        ])
        .await;
        let (_dir, _store, icons) = serving(&site).await;

        let (a, b) = tokio::join!(icons.get("127.0.0.1"), icons.get("127.0.0.1"));

        assert_eq!(a.unwrap(), b.unwrap());
        assert_eq!(
            site.asked(),
            ["/", "/favicon.ico"],
            "one page, one icon, however many asked"
        );
    }

    #[test]
    fn only_a_public_name_may_be_reached_on_a_crumbs_word() {
        assert!(is_public_name("portal.vendor.com"));
        for host in [
            "localhost",
            "10.0.0.1",
            "printer",
            "nas.local",
            "db.internal",
        ] {
            assert!(!is_public_name(host), "{host}");
        }
    }
}
