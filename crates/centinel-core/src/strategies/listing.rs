//! `listing` — an open directory index, which is what a server does when nobody
//! configured it.
//!
//! Keyed on a **server default** rather than a product, so it is the broadest recogniser
//! in the registry: any IIS, Apache or nginx host with directory browsing left on answers
//! to it, whoever runs it and whatever it holds.
//!
//! ## Why this one is second
//!
//! It is the richest source in `docs/FIELD-NOTES.md` and the cheapest to collect. Entry 4
//! system C — `publicrec.hillsclerk.com`, two clicks from a search page that could not be
//! collected at all — is roughly **6 GB** across ~1,500 files, refreshed daily, with no
//! authentication, no POST, no token, no view state, no cap and no JavaScript:
//!
//! > After three entries of increasingly baroque discovery, the richest source in the file
//! > is an Apache-style index from 2014.
//!
//! ## What it will not do
//!
//! **It never leaves the directory it was pointed at.** Every candidate must be on the
//! seed's host and under the seed's path, so a listing that links to `/` cannot turn a
//! request for one folder into a walk of the whole server. That rule is also what makes
//! the parent link a non-event: `/` does not start with `/Civil/`, so it is dropped by the
//! same test that drops a link to another host.

use std::collections::HashSet;

use futures::future::BoxFuture;
use url::Url;

use super::{Addresses, Crawl, Enumerated, Keyed, Recognition, Seed, Strategy, StrategyDef};
use crate::domain::Note;

pub struct Listing;

inventory::submit! { StrategyDef { name: "listing", it: &Listing } }

/// Directory nesting depth. Entry 4's deepest measured tree is
/// `/Criminal/name_index/hccc1020/` at three, so this is not a tight bound — it is a stop
/// for a symlink loop that a server presents as an infinitely deep tree.
const MAX_DEPTH: usize = 10;

/// IIS writes this on every generated listing and nothing else does.
const IIS_MARKER: &str = "[to parent directory]";

/// Apache and nginx both title the page this way.
const UNIX_MARKER: &str = "index of /";

impl Strategy for Listing {
    fn name(&self) -> &'static str {
        "listing"
    }

    fn recognise(&self, seed: &Seed) -> Option<Recognition> {
        let page = seed.text();
        let lower = page.to_ascii_lowercase();

        // Two server families, one shape. The recognition names which was seen, because
        // that is the fact an operator checks, and because the two disagree about almost
        // everything else they emit.
        let server = if lower.contains(IIS_MARKER) {
            "IIS directory index"
        } else if lower.contains(UNIX_MARKER) {
            "Apache/nginx directory index"
        } else {
            return None;
        };

        let url = seed.final_url()?;
        // A page that merely *mentions* the phrase is not a listing. A real one is a list
        // of links, so require some.
        let links = crate::html::Scan::new(&page).tags(&["a"]).len();
        if links < 2 {
            return None;
        }

        let mut r = Recognition::new(self.name(), Keyed::ServerDefault(server))
            .seeing("markup", format!("the page carries a {server}"))
            .seeing("links", format!("{links} anchors, one per row"))
            .seeing(
                "root",
                format!("the walk stays under {}", directory_of(&url)),
            );

        if !seed.robots.declared {
            // Measured on this exact host: `publicrec.hillsclerk.com` answers 404 for
            // robots.txt. Worth saying, because it is why the walk is bounded by the
            // seed's own path rather than by the host's rules.
            r = r.warning("robots.txt", "unreachable — rules were assumed, not read");
        }
        Some(r)
    }

    fn enumerate<'a>(
        &'a self,
        seed: &'a Seed,
        crawl: &'a dyn Crawl,
    ) -> BoxFuture<'a, anyhow::Result<Enumerated>> {
        Box::pin(async move {
            let start = seed
                .final_url()
                .ok_or_else(|| anyhow::anyhow!("the seed does not record where it was served"))?;
            let root = directory_of(&start);
            let mut out = Enumerated::default();
            out.notes.push(Note::new("root", root.to_string()));

            let mut queue: Vec<(Url, usize)> = vec![(root.clone(), 0)];
            let mut visited: HashSet<String> = HashSet::new();
            let mut seen: HashSet<String> = HashSet::new();
            let mut directories = 0u64;
            let mut disallowed = 0u64;

            while let Some((dir, depth)) = queue.pop() {
                if !crawl.may_fetch() {
                    out.warnings.push(format!(
                        "stopped at the {}-request budget; the tree is larger than this \
                         run captured",
                        crawl.budget()
                    ));
                    break;
                }
                // Here rather than only where an address is pushed, so the walk stops
                // instead of listing the rest of the tree to discard it — and so the
                // warning is written once rather than once per directory after the cap.
                if out.addresses.len() >= crawl.max_addresses() {
                    out.warnings.push(format!(
                        "stopped at {} addresses; the tree is larger than this run \
                         captured",
                        crawl.max_addresses()
                    ));
                    break;
                }
                if !visited.insert(dir.to_string()) {
                    continue;
                }
                if depth > MAX_DEPTH {
                    out.warnings
                        .push(format!("depth limit reached, skipping {dir}"));
                    continue;
                }

                crawl.progress().step(
                    format!("listing {}", dir.path()),
                    directories,
                    directories + queue.len() as u64 + 1,
                );

                // The root's bytes are already in hand: they are what the recogniser read.
                // Fetching them again would spend a request to learn what we know, and on
                // a one-directory tree that is the *only* request.
                let body = match depth {
                    0 => seed.page.bytes.clone(),
                    _ => match crawl.get(dir.as_str()).await {
                        Ok(f) => f.bytes,
                        Err(refusal) => {
                            out.warnings.push(format!("{dir}: {refusal}"));
                            continue;
                        }
                    },
                };
                directories += 1;

                let html = String::from_utf8_lossy(&body);
                for target in links_in(&html, &dir, &root) {
                    match target.as_str().ends_with('/') {
                        true => queue.push((target, depth + 1)),
                        false => {
                            // The loop above writes the warning and ends the walk.
                            if out.addresses.len() >= crawl.max_addresses() {
                                break;
                            }
                            if !seed.robots.allowed(target.as_str()) {
                                disallowed += 1;
                                continue;
                            }
                            if seen.insert(target.to_string()) {
                                out.addresses.push(target.to_string());
                            }
                        }
                    }
                }
            }

            if disallowed > 0 {
                out.notes.push(Note::marked(
                    "disallowed",
                    format!(
                        "{} excluded by the site's own rules",
                        crate::render::count(disallowed)
                    ),
                    crate::domain::NoteMark::Ok,
                ));
            }
            out.notes.push(Note::new(
                "directories",
                format!("{} walked", crate::render::count(directories)),
            ));
            out.figures.insert("directories".into(), directories);
            out.figures.insert("disallowed".into(), disallowed);

            Ok(out)
        })
    }

    /// A file in a directory listing **is** the document.
    ///
    /// So acquisition runs no enclosure scan over it. Nothing is nested here, and entry 4
    /// system C is 6 GB of csv, txt, pdf and zip — none of which is a wrapper around
    /// anything.
    fn addresses_are(&self) -> Addresses {
        Addresses::Documents
    }
}

