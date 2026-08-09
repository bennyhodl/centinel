//! `sitemap` — the standard, and the strategy every other one is measured against.
//!
//! A port rather than a new thing. The walk below is the old `Discoverer`'s, moved behind
//! [`Strategy`] so that the one hardcoded line in `SiteSource::enumerate`
//! becomes a choice. Its behaviour is deliberately unchanged, because it is the strategy
//! every existing source in the store was collected with.
//!
//! ## It recognises itself, and that matters more than it sounds
//!
//! `robots.txt` naming a `<sitemapindex>` is a real, evidenced recognition — Hillsborough
//! Clerk's system A in `docs/FIELD-NOTES.md` is exactly this, and it enumerates cleanly.
//!
//! Falling back to a sitemap walk because **nothing** recognised the seed is a different
//! event with the same result, and the store cannot currently tell them apart: both record
//! `method = "sitemap"`. Separating them is what makes a Lead possible, so this strategy
//! answers `None` unless it has read a declaration. A site with an undeclared
//! `/sitemap.xml` is still walked — the host falls back to this strategy — but it is
//! walked *as a fallback*, and recorded as one.
//!
//! ## One request cheaper than it used to be
//!
//! `Discoverer` fetched `robots.txt` itself. The [`Seed`] now carries it, because more
//! than one recogniser wants it, so the walk starts from rules that are already in hand.

use futures::future::BoxFuture;

use super::{Enumerated, Pass, Seed, Strategy, StrategyDef, Walk};
use crate::discovery::{SitemapDoc, sitemap as doc};
use crate::domain::Note;
use crate::strategies::{Keyed, Recognition};

pub struct Sitemap;

inventory::submit! { StrategyDef { name: "sitemap", it: &Sitemap } }

/// Index nesting depth. `index → index → urlset` is legal, so this is not 1.
///
/// Structural rather than configurable: it bounds the *shape* a document may have, where
/// [`Walk::budget`] bounds the work a run may do. Nothing has ever wanted to tune it.
const MAX_DEPTH: usize = 5;

/// A declared `Sitemap:` line, resolved against the host that served `robots.txt`.
///
/// The standard says the line carries an absolute URL. Real servers disagree, and
/// `buffalony.gov` is the one that cost a whole site: it declares `Sitemap: /sitemap.xml`,
/// a path, and that file is real — 200, `application/xml`, 579 addresses, every one on the
/// same host. Unresolved, the string reached `reqwest` as a path with no host, which cannot
/// become a request. 579 addresses became 0, under a run that reported one failed stage.
///
/// The same unresolved string then failed the cross-host test in [`Sitemap::recognise`],
/// because a path never begins with an origin — so the report also blamed another host for
/// a file sitting on this one, and pointed the operator away from the cause. One resolution
/// fixes both faults, which is why it happens here rather than at either use.
///
/// Resolved against the **root**, not the seed's own path: `robots.txt` is served from `/`,
/// so a reference inside it is relative to `/` and not to whatever page happened to be the
/// seed. `Url::join` returns an absolute input unchanged, so the ordinary case is untouched.
fn resolve(declared: &str, base: &url::Url) -> Option<url::Url> {
    base.join("/").ok()?.join(declared).ok()
}

