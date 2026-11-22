//! `opengov-stories` — an OpenGov Stories book: every published page its table of contents
//! names.
//!
//! Keyed on a **product**, at two sightings: Tampa's budget books, FY2024 to FY2027, and
//! Pasco County's FY2026 budget book and FY2026–2030 capital plan (`docs/FIELD-NOTES.md`
//! entries 1 and 19). Some are published nowhere else: Tampa posts no adopted budget book as
//! a PDF after FY2017, and Pasco no PDF of that capital plan.
//!
//! ## Where the pages are
//!
//! A book's table of contents is served at
//! `stories.opengov.com/{client}/{uuid}/published/{slug}?currentPageId=TOC`. The address bar
//! lies: the server ignores `currentPageId` and returns the same bytes for every page, and the
//! browser swaps the body in. Each entry is an anchor whose `href` is that client-side route
//! and whose **`data-public-url`** is the page's own address:
//!
//! ```html
//! <a href="?currentPageId=682f6a10865e7e951c1f9156"
//!    class="toc-story-link js-toc-story-link"
//!    data-id="682f6a10865e7e951c1f9156"
//!    data-public-url="https://stories.opengov.com/pascocountyfl/published/2sR05GPYMy">
//! ```
//!
//! Those public pages are server-rendered HTML with the page's text in them, and nothing
//! else lists them: no `robots.txt`, no sitemap, no client index (`/tampa` is a 404), and a
//! public page links at most a neighbour or two. So the walk is the seed: every
//! `data-public-url` on it, in order, and no request.
//!
//! ## The seed
//!
//! The contents page is the only address that holds the set, and it reads "Preview link
//! expired" — it is the editor's preview, which the city links from its own budget page. It
//! still lists every page, on 2026-08-07 at Tampa and again on 2026-11-22 at Tampa and
//! Pasco, so the recognition says what it is rather than refusing it. A book is one source;
//! each year's book is a different contents page.
//!
//! ## What it does not collect
//!
//! The figures. A page draws its charts in iframes from `{client}.opengov.com/transparency`,
//! behind a `#` fragment and a script, so the narrative is collected and the tables are not.

use futures::future::BoxFuture;
use url::Url;

use super::{Enumerated, Pass, Seed, Strategy, StrategyDef, Walk};
use crate::domain::Note;
use crate::strategies::{Keyed, Recognition};

pub struct OpenGovStories;

inventory::submit! { StrategyDef { name: "opengov-stories", it: &OpenGovStories } }

/// The product, as a recognition names it.
const PRODUCT: &str = "OpenGov Stories";

/// The class every contents entry carries, and nothing else on the page.
const ENTRY: &str = "toc-story-link";

/// Where an entry keeps the page's address.
const PUBLIC_URL: &str = "data-public-url";

/// What the preview page says about itself.
const EXPIRED: &str = "Preview link expired";

/// The walk reads nothing beyond the seed.
const MAX_DEPTH: usize = 0;

impl Strategy for OpenGovStories {
    fn name(&self) -> &'static str {
        "opengov-stories"
    }

    fn recognise(&self, seed: &Seed) -> Option<Recognition> {
        let at = seed.final_url()?;
        let page = seed.text();
        let pages = pages_in(&page, &at);
        // A public page names no contents, and neither does anything else: nothing here
        // could be walked.
        if pages.is_empty() {
            return None;
        }
        let mut r = Recognition::new(self.name(), Keyed::Product(PRODUCT)).seeing(
            "contents",
            format!("{} pages named by `{PUBLIC_URL}`", pages.len()),
        );
        if let Some(title) = crate::html::Scan::new(&page).title() {
            r = r.seeing("book", title);
        }
        if page.contains(EXPIRED) {
            r = r.warning(
                "preview",
                format!(
                    "this contents page is the editor's preview and says \"{EXPIRED}\"; it \
                     still names every page, and is the only address that does"
                ),
            );
        }
        Some(r)
    }

    fn enumerate<'a>(
        &'a self,
        seed: &'a Seed,
        crawl: &'a dyn Walk,
    ) -> BoxFuture<'a, anyhow::Result<Enumerated>> {
        Box::pin(async move {
            let at = seed
                .final_url()
                .ok_or_else(|| anyhow::anyhow!("the seed does not record where it was served"))?;
            let mut pass = Pass::<&str>::new(crawl, "book", MAX_DEPTH);
            pass.note(Note::new("contents", at.to_string()));
            let pages = pages_in(&seed.text(), &at);
            if !pages.is_empty() {
                pass.read_surface();
            }
            pass.figure("pages", pages.len() as u64);
            for page in pages {
                if pass.full() {
                    break;
                }
                pass.keep(page, &seed.robots);
            }
            Ok(pass.finish())
        })
    }
}

