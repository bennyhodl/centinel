//! `links` — the seed, and the readable pages it links to inside its scope.
//!
//! What a person does on a site with no sitemap: open the page they were given and click
//! through. A Google Site serves every page as plain HTML, publishes neither `robots.txt`
//! nor a sitemap, and a sitemap walk of it ends at zero addresses — measured on
//! `pinellasdsa.org`, whose `/home`, `/join`, `/calendar`, `/work`, `/blog` and `/contact`
//! are all one link from the front door.
//!
//! ## What it will not do
//!
//! **Leave its [`Scope`].** Every link it follows is on the seed's origin and under the
//! seed's path, so a seed of `linktr.ee/TampaDSA` is that page and not the platform. A link
//! anywhere else is not followed; an off-host one becomes a crumb when the page is
//! collected, which is how a host is promoted to its own source by a person.
//!
//! **Follow a link to a document.** A PDF linked from a page is found again by the
//! enclosure scan when the page is acquired, and fetching it here as well would spend a
//! request to learn its address.
//!
//! **Recognise anything.** It keys on nothing — every site has links — so it never answers
//! [`Strategy::recognise`]. It runs because it was pinned, or because the strategy that
//! ran read nothing at all ([`super::last_resort`]).
//!
//! The seed is always an address, read or not. A seed that answers 403 is still the page
//! the operator named, and acquiring it is what records it as blocked; a run that returned
//! nothing would record nothing at all.

use futures::future::BoxFuture;
use url::Url;

use super::scope::Scope;
use super::{Enumerated, Pass, Seed, Strategy, StrategyDef, Walk};
use crate::content::ContentKind;
use crate::domain::Note;
use crate::strategies::Recognition;

pub struct Links;

inventory::submit! { StrategyDef { name: "links", it: &Links } }

/// Clicks from the seed. A small site's every page is within two or three; this is a stop
/// for a calendar that links to next month forever, not a tuning knob — the request
/// budget is what bounds the work.
const MAX_DEPTH: usize = 4;

impl Strategy for Links {
    fn name(&self) -> &'static str {
        "links"
    }

    fn recognise(&self, _seed: &Seed) -> Option<Recognition> {
        None
    }

    fn enumerate<'a>(
        &'a self,
        seed: &'a Seed,
        crawl: &'a dyn Walk,
    ) -> BoxFuture<'a, anyhow::Result<Enumerated>> {
        Box::pin(async move {
            let mut start = seed
                .final_url()
                .ok_or_else(|| anyhow::anyhow!("the seed does not record where it was served"))?;
            start.set_fragment(None);
            let scope = Scope::of(&start);
            let robots = &seed.robots;

            let mut pass = Pass::new(crawl, "site", MAX_DEPTH);
            pass.note(Note::new("scope", scope.to_string()));
            pass.keep(start.to_string(), robots);
            pass.push(start.to_string(), 0);

            let mut outside = 0u64;
            while let Some((page, depth)) = pass.next_to_visit() {
                pass.visiting(format!("page {page}"));

                // The seed's bytes are already in hand: they are what recognition read.
                // Empty means the seed refused, and asking again would only refuse again.
                let fetched = match depth {
                    0 if seed.page.bytes.is_empty() => {
                        pass.warn(format!(
                            "{page}: the seed could not be read, so no links were followed"
                        ));
                        continue;
                    }
                    0 => seed.page.clone(),
                    _ => match pass.walk().get(&page).await {
                        Ok(f) => f,
                        Err(refusal) => {
                            pass.refused(&page, &refusal);
                            continue;
                        }
                    },
                };
                if ContentKind::classify(&fetched.meta, &fetched.bytes) != ContentKind::Html {
                    continue;
                }
                pass.read_surface();

                // Relative links resolve against where the page was served, which a
                // redirect may have moved.
                let base = fetched
                    .meta
                    .get("final_url")
                    .and_then(|u| Url::parse(u).ok())
                    .unwrap_or_else(|| start.clone());
                let html = String::from_utf8_lossy(&fetched.bytes);
                for target in pages_in(&html, &base) {
                    if !scope.contains(&target) {
                        if target.origin() == start.origin() {
                            outside += 1;
                        }
                        continue;
                    }
                    if pass.full() {
                        break;
                    }
                    let address = target.to_string();
                    // Checked before it is queued as well as when it is kept: a page the
                    // host disallows is neither collected nor fetched to find its links.
                    if robots.allowed(&address) {
                        pass.push(address.clone(), depth + 1);
                    }
                    pass.keep(address, robots);
                }
            }

            let pages = pass.visits();
            pass.figure("pages_visited", pages);
            pass.figure("outside_scope", outside);
            if outside > 0 {
                pass.note(Note::new(
                    "outside scope",
                    format!(
                        "{} link(s) to this host outside {scope} were not followed",
                        crate::render::count(outside)
                    ),
                ));
            }
            Ok(pass.finish())
        })
    }

    fn first_address_is_the_seed(&self) -> bool {
        true
    }
}

