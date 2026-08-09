//! `sitemap` — the standard, and the strategy every other one is measured against.
//!
//! A port rather than a new thing. The walk below is [`crate::discovery::Discoverer`]'s,
//! moved behind [`Strategy`] so that the one hardcoded line in `SiteSource::enumerate`
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

use std::collections::HashSet;

use futures::future::BoxFuture;

use super::{Enumerated, Seed, Strategy, StrategyDef, Walk};
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
        // sitemap is advertised by hillsboroughcounty.org.
        if let Some(origin) = seed.final_url().map(|u| u.origin().ascii_serialization())
            && sitemaps.iter().any(|s| !s.starts_with(&origin))
        {
            r = r.warning("sitemap", "at least one sitemap is on another host");
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
            let mut out = Enumerated::default();

            // Everything that would explain a wrong count, stated as provenance rather
            // than left for the reader to reconstruct from a bare number.
            out.notes.push(Note::ok_or_warn(
                "robots.txt",
                match robots.declared {
                    true => "read",
                    false => "unreachable — rules were assumed, not read",
                },
                robots.declared,
            ));
            if let Some(delay) = robots.crawl_delay() {
                out.notes.push(Note::new(
                    "crawl-delay",
                    format!("{}s declared by the host", delay.as_secs_f64()),
                ));
            }

            // Prefer what robots.txt declares; these routinely point at another host.
            let mut queue: Vec<(String, usize)> =
                robots.sitemaps().iter().map(|s| (s.clone(), 0)).collect();
            if queue.is_empty() {
                let guess = base.join("/sitemap.xml")?;
                out.warnings.push(format!(
                    "robots.txt declared no sitemap; trying {guess} by convention"
                ));
                queue.push((guess.to_string(), 0));
            }

            let mut visited: HashSet<String> = HashSet::new();
            let mut seen: HashSet<String> = HashSet::new();
            let mut fetched = 0usize;
            let mut disallowed = 0u64;

            while let Some((loc, depth)) = queue.pop() {
                if !crawl.may_fetch() {
                    out.warnings.push(format!(
                        "stopped at the {}-request budget; the surface is larger than \
                         this run captured",
                        crawl.budget()
                    ));
                    break;
                }
                // Tested here rather than only where addresses are pushed, so a full run
                // stops fetching instead of walking the remaining index to throw every
                // urlset away — and so the warning is written once rather than once per
                // document that arrived after the cap.
                if out.addresses.len() >= crawl.max_addresses() {
                    out.warnings.push(format!(
                        "stopped at {} addresses; the surface is larger than this run \
                         captured",
                        crawl.max_addresses()
                    ));
                    break;
                }
                // Loop protection. Self-referential indexes exist in the wild.
                if !visited.insert(loc.clone()) {
                    continue;
                }
                if depth > MAX_DEPTH {
                    out.warnings
                        .push(format!("depth limit reached, skipping {loc}"));
                    continue;
                }

                crawl.progress().step(
                    format!("sitemap {loc}"),
                    fetched as u64,
                    (fetched + queue.len() + 1) as u64,
                );

                let body = match crawl.get(&loc).await {
                    Ok(f) => f.bytes,
                    Err(refusal) => {
                        out.warnings.push(format!("{loc}: {refusal}"));
                        continue;
                    }
                };
                fetched += 1;
                out.notes.push(Note::new("sitemap", loc.clone()));

                match doc::parse(&body) {
                    Ok(SitemapDoc::Index(refs)) => {
                        for r in refs {
                            queue.push((r.loc, depth + 1));
                        }
                    }
                    Ok(SitemapDoc::UrlSet(entries)) => {
                        for e in entries {
                            // The loop above writes the warning and ends the walk.
                            if out.addresses.len() >= crawl.max_addresses() {
                                break;
                            }
                            if !robots.allowed(&e.loc) {
                                disallowed += 1;
                                continue;
                            }
                            // Dedup on the full URL *including* query string — stripping
                            // it would collapse distinct .gov agenda pages into one.
                            if seen.insert(e.loc.clone()) {
                                out.addresses.push(e.loc);
                            }
                        }
                    }
                    Err(e) => out.warnings.push(format!("{loc}: {e}")),
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
            out.figures.insert("disallowed".into(), disallowed);
            out.figures
                .insert("sitemaps_fetched".into(), fetched as u64);
            out.figures
                .insert("robots_declared".into(), robots.declared as u64);

            Ok(out)
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
        assert!(
            got.warnings.iter().any(|w| w.contains("budget")),
            "and it said so: {:?}",
            got.warnings
        );
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
}