/// Every page a contents page names, in its order, on the contents page's own host.
///
/// Read from the anchors' `data-public-url`, never their `href`, which is the client-side
/// route every entry shares the server's answer for.
fn pages_in(page: &str, at: &Url) -> Vec<String> {
    let scan = crate::html::Scan::new(page);
    let base = scan.base(at);
    scan.tags(&["a"])
        .into_iter()
        .filter(|tag| {
            tag.attr("class")
                .is_some_and(|class| class.split_whitespace().any(|c| c == ENTRY))
        })
        .filter_map(|tag| base.join(tag.attr(PUBLIC_URL)?))
        .filter(|u| u.host_str() == at.host_str())
        .map(String::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::crawl::tests::{Fake, seed};

    const CONTENTS: &str = "https://stories.opengov.com/pascocountyfl/e8874262-2d00-4795-9735-967ec5b0d93b/published/eBgL6jMBk?currentPageId=TOC";

    /// Pasco County's FY 2026 budget book contents, as served on 2026-11-22, cut to its first
    /// section's three entries, a repeat of one, the preview notice, and the footer's one
    /// link that is not an entry.
    const PASCO_TOC: &str = r#"<!DOCTYPE html><html><head><title>Budget Book FY 2026</title></head><body>
  <p class="error-page-title">Preview link expired</p>
  <p class="error-page-text">Click Preview button on the Setup Online Publication page to generate a new Preview link</p>
</div>
<div class="page toc-page js-page " id="page_TOC" data-id="TOC">
<div class="toc-body">
  <h3 class="toc-page-title">Table of Contents</h3>
  <div class="toc-columns-container"><div class="toc-column">
    <h4 class="toc-section-title">Section 1 - Introduction &amp; Overview</h4>
    <a
      href="?currentPageId=682f6a10865e7e951c1f9156"
      class="toc-story-link js-toc-story-link"
      data-id="682f6a10865e7e951c1f9156"
      data-public-url="https://stories.opengov.com/pascocountyfl/published/2sR05GPYMy"
    >
      Introduction &amp; Overview
    </a>
    <a
      href="?currentPageId=682f6a107558485a5598beda"
      class="toc-story-link js-toc-story-link"
      data-id="682f6a107558485a5598beda"
      data-public-url="https://stories.opengov.com/pascocountyfl/published/KTfvuGoNCL"
    >
      County Administrator's Transmittal Letter
    </a>
    <a
      href="?currentPageId=68f8da2462845b7a0027d720"
      class="toc-story-link js-toc-story-link"
      data-id="68f8da2462845b7a0027d720"
      data-public-url="https://stories.opengov.com/pascocountyfl/published/2J1ppebf7"
    >
      Budget Highlights
    </a>
    <a
      href="?currentPageId=682f6a10865e7e951c1f9156"
      class="toc-story-link js-toc-story-link"
      data-id="682f6a10865e7e951c1f9156"
      data-public-url="https://stories.opengov.com/pascocountyfl/published/2sR05GPYMy"
    >
      Introduction &amp; Overview
    </a>
  </div></div>
</div></div>
<a href="https://opengov.com" target="_blank" rel="noopener noreferrer">OpenGov</a>
</body></html>"#;

    /// Pasco's "Introduction & Overview", a public page, cut: it names its neighbours in
    /// plain links and no contents.
    const PASCO_PAGE: &str = r#"<!DOCTYPE html><html><head><title>Introduction &amp; Overview</title></head><body>
<h1>Fiscal Year 2026 Adopted Budget</h1><p>The vision for Pasco County has been developed by our leaders with respect for the lessons learned from our history.</p>
<a href="https://stories.opengov.com/pascocountyfl/published/KTfvuGoNCL">Next</a>
</body></html>"#;

    /// The contents page is the walk: each page its entries name, once, in order, by the
    /// address in `data-public-url` and not the client-side route in `href`. No request.
    #[tokio::test]
    async fn the_contents_page_names_every_page_and_costs_nothing() {
        let s = seed(PASCO_TOC, CONTENTS);
        let crawl = Fake::new([]);

        let got = OpenGovStories.enumerate(&s, &crawl).await.unwrap();

        assert_eq!(
            got.addresses,
            [
                "https://stories.opengov.com/pascocountyfl/published/2sR05GPYMy",
                "https://stories.opengov.com/pascocountyfl/published/KTfvuGoNCL",
                "https://stories.opengov.com/pascocountyfl/published/2J1ppebf7",
            ]
        );
        assert_eq!(crawl.requests(), 0);
        assert_eq!(
            got.figures["pages"], 4,
            "entries as named, repeats included"
        );
        assert!(
            !got.truncated && !got.discovery_failed && got.warnings.is_empty(),
            "{:?}",
            got.warnings
        );
    }

    /// Recognised on a contents page, with the preview said up front. A public page of the
    /// same book names no contents, so it is not recognised: walking it would collect one
    /// page and call the book done.
    #[test]
    fn a_contents_page_is_recognised_and_a_public_page_is_not() {
        let r = OpenGovStories
            .recognise(&seed(PASCO_TOC, CONTENTS))
            .expect("a contents page");
        assert_eq!(r.keyed_on, Keyed::Product(PRODUCT));
        assert!(
            r.evidence
                .iter()
                .any(|n| n.label == "book" && n.detail == "Budget Book FY 2026"),
            "{:?}",
            r.evidence
        );
        assert!(r.warnings.iter().any(|n| n.label == "preview"));

        assert!(
            OpenGovStories
                .recognise(&seed(
                    PASCO_PAGE,
                    "https://stories.opengov.com/pascocountyfl/published/2sR05GPYMy"
                ))
                .is_none()
        );
    }

    /// The address cap stops the walk and says so.
    #[tokio::test]
    async fn a_book_larger_than_the_cap_is_marked_truncated() {
        let s = seed(PASCO_TOC, CONTENTS);
        let crawl = Fake::new([]).with_max_addresses(2);

        let got = OpenGovStories.enumerate(&s, &crawl).await.unwrap();

        assert_eq!(got.addresses.len(), 2);
        assert!(got.truncated, "{:?}", got.warnings);
    }
}