/// Every `<a href>` on a page that could be a page: http(s), fragment dropped, and not an
/// address whose name says it is a document.
fn pages_in(html: &str, base: &Url) -> Vec<Url> {
    let mut out = Vec::new();
    for tag in crate::html::Scan::new(html).tags(&["a"]) {
        let Some(href) = tag.attr("href") else {
            continue;
        };
        // `&amp;` in an attribute is an escaped `&`, and joining it unescaped yields a
        // different address.
        let Ok(mut target) = base.join(&crate::html::unescape(href)) else {
            continue;
        };
        if !matches!(target.scheme(), "http" | "https") {
            continue;
        }
        // A fragment is a place on the same page.
        target.set_fragment(None);
        let named = ContentKind::from_path(std::path::Path::new(target.path()));
        if named.is_some_and(|k| k != ContentKind::Html) {
            continue;
        }
        out.push(target);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::crawl::tests::{Fake, seed};

    fn page(links: &[&str]) -> String {
        let anchors: String = links
            .iter()
            .map(|l| format!("<a href=\"{l}\">{l}</a>"))
            .collect();
        format!("<html><body><nav>{anchors}</nav><main>text</main></body></html>")
    }

    /// `pinellasdsa.org`: no sitemap, every page one click from the front door.
    #[tokio::test]
    async fn the_pages_linked_from_the_seed_are_walked_and_kept() {
        let s = seed(
            &page(&[
                "/home",
                "/join",
                "/blog#latest",
                "https://dsa-pinellas.medium.com/",
            ]),
            "https://www.pinellasdsa.org/",
        );
        let crawl = Fake::new([
            ("https://www.pinellasdsa.org/home", page(&["/"])),
            ("https://www.pinellasdsa.org/join", page(&["/contact"])),
            (
                "https://www.pinellasdsa.org/blog",
                page(&["/files/minutes.pdf"]),
            ),
            ("https://www.pinellasdsa.org/contact", page(&[])),
        ]);

        let got = Links.enumerate(&s, &crawl).await.unwrap();

        let mut found = got.addresses.clone();
        found.sort();
        assert_eq!(
            found,
            [
                "https://www.pinellasdsa.org/",
                "https://www.pinellasdsa.org/blog",
                "https://www.pinellasdsa.org/contact",
                "https://www.pinellasdsa.org/home",
                "https://www.pinellasdsa.org/join",
            ],
            "the seed, its pages, and no other host and no document"
        );
        assert!(!got.truncated && !got.discovery_failed);
        assert_eq!(crawl.requests(), 4, "each page once; the seed was in hand");
    }

    /// A seed on a platform is one account. The platform's own navigation is not followed.
    #[tokio::test]
    async fn a_seed_on_a_shared_host_stays_on_its_own_path() {
        let s = seed(
            &page(&["/TampaDSA/events", "/OtherOrg", "/", "/s/about"]),
            "https://linktr.ee/TampaDSA",
        );
        let crawl = Fake::new([("https://linktr.ee/TampaDSA/events", page(&[]))]);

        let got = Links.enumerate(&s, &crawl).await.unwrap();

        assert_eq!(
            got.addresses,
            [
                "https://linktr.ee/TampaDSA",
                "https://linktr.ee/TampaDSA/events"
            ]
        );
        assert_eq!(got.figures["outside_scope"], 3);
        assert_eq!(crawl.requests(), 1);
    }

    /// The seed refused: discovery failed, and the seed is still the address the operator
    /// named, so acquiring it records the refusal.
    #[tokio::test]
    async fn a_seed_that_refused_is_kept_and_discovery_is_reported_failed() {
        let s = seed("", "https://linktr.ee/TampaDSA");
        let crawl = Fake::new([]);

        let got = Links.enumerate(&s, &crawl).await.unwrap();

        assert_eq!(got.addresses, ["https://linktr.ee/TampaDSA"]);
        assert!(got.discovery_failed);
        assert!(!got.truncated);
        assert_eq!(crawl.requests(), 0, "a refused seed is not asked twice");
    }

    /// A walk that runs out of requests with pages still queued is a floor, and says so.
    #[tokio::test]
    async fn running_out_of_requests_with_pages_left_is_truncation() {
        let s = seed(&page(&["/a", "/b", "/c"]), "https://www.pinellasdsa.org/");
        let crawl = Fake::new([
            ("https://www.pinellasdsa.org/a", page(&[])),
            ("https://www.pinellasdsa.org/b", page(&[])),
            ("https://www.pinellasdsa.org/c", page(&[])),
        ])
        .with_budget(1);

        let got = Links.enumerate(&s, &crawl).await.unwrap();

        assert!(got.truncated);
        assert!(got.warnings.iter().any(|w| w.contains("1-request budget")));
        assert_eq!(
            got.addresses.len(),
            4,
            "known addresses are kept, read or not"
        );
    }

    #[tokio::test]
    async fn a_page_the_host_disallows_is_neither_fetched_nor_kept() {
        let s = crate::strategies::crawl::Seed {
            robots: crate::discovery::Robots::parse(
                "centinel",
                b"User-agent: *\nDisallow: /private\n",
            ),
            ..seed(
                &page(&["/private/x", "/open"]),
                "https://www.pinellasdsa.org/",
            )
        };
        let crawl = Fake::new([("https://www.pinellasdsa.org/open", page(&[]))]);

        let got = Links.enumerate(&s, &crawl).await.unwrap();

        assert_eq!(
            got.addresses,
            [
                "https://www.pinellasdsa.org/",
                "https://www.pinellasdsa.org/open"
            ]
        );
        assert_eq!(crawl.requests(), 1);
        assert_eq!(got.figures["disallowed"], 1);
    }
}
