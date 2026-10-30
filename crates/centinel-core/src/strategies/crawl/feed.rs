//! `feed` — an address that serves RSS, Atom or iCalendar.
//!
//! Keyed on a **standard**, like `sitemap`, and recognised from the bytes the seed served
//! rather than from its address: Medium serves its feed as `text/xml` at `/feed`, Google
//! Calendar serves `text/calendar` at `…/basic.ics`, and neither path is a promise.
//!
//! The enumeration is one address, the feed. What the feed holds is acquisition's
//! business: each entry is stored as its own artifact, cut from the response — see
//! [`crate::feed`]. So the count here is the feed, and the entries are reported where
//! they are stored.
//!
//! On a tie with `sitemap` — a feed seed on a host whose `robots.txt` declares a sitemap —
//! this one runs, by the registry's stable name order: the operator handed over the feed,
//! and the feed is what the seed is.

use futures::future::BoxFuture;

use super::{Enumerated, Pass, Seed, Strategy, StrategyDef, Walk};
use crate::domain::Note;
use crate::feed::Format;
use crate::strategies::{Keyed, Recognition};

pub struct Feed;

inventory::submit! { StrategyDef { name: "feed", it: &Feed } }

impl Strategy for Feed {
    fn name(&self) -> &'static str {
        "feed"
    }

    fn recognise(&self, seed: &Seed) -> Option<Recognition> {
        let format = Format::sniff(&seed.page.bytes)?;
        let standard = match format {
            Format::Rss => "RSS",
            Format::Atom => "Atom",
            Format::Calendar => "iCalendar",
        };
        let mut r = Recognition::new(self.name(), Keyed::Standard(standard))
            .seeing("feed", format!("the address serves {format}"));
        match crate::feed::entries(&seed.page.bytes) {
            Ok(split) => {
                r = r.seeing(
                    format!("{}s", split.format.noun()),
                    format!(
                        "{} in the feed",
                        crate::render::count(split.entries.len() as u64)
                    ),
                );
                if format.is_window() {
                    // Said at recognition, so it is carried into every run's notes: ten
                    // items is the latest ten, not the blog.
                    r = r.warning(
                        "window",
                        "a feed lists its latest entries; anything older is not in it",
                    );
                }
            }
            Err(e) => r = r.warning("feed", format!("does not split into entries: {e}")),
        }
        Some(r)
    }

    fn enumerate<'a>(
        &'a self,
        seed: &'a Seed,
        crawl: &'a dyn Walk,
    ) -> BoxFuture<'a, anyhow::Result<Enumerated>> {
        Box::pin(async move {
            let mut at = seed
                .final_url()
                .ok_or_else(|| anyhow::anyhow!("the seed does not record where it was served"))?;
            at.set_fragment(None);

            let mut pass = Pass::<String>::new(crawl, "feed", 0);
            if Format::sniff(&seed.page.bytes).is_some() {
                pass.read_surface();
                pass.note(Note::new("feed", at.to_string()));
            }
            pass.keep(at.to_string(), &seed.robots);
            Ok(pass.finish())
        })
    }

    fn first_address_is_the_seed(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::crawl::tests::{Fake, seed, seed_with_robots};

    const RSS: &str = r#"<?xml version="1.0"?><rss version="2.0"><channel><title>Blog</title><item><guid>1</guid><title>One</title></item></channel></rss>"#;

    #[tokio::test]
    async fn a_feed_seed_is_recognised_and_is_its_own_address() {
        let s = seed(RSS, "https://dsa-pinellas.medium.com/feed");
        let r = Feed.recognise(&s).expect("the seed is RSS");
        assert_eq!(r.keyed_on, Keyed::Standard("RSS"));
        assert!(r.warnings.iter().any(|n| n.label == "window"));

        let crawl = Fake::new([]);
        let got = Feed.enumerate(&s, &crawl).await.unwrap();
        assert_eq!(got.addresses, ["https://dsa-pinellas.medium.com/feed"]);
        assert!(!got.discovery_failed);
        assert_eq!(crawl.requests(), 0, "the feed was in hand");
    }

    /// The operator handed over the feed. A sitemap declared by the same host is true and
    /// is not what was asked for.
    #[test]
    fn a_feed_seed_outranks_a_sitemap_the_host_declares() {
        let s = crate::strategies::crawl::Seed {
            page: seed(RSS, "https://dsa-pinellas.medium.com/feed").page,
            ..seed_with_robots(
                "https://dsa-pinellas.medium.com/feed",
                "Sitemap: https://dsa-pinellas.medium.com/sitemap/sitemap.xml\n",
            )
        };
        let (best, _) = crate::strategies::crawl::best(&s).unwrap();
        assert_eq!(best.name, "feed");
    }

    #[test]
    fn a_page_is_not_a_feed() {
        let s = seed("<html><body>news</body></html>", "https://a.gov/feed");
        assert!(Feed.recognise(&s).is_none());
    }
}