impl Strategy for Sitemap {
    fn name(&self) -> &'static str {
        "sitemap"
    }

    fn recognise(&self, seed: &Seed) -> Option<Recognition> {
        let sitemaps = seed.robots.sitemaps();
        if sitemaps.is_empty() {
            // Either the host declared none, or robots.txt was unreachable and the rules
            // were assumed. Neither is a recognition, and `None` here is what lets the
            // difference between "recognised" and "fell back" survive into the store.
            return None;
        }

        let mut r = Recognition::new(self.name(), Keyed::Standard("sitemap.xml")).seeing(
            "robots.txt",
            format!(
                "declares {}",
                match sitemaps.len() {
                    1 => "one sitemap".to_string(),
                    n => format!("{n} sitemaps"),
                }
            ),
        );
        // Labelled `declared` rather than `sitemap`, because the walk below reports the
        // documents it actually *fetched* under that word and printing both under one
        // label reads as the same fact stated twice.
        if let Some(first) = sitemaps.first() {
            r = r.seeing("declared", first.clone());
        }
        // Routine, and the reason discovery is per-host rather than per-run: hcfl.gov's
        // sitemap is advertised by hillsboroughcounty.org. Tested on the *resolved* URL —
        // an unresolved `/sitemap.xml` never begins with an origin, so this used to report
        // every relative declaration as belonging to somebody else.
        if let Some(seed_url) = seed.final_url() {
            let origin = seed_url.origin().ascii_serialization();
            let elsewhere = sitemaps.iter().any(|s| match resolve(s, &seed_url) {
                Some(u) => u.origin().ascii_serialization() != origin,
                // Unresolvable is not off-host; it is worse, and it gets its own line
                // rather than being folded into a warning about somebody else's server.
                None => false,
            });
            if elsewhere {
                r = r.warning("sitemap", "at least one sitemap is on another host");
            }
            if sitemaps.iter().any(|s| resolve(s, &seed_url).is_none()) {
                r = r.warning(
                    "sitemap",
                    "at least one declared sitemap is not a usable URL",
                );
            }
        }
        Some(r)
    }

    fn enumerate<'a>(
        &'a self,
        seed: &'a Seed,
        crawl: &'a dyn Walk,
    ) -> BoxFuture<'a, anyhow::Result<Enumerated>> {
        Box::pin(async move {
            let base = seed
                .final_url()
                .ok_or_else(|| anyhow::anyhow!("the seed does not record where it was served"))?;
            let robots = &seed.robots;
            let mut pass = Pass::new(crawl, "surface", MAX_DEPTH);

            // Everything that would explain a wrong count, stated as provenance rather
            // than left for the reader to reconstruct from a bare number.
            pass.note(Note::ok_or_warn(
                "robots.txt",
                match robots.declared {
                    true => "read",
                    false => "unreachable — rules were assumed, not read",
                },
                robots.declared,
            ));
            if let Some(delay) = robots.crawl_delay() {
                pass.note(Note::new(
                    "crawl-delay",
                    format!("{}s declared by the host", delay.as_secs_f64()),
                ));
            }

            // Prefer what robots.txt declares; these routinely point at another host.
            // Resolved first — see `resolve`. A declaration that will not resolve is
            // dropped with a warning rather than queued, because queuing it spends a
            // request to learn what the URL parser already knew.
            let mut declared_any = false;
            for declared in robots.sitemaps() {
                match resolve(declared, &base) {
                    Some(url) => {
                        pass.push(url.to_string(), 0);
                        declared_any = true;
                    }
                    None => pass.warn(format!(
                        "robots.txt declares `{declared}`, which is not a usable address"
                    )),
                }
            }
            if !declared_any {
                let guess = base.join("/sitemap.xml")?;
                pass.warn(format!(
                    "robots.txt declared no sitemap; trying {guess} by convention"
                ));
                pass.push(guess.to_string(), 0);
            }

            while let Some((loc, depth)) = pass.next_to_visit() {
                pass.visiting(format!("sitemap {loc}"));

                let body = match pass.walk().get(&loc).await {
                    Ok(f) => f.bytes,
                    Err(refusal) => {
                        pass.refused(&loc, &refusal);
                        continue;
                    }
                };
                pass.note(Note::new("sitemap", loc.clone()));

                match doc::parse(&body) {
                    Ok(SitemapDoc::Index(refs)) => {
                        for r in refs {
                            pass.push(r.loc, depth + 1);
                        }
                    }
                    Ok(SitemapDoc::UrlSet(entries)) => {
                        for e in entries {
                            // `next` writes the warning and ends the walk; this only stops
                            // filling past the ceiling.
                            if pass.full() {
                                break;
                            }
                            pass.keep(e.loc, robots);
                        }
                    }
                    Err(e) => pass.warn(format!("{loc}: {e}")),
                }
            }

            let fetched = pass.visits();
            pass.figure("sitemaps_fetched", fetched);
            pass.figure("robots_declared", robots.declared as u64);

            Ok(pass.finish())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategies::crawl::tests::{Fake, seed_with_robots};

    const INDEX: &str = r#"<?xml version="1.0"?>
        <sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
          <sitemap><loc>https://a.gov/one.xml</loc></sitemap>
          <sitemap><loc>https://a.gov/two.xml</loc></sitemap>
        </sitemapindex>"#;

    fn urlset(locs: &[&str]) -> String {
        let body: String = locs
            .iter()
            .map(|l| format!("<url><loc>{l}</loc></url>"))
            .collect();
        format!(
            r#"<?xml version="1.0"?><urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">{body}</urlset>"#
        )
    }

    #[test]
    fn a_declared_sitemap_is_a_recognition_and_a_silent_one_is_not() {
        let declared = seed_with_robots("https://a.gov/", "Sitemap: https://a.gov/sitemap.xml\n");
        let r = Sitemap.recognise(&declared).expect("robots names one");
        assert_eq!(r.strategy, "sitemap");
        assert_eq!(r.keyed_on, Keyed::Standard("sitemap.xml"));

        // The site may still have a /sitemap.xml. We have not read that it does, and a
        // recogniser that answers anyway is the confident half-answer §7 is about.
        let silent = seed_with_robots("https://a.gov/", "User-agent: *\nDisallow:\n");
        assert!(Sitemap.recognise(&silent).is_none());
    }

    /// One sitemap advertised by another host is routine, not an error — but it is worth
    /// saying, because it is why pacing is per host rather than per run.
    #[test]
    fn a_sitemap_on_another_host_is_recognised_and_remarked_on() {
        let s = seed_with_robots(
            "https://hillsboroughcounty.org/",
            "Sitemap: https://hcfl.gov/sitemap.xml\n",
        );
        let r = Sitemap.recognise(&s).unwrap();
        assert!(r.warnings.iter().any(|n| n.detail.contains("another host")));
    }

    #[tokio::test]
    async fn an_index_is_walked_into_its_children_and_the_urls_come_back() {
        let s = seed_with_robots("https://a.gov/", "Sitemap: https://a.gov/all.xml\n");
        let crawl = Fake::new([
            ("https://a.gov/all.xml", INDEX.to_string()),
            ("https://a.gov/one.xml", urlset(&["https://a.gov/p1"])),
            ("https://a.gov/two.xml", urlset(&["https://a.gov/p2"])),
        ]);

        let got = Sitemap.enumerate(&s, &crawl).await.unwrap();
        let mut found = got.addresses.clone();
        found.sort();
        assert_eq!(found, ["https://a.gov/p1", "https://a.gov/p2"]);
        assert_eq!(got.figures["sitemaps_fetched"], 3);
    }

    /// A sitemap is attacker-or-accident controlled, and a self-referential index is a
    /// thing real sites ship.
    #[tokio::test]
    async fn a_sitemap_that_names_itself_is_fetched_once() {
        let s = seed_with_robots("https://a.gov/", "Sitemap: https://a.gov/loop.xml\n");
        let loop_doc = r#"<?xml version="1.0"?>
            <sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
              <sitemap><loc>https://a.gov/loop.xml</loc></sitemap>
            </sitemapindex>"#;
        let crawl = Fake::new([("https://a.gov/loop.xml", loop_doc.to_string())]);

        let got = Sitemap.enumerate(&s, &crawl).await.unwrap();
        assert!(got.addresses.is_empty());
        assert_eq!(crawl.requests(), 1, "the loop must not be re-entered");
    }

    /// The rule the whole design turns on: a truncated snapshot looks exactly like a
    /// source that shrank, so nothing may silently cap one.
    #[tokio::test]
    async fn running_out_of_budget_is_a_warning_and_not_a_short_answer() {
        let s = seed_with_robots("https://a.gov/", "Sitemap: https://a.gov/all.xml\n");
        let crawl = Fake::new([
            ("https://a.gov/all.xml", INDEX.to_string()),
            ("https://a.gov/one.xml", urlset(&["https://a.gov/p1"])),
            ("https://a.gov/two.xml", urlset(&["https://a.gov/p2"])),
        ])
        .with_budget(2);

        let got = Sitemap.enumerate(&s, &crawl).await.unwrap();
        assert!(got.addresses.len() < 2, "the walk was cut short");
        assert!(got.truncated, "and the flag says so, not only the prose");
        assert!(
            got.warnings.iter().any(|w| w.contains("budget")),
            "and it said so: {:?}",
            got.warnings
        );
    }

    /// `dunedin.gov`: one sitemap holding more URLs than the cap allows. The walk fills
    /// past its ceiling on its only pass, the queue empties, and it exits by the normal
    /// route — so the ceiling was never reported and `investigate` printed a checkmark
    /// beside 500 addresses against a real 1,625.
    #[tokio::test]
    async fn a_single_sitemap_larger_than_the_cap_still_reports_the_cap() {
        let s = seed_with_robots(
            "https://dunedin.test/",
            "Sitemap: https://dunedin.test/s.xml\n",
        );
        let many: Vec<String> = (0..8)
            .map(|i| format!("https://dunedin.test/p{i}"))
            .collect();
        let refs: Vec<&str> = many.iter().map(String::as_str).collect();
        let crawl =
            Fake::new([("https://dunedin.test/s.xml", urlset(&refs))]).with_max_addresses(3);

        let got = Sitemap.enumerate(&s, &crawl).await.unwrap();
        assert_eq!(got.addresses.len(), 3, "it stopped at the ceiling");
        assert!(got.truncated, "and it did not call that a total");
        assert!(
            got.warnings.iter().any(|w| w.contains("stopped at 3")),
            "{:?}",
            got.warnings
        );
    }

    /// The other side of the same flag, and the one that keeps it honest: an ordinary walk
    /// that ends because it ran out of sitemap is complete, and must not warn.
    #[tokio::test]
    async fn a_walk_that_finishes_under_its_ceiling_is_not_truncated() {
        let s = seed_with_robots("https://a.gov/", "Sitemap: https://a.gov/all.xml\n");
        let crawl = Fake::new([(
            "https://a.gov/all.xml",
            urlset(&["https://a.gov/p1", "https://a.gov/p2"]),
        )]);

        let got = Sitemap.enumerate(&s, &crawl).await.unwrap();
        assert_eq!(got.addresses.len(), 2);
        assert!(!got.truncated);
        assert!(got.warnings.is_empty(), "{:?}", got.warnings);
    }

    #[tokio::test]
    async fn an_address_the_host_disallows_is_counted_rather_than_collected() {
        let s = seed_with_robots(
            "https://a.gov/",
            "Sitemap: https://a.gov/all.xml\nUser-agent: *\nDisallow: /private/\n",
        );
        let crawl = Fake::new([(
            "https://a.gov/all.xml",
            urlset(&["https://a.gov/ok", "https://a.gov/private/x"]),
        )]);

        let got = Sitemap.enumerate(&s, &crawl).await.unwrap();
        assert_eq!(got.addresses, ["https://a.gov/ok"]);
        assert_eq!(got.figures["disallowed"], 1);
    }

    /// A sitemap that refuses is a warning, not a failed pass — a partial enumeration
    /// with its caveats recorded beats a hard failure.
    #[tokio::test]
    async fn one_refused_sitemap_does_not_cancel_the_others() {
        let s = seed_with_robots("https://a.gov/", "Sitemap: https://a.gov/all.xml\n");
        let crawl = Fake::new([
            ("https://a.gov/all.xml", INDEX.to_string()),
            ("https://a.gov/two.xml", urlset(&["https://a.gov/p2"])),
        ]);

        let got = Sitemap.enumerate(&s, &crawl).await.unwrap();
        assert_eq!(got.addresses, ["https://a.gov/p2"]);
        assert!(got.warnings.iter().any(|w| w.contains("one.xml")));
    }

    /// `buffalony.gov`, which declares `Sitemap: /sitemap.xml` and lost all 579 of its
    /// addresses to it. The path is legal enough that a real server ships it and a real
    /// browser follows it; unresolved it is not a URL at all.
    #[tokio::test]
    async fn a_sitemap_declared_as_a_path_is_resolved_against_the_host() {
        let s = seed_with_robots("https://buffalony.test/", "Sitemap: /sitemap.xml\n");
        let crawl = Fake::new([(
            "https://buffalony.test/sitemap.xml",
            urlset(&["https://buffalony.test/1/Home"]),
        )]);

        let got = Sitemap.enumerate(&s, &crawl).await.unwrap();
        assert_eq!(got.addresses, ["https://buffalony.test/1/Home"]);
    }

    /// The second half of the same defect, and the one that misdirected the operator: a
    /// path never begins with an origin, so the cross-host test called this a foreign
    /// sitemap while it sat on the seed's own host.
    #[test]
    fn a_path_is_not_reported_as_belonging_to_another_host() {
        let s = seed_with_robots("https://buffalony.test/", "Sitemap: /sitemap.xml\n");
        let r = Sitemap.recognise(&s).expect("robots names one");
        assert!(
            !r.warnings.iter().any(|n| n.detail.contains("another host")),
            "{:?}",
            r.warnings
        );
    }

    /// A relative reference resolves against `/`, because that is where `robots.txt` is
    /// served from — not against whatever page happened to be handed in as the seed.
    #[tokio::test]
    async fn a_relative_sitemap_resolves_against_the_root_and_not_the_seed_path() {
        let s = seed_with_robots("https://a.gov/departments/clerk/", "Sitemap: sitemap.xml\n");
        let crawl = Fake::new([("https://a.gov/sitemap.xml", urlset(&["https://a.gov/p1"]))]);

        let got = Sitemap.enumerate(&s, &crawl).await.unwrap();
        assert_eq!(got.addresses, ["https://a.gov/p1"]);
    }

    /// An absolute declaration is the ordinary case and must be untouched by any of this.
    #[tokio::test]
    async fn an_absolute_declaration_is_left_exactly_as_it_was_given() {
        let s = seed_with_robots(
            "https://hillsboroughcounty.test/",
            "Sitemap: https://hcfl.test/sitemap.xml\n",
        );
        let crawl = Fake::new([(
            "https://hcfl.test/sitemap.xml",
            urlset(&["https://hcfl.test/p"]),
        )]);

        let got = Sitemap.enumerate(&s, &crawl).await.unwrap();
        assert_eq!(got.addresses, ["https://hcfl.test/p"]);
        assert!(
            Sitemap
                .recognise(&s)
                .unwrap()
                .warnings
                .iter()
                .any(|n| n.detail.contains("another host")),
            "a genuinely foreign sitemap is still remarked on"
        );
    }

    /// Dropped before it costs a request, and named. Queuing it would spend a fetch to
    /// learn what the URL parser already knew.
    ///
    /// The input is a space inside the host, which is what a hand-edited `robots.txt`
    /// produces. It matters that the example is of *that* kind: almost any other garbage —
    /// `://nonsense` included — is a legal relative reference, so it resolves to a
    /// same-host address and costs one 404 rather than reaching this branch. Only a
    /// malformed authority fails to resolve at all.
    #[tokio::test]
    async fn a_declaration_that_cannot_be_a_url_is_named_and_the_convention_still_runs() {
        let s = seed_with_robots("https://a.gov/", "Sitemap: https://exa mple.gov/s.xml\n");
        let crawl = Fake::new([("https://a.gov/sitemap.xml", urlset(&["https://a.gov/p1"]))]);

        let got = Sitemap.enumerate(&s, &crawl).await.unwrap();
        assert!(
            got.warnings
                .iter()
                .any(|w| w.contains("not a usable address")),
            "the bad declaration is named: {:?}",
            got.warnings
        );
        // Dropping it empties the queue, so the walk reaches the convention it would have
        // used had `robots.txt` declared nothing at all. That is the useful outcome: a
        // typo in one line does not cost the site.
        assert_eq!(got.addresses, ["https://a.gov/p1"]);
        assert_eq!(
            crawl.requests(),
            1,
            "and only the conventional path was tried"
        );
    }
}