/// The directory a URL sits in: `…/Civil/index.html` → `…/Civil/`.
///
/// A listing served from a file path still bounds its walk by the folder, which is what a
/// person clicking the same link would get.
fn directory_of(url: &Url) -> Url {
    let path = url.path();
    if path.ends_with('/') {
        return url.clone();
    }
    let cut = path.rfind('/').map_or(1, |i| i + 1);
    let mut dir = url.clone();
    dir.set_path(&path[..cut]);
    dir.set_query(None);
    dir.set_fragment(None);
    dir
}

/// Every link on a listing page that is inside the tree being walked.
///
/// Parsed from `href` attributes, never scanned out of the text. A listing's markup is
/// trivial, but the rule holds anyway: a scan of a script body is what produced
/// `/251agendaonline/.pdf?documentType=`, an address naming no document.
fn links_in(html: &str, page: &Url, root: &Url) -> Vec<Url> {
    let mut out = Vec::new();
    for tag in crate::html::Scan::new(html).tags(&["a"]) {
        let Some(href) = tag.attr("href") else {
            continue;
        };
        // `&amp;` in an attribute is an escaped `&`, and joining it unescaped yields a
        // different address.
        let Ok(target) = page.join(&crate::html::unescape(href)) else {
            continue;
        };
        // Same host, and inside the tree. This is what makes the parent link a non-event
        // rather than a special case: `/` does not start with `/Civil/`.
        if target.host_str() != root.host_str() || !target.path().starts_with(root.path()) {
            continue;
        }
        // A fragment is the same page, and a sort link (`?C=N;O=D` on Apache) is the same
        // listing in another order — following either is a loop with extra steps.
        if target.path() == page.path() {
            continue;
        }
        out.push(target);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::tests::{Fake, seed};

    /// The shape IIS emits, trimmed to what matters. Measured on
    /// `publicrec.hillsclerk.com`.
    fn iis(path: &str, rows: &str) -> String {
        format!(
            "<html><head><title>publicrec.hillsclerk.com - {path}</title></head><body>\
             <H1>publicrec.hillsclerk.com - {path}</H1><hr><pre>\
             <A HREF=\"/\">[To Parent Directory]</A><br><br>{rows}</pre><hr></body></html>"
        )
    }

    #[test]
    fn both_server_families_are_recognised_and_named() {
        let s = seed(
            &iis("/Civil/", "<A HREF=\"/Civil/a.csv\">a.csv</A>"),
            "https://p.gov/Civil/",
        );
        let r = Listing.recognise(&s).expect("an IIS listing");
        assert_eq!(r.keyed_on, Keyed::ServerDefault("IIS directory index"));

        let unix = seed(
            "<html><head><title>Index of /pub</title></head><body>\
             <a href=\"../\">Parent</a><a href=\"x.txt\">x.txt</a></body></html>",
            "https://p.gov/pub/",
        );
        let r = Listing.recognise(&unix).unwrap();
        assert_eq!(
            r.keyed_on,
            Keyed::ServerDefault("Apache/nginx directory index")
        );
    }

    /// A page that says the words is not a page that is one.
    #[test]
    fn prose_about_a_directory_index_is_not_a_directory_index() {
        let s = seed(
            "<html><body><p>Our server shows an Index of / when browsing is on.</p></body></html>",
            "https://p.gov/help",
        );
        assert!(Listing.recognise(&s).is_none());
    }

    #[tokio::test]
    async fn the_tree_is_walked_and_the_files_come_back() {
        let root = iis(
            "/Civil/",
            "<A HREF=\"/Civil/bulkdata/\">bulkdata</A><A HREF=\"/Civil/readme.txt\">readme.txt</A>",
        );
        let s = seed(&root, "https://p.gov/Civil/");
        let crawl = Fake::new([
            ("https://p.gov/Civil/", root.clone()),
            (
                "https://p.gov/Civil/bulkdata/",
                iis(
                    "/Civil/bulkdata/",
                    "<A HREF=\"/Civil/bulkdata/f.csv\">f.csv</A>",
                ),
            ),
        ]);

        let got = Listing.enumerate(&s, &crawl).await.unwrap();
        let mut found = got.addresses.clone();
        found.sort();
        assert_eq!(
            found,
            [
                "https://p.gov/Civil/bulkdata/f.csv",
                "https://p.gov/Civil/readme.txt"
            ]
        );
        assert_eq!(got.figures["directories"], 2);
    }

    /// The rule that keeps a request for one folder from becoming a walk of the server.
    #[tokio::test]
    async fn the_walk_never_climbs_out_of_the_directory_it_started_in() {
        let root = iis(
            "/Civil/",
            "<A HREF=\"/Criminal/\">Criminal</A>\
             <A HREF=\"https://elsewhere.gov/x.csv\">offsite</A>\
             <A HREF=\"/Civil/ok.csv\">ok.csv</A>",
        );
        let s = seed(&root, "https://p.gov/Civil/");
        let crawl = Fake::new([("https://p.gov/Civil/", root.clone())]);

        let got = Listing.enumerate(&s, &crawl).await.unwrap();
        assert_eq!(got.addresses, ["https://p.gov/Civil/ok.csv"]);
        // The parent link, the sibling tree and the other host are all dropped by the
        // same test, so `[To Parent Directory]` needs no special case. And the root cost
        // nothing: the recogniser had already read those bytes.
        assert_eq!(crawl.requests(), 0);
    }

    #[tokio::test]
    async fn a_listing_that_links_to_itself_is_not_walked_twice() {
        let root = iis(
            "/Civil/",
            "<A HREF=\"/Civil/\">.</A><A HREF=\"/Civil/a.csv\">a.csv</A>",
        );
        let s = seed(&root, "https://p.gov/Civil/");
        let crawl = Fake::new([("https://p.gov/Civil/", root.clone())]);

        let got = Listing.enumerate(&s, &crawl).await.unwrap();
        assert_eq!(got.addresses, ["https://p.gov/Civil/a.csv"]);
        assert_eq!(crawl.requests(), 0);
    }

    #[tokio::test]
    async fn a_subdirectory_that_refuses_does_not_cancel_its_siblings() {
        let root = iis(
            "/Civil/",
            "<A HREF=\"/Civil/gone/\">gone</A><A HREF=\"/Civil/here/\">here</A>",
        );
        let s = seed(&root, "https://p.gov/Civil/");
        let crawl = Fake::new([
            ("https://p.gov/Civil/", root.clone()),
            (
                "https://p.gov/Civil/here/",
                iis("/Civil/here/", "<A HREF=\"/Civil/here/x.csv\">x.csv</A>"),
            ),
        ]);

        let got = Listing.enumerate(&s, &crawl).await.unwrap();
        assert_eq!(got.addresses, ["https://p.gov/Civil/here/x.csv"]);
        assert!(got.warnings.iter().any(|w| w.contains("gone")));
    }

    #[test]
    fn a_directory_is_taken_from_a_file_path() {
        let file = Url::parse("https://p.gov/Civil/index.html?sort=n#top").unwrap();
        assert_eq!(directory_of(&file).as_str(), "https://p.gov/Civil/");
        let dir = Url::parse("https://p.gov/Civil/").unwrap();
        assert_eq!(directory_of(&dir).as_str(), "https://p.gov/Civil/");
        let bare = Url::parse("https://p.gov").unwrap();
        assert_eq!(directory_of(&bare).as_str(), "https://p.gov/");
    }

    /// A document is a document, so nothing scans it for enclosures afterwards.
    #[test]
    fn a_file_in_a_listing_is_the_document() {
        assert_eq!(Listing.addresses_are(), Addresses::Documents);
    }
}
