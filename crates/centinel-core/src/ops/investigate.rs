//! `investigate` — who recognises this address, and what would it collect?
//!
//! The registry's front door. Give it a link and it answers the question that comes before
//! `source add`: *is there a corpus behind this, and does anything here know how to reach
//! it?* Nothing is stored, logged or written.
//!
//! ## Why it is not `check --strategy`
//!
//! They ask different questions and the difference is the number of requests.
//!
//! `check` asks *what does the pipeline make of **this document***, and answers it in
//! depth: the bytes as served, the reader that spoke, the text on disk to read. One
//! address, fully examined.
//!
//! `investigate` asks *should I add **this site***, and answers it in three to five
//! requests: the seed, `robots.txt`, and a capped probe of whatever the winning strategy
//! would enumerate. The count it prints is a **probe** and says so on the line — a number
//! that reads like a total, taken from a walk that stopped early, is the same lie a silent
//! cap tells.
//!
//! ## It writes nothing, and that is a property worth keeping
//!
//! There is no `--add`. Promotion is `centinel source add`, which already exists, and the
//! line to run is printed ready to paste. Keeping the two apart is what makes this command
//! safe to point at an address nobody has vetted: reading the evidence and acting on it
//! stay separate decisions, and the second one is always yours.
//!
//! ## The three answers
//!
//! `docs/STRATEGIES.md` §7 names them, and two are built:
//!
//! 1. **A strategy, with its evidence.** The operator accepts or rejects on what was seen.
//! 2. **Nothing, said plainly** — with the measurements that say whether the silence is
//!    ordinary or a **Lead** worth writing a strategy for.
//! 3. *A refusal* — a query box recognised and declined. Not built: it needs the `none`
//!    strategy, and recognising "a search form with nothing behind it" without also
//!    refusing every CMS that has a search box is a harder problem than it looks. One
//!    sighting, so it waits.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::content::ContentKind;
use crate::discovery::DiscoveryLimits;
use crate::policy::{DEFAULT_USER_AGENT, HostPolicy};
use crate::prelude::*;
use crate::sources::SiteSource;
use crate::strategies::crawl::{self, Seed};
use crate::verdict::Verdict;

/// Requests the size probe may spend, and addresses it will keep.
///
/// Deliberately small. This command exists to be run on ten hosts in a row while deciding
/// which are worth collecting, and a walk that takes minutes is one nobody runs twice.
const PROBE_REQUESTS: usize = 25;
const PROBE_ADDRESSES: usize = 500;

/// Addresses shown before the list is cut off.
const SAMPLE: usize = 5;

/// Off-host hosts named before the list is cut off.
const MAX_CRUMBS: usize = 8;

#[derive(Clone, Debug, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct InvestigateArgs {
    /// The address to investigate. Any URL on the site.
    ///
    /// **Keep the path.** A strategy that walks a directory bounds itself by the one it
    /// was pointed at, so `/Civil/` investigates `/Civil/` and the bare host investigates
    /// everything.
    #[arg(value_name = "URL")]
    pub target: String,

    /// Skip the size probe. Recognition only — two requests, and no walk at all.
    #[arg(long)]
    #[serde(default)]
    pub no_probe: bool,

    /// User-Agent header. A descriptive one measurably reduces WAF 403s.
    #[arg(long, default_value = DEFAULT_USER_AGENT)]
    #[serde(default = "default_ua")]
    pub user_agent: String,

    /// Per-request timeout in seconds.
    #[arg(long, default_value_t = 30)]
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

fn default_ua() -> String {
    DEFAULT_USER_AGENT.to_string()
}
fn default_timeout() -> u64 {
    30
}

/// The address as served, which is what every recogniser keys on.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SeedSummary {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_status: Option<String>,
    pub bytes: usize,
    pub kind: String,
    /// Where the bytes really came from, when a redirect moved it. *The visible URL is not
    /// the fetchable one* — two sightings in the field notes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_url: Option<String>,
    /// True when `robots.txt` was read rather than assumed.
    pub robots_declared: bool,
}

/// One strategy that answered, and why.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Recognised {
    pub strategy: String,
    /// `Hyland OnBase Agenda Online`.
    pub keyed_on: String,
    /// `product`, `framework`, `server default`, `standard`.
    pub kind: String,
    /// The one that would run. More specific beats less specific, so a product outranks a
    /// standard every server can satisfy.
    pub best: bool,
    pub evidence: Vec<Note>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<Note>,
}

/// Roughly how much is behind the address.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Probe {
    pub addresses: usize,
    /// False when the probe hit its own ceiling, which means the real number is larger and
    /// unknown. Never presented as a total.
    pub complete: bool,
    pub requests_allowed: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sample: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub figures: BTreeMap<String, u64>,
    /// What the source says it holds, where it says anything.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_total: Option<u64>,
}

/// Why nothing recognised this, in numbers.
///
/// `docs/STRATEGIES.md` §17. Most `.gov` sites are collected correctly by the sitemap
/// fallback, so "nothing recognised it" on its own is noise at exactly the scale that
/// makes it useless. What separates a Lead from an ordinary site is a **bad measurement**,
/// and each of these is taken from the field-note entry that proves it works.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Lead {
    /// What the reader actually got out of the seed — entry 2's 94,125 bytes in and 695
    /// characters out, and the menu case that has no site shape at all.
    pub read: Verdict,
    /// Entries 1 and 2: the address set is on the page and not in a link.
    pub anchors: usize,
    pub script_bytes: usize,
    /// Entry 1: no sitemap, so there is no declared surface to walk.
    pub sitemap_declared: bool,
    /// Findings about the site's *shape*. What is wrong with the **read** lives in
    /// [`Self::read`], because the two are independent: `hillsclerk.com` has an ordinary
    /// shape and a ruined read, and only separating them makes that sayable.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<String>,
}

impl Lead {
    /// Whether this is worth a person's attention, rather than an ordinary site the
    /// sitemap fallback will collect perfectly well.
    pub fn is_lead(&self) -> bool {
        !self.findings.is_empty() || self.read.is_poor()
    }
}

/// An off-host link, recorded and **not followed**.
///
/// One Source per exact host is the rule the field notes arrived at — a domain is not a
/// Source — and following these automatically is how one address becomes a crawl of the
/// internet. So they are counted, named, and left for the operator to promote.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Crumb {
    pub host: String,
    pub links: usize,
    /// One of them, to look at.
    pub example: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct InvestigateReport {
    pub address: String,
    pub seed: SeedSummary,
    /// Everything that recognised it, most specific first. Empty is a real answer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recognised: Vec<Recognised>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probe: Option<Probe>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead: Option<Lead>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub crumbs: Vec<Crumb>,
    /// The command that would add this, ready to paste. Absent when nothing recognised it,
    /// because suggesting it would be suggesting a corpus of one front page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub promote: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    pub elapsed_secs: f64,
}

/// Ask the registry what it makes of an address. Nothing is stored.
///
/// Fetches the address and its `robots.txt`, runs every recogniser in the build, and — for
/// whichever answered most specifically — walks a capped sample to say roughly how much is
/// there. Prints the `source add` line ready to paste, and writes nothing itself.
///
/// `reach = "operator"` because it makes outbound requests to an address the caller
/// chooses. That is a read-only act locally and a server-side request forgery primitive if
/// it were exposed over HTTP or MCP, so it stays on the CLI and the scheduler.
#[op(long_running, reach = "operator", group = "corpus")]
pub async fn investigate(
    _ctx: &Ctx,
    args: InvestigateArgs,
    progress: &Progress,
    cancel: &Cancel,
) -> anyhow::Result<InvestigateReport> {
    let started = std::time::Instant::now();

    let url = url::Url::parse(&args.target)
        .ok()
        .filter(|u| matches!(u.scheme(), "http" | "https"))
        .ok_or_else(|| anyhow::anyhow!("`{}` is not an http(s) address", args.target))?;

    let site = SiteSource::new(
        SourceId::new("investigate".to_string())?,
        url.as_str(),
        HostPolicy {
            user_agent: args.user_agent.clone(),
            timeout: std::time::Duration::from_secs(args.timeout_secs),
            ..Default::default()
        },
        DiscoveryLimits {
            max_sitemaps: PROBE_REQUESTS,
            max_urls: PROBE_ADDRESSES,
        },
    )?;

    let (seed, mut warnings) = site.seed(progress).await?;
    cancel.check()?;

    // Every recogniser, not the first to answer. Recognition is pure over bytes already in
    // hand, so asking all of them costs nothing — and what the runners-up saw is evidence
    // worth showing when the choice between two of them matters.
    let hits = crawl::recognise(&seed);
    let recognised: Vec<Recognised> = hits
        .iter()
        .enumerate()
        .map(|(i, r)| Recognised {
            strategy: r.strategy.to_string(),
            keyed_on: r.keyed_on.name().to_string(),
            kind: r.keyed_on.kind().to_string(),
            best: i == 0,
            evidence: r.evidence.clone(),
            warnings: r.warnings.clone(),
        })
        .collect();

    let probe = match (hits.first(), args.no_probe) {
        (Some(best), false) => {
            let def = crawl::by_name(best.strategy)?;
            let found = site.run(def, &seed, progress).await?;
            // The walk's own account of stopping early. A probe that ran out is the one
            // case where a count must never be read as a total.
            let complete = found.warnings.iter().all(|w| !w.contains("stopped at"));
            warnings.extend(found.warnings);
            Some(Probe {
                addresses: found.addresses.len(),
                complete,
                requests_allowed: PROBE_REQUESTS,
                sample: found.addresses.iter().take(SAMPLE).cloned().collect(),
                figures: found.figures,
                declared_total: found.declared_total,
            })
        }
        _ => None,
    };

    // Always, and the correction matters. This was once gated on `hits.is_empty()`, on
    // the reasoning that a recognised site needs no explaining. `hillsclerk.com` is
    // recognised by `sitemap`, enumerates cleanly, and puts 23,213 characters of
    // navigation into the corpus for a page whose content is one sentence — so the gate
    // made the tool structurally unable to report the only thing wrong with it.
    // Recognition says how to find the pages. It says nothing about reading them.
    //
    // What recognition *does* say is whether the seed resembles what will be collected. A
    // strategy naming documents was pointed at an index, and an index is a page of links
    // on purpose — `publicrec.hillsclerk.com/Civil/` reads as 62% link text and is working
    // perfectly. Its text is not the corpus; the files it lists are.
    let seed_is_collected = hits
        .first()
        .and_then(|r| crawl::by_name(r.strategy).ok())
        .is_none_or(|def| !matches!(def.it.addresses_are(), crawl::Addresses::Documents));
    let lead = Some(measure(&seed, seed_is_collected).await);

    let promote = hits
        .first()
        .map(|_| format!("centinel source add {} --site {url}", suggest_id(&url)));

    Ok(InvestigateReport {
        address: args.target,
        seed: SeedSummary {
            http_status: seed.page.meta.get("http_status").cloned(),
            bytes: seed.page.bytes.len(),
            kind: ContentKind::classify(&seed.page.meta, &seed.page.bytes).to_string(),
            final_url: seed
                .page
                .meta
                .get("final_url")
                .filter(|u| *u != &url.to_string())
                .cloned(),
            robots_declared: seed.robots.declared,
        },
        recognised,
        probe,
        lead,
        crumbs: crumbs_on(&seed),
        promote,
        warnings,
        elapsed_secs: started.elapsed().as_secs_f64(),
    })
}

/// The five measurements §17 names, taken on the seed.
///
/// Runs the **real** extractor for the character count, for the same reason `check` does:
/// a number produced by a second, simpler reader would answer a question nobody asked.
async fn measure(seed: &Seed, seed_is_collected: bool) -> Lead {
    let kind = ContentKind::classify(&seed.page.meta, &seed.page.bytes);
    let mut read = Verdict::on(&seed.page.bytes, &extracted_text(kind, seed).await);
    // The numbers stay; only the judgement is withdrawn. Nothing is gained by telling an
    // operator that the directory index they pointed at is a list of links.
    if !seed_is_collected {
        read.findings.clear();
    }

    let html = seed.text();
    let scan = crate::html::Scan::new(&html);
    let anchors = scan.tags(&["a"]).len();
    let script_bytes: usize = scan.scripts().iter().map(|s| s.len()).sum();
    let sitemap_declared = !seed.robots.sitemaps().is_empty();

    let mut findings = Vec::new();
    // The shape entries 1 and 2 share: a source declares where its addresses are, and it
    // is not in a link.
    //
    // A **ratio**, not a count. The first draft of this tested `anchors == 0`, which
    // sounds like entry 2's *"not one `<a href>` to a meeting anywhere on the page"* and
    // is not the same claim: that page carries 24 anchors, all of them navigation. Zero
    // anchors is a condition almost no real page meets, so the test would have been dead
    // code that read as coverage. What is actually true of both entries is that the page
    // is mostly *code* — 87.5 of 93.8 KiB here — and a page that is mostly code builds its
    // addresses at run time.
    if script_bytes * 2 > seed.page.bytes.len() {
        findings.push(format!(
            "most of the page is script — {} of {}, and {anchors} anchors. Its addresses \
             are likely built at run time.",
            crate::render::bytes(script_bytes as u64),
            crate::render::bytes(seed.page.bytes.len() as u64)
        ));
    }
    if !sitemap_declared {
        findings.push("no sitemap declared, so there is no surface to walk".to_string());
    }

    Lead {
        read,
        anchors,
        script_bytes,
        sitemap_declared,
        findings,
    }
}

/// What the real extractor gets out of the seed.
///
/// The bytes go to a temporary file because [`crate::extract::derive`] takes a path — some
/// readers shell out to a tool that needs one. It is removed when this returns, which is
/// the difference between this command and `check`: nothing here is meant to be opened.
///
/// The text rather than a count of it, because [`Verdict`] reads the text: a page can be
/// long and still be a menu, and only the characters say which.
async fn extracted_text(kind: ContentKind, seed: &Seed) -> String {
    let Ok(file) = tempfile::NamedTempFile::new() else {
        return String::new();
    };
    if tokio::fs::write(file.path(), &seed.page.bytes)
        .await
        .is_err()
    {
        return String::new();
    }
    crate::extract::derive(kind, &seed.page.bytes, file.path(), None, None)
        .await
        .outcome
        .text()
        .map(str::to_string)
        .unwrap_or_default()
}

/// Every off-host link on the seed, grouped by host.
fn crumbs_on(seed: &Seed) -> Vec<Crumb> {
    let Some(base) = seed.final_url() else {
        return Vec::new();
    };
    let here = base.host_str().unwrap_or_default().to_string();

    let html = seed.text();
    let mut by_host: BTreeMap<String, (usize, String)> = BTreeMap::new();
    for tag in crate::html::Scan::new(&html).tags(&["a"]) {
        let Some(href) = tag.attr("href") else {
            continue;
        };
        let Ok(target) = base.join(&crate::html::unescape(href)) else {
            continue;
        };
        let Some(host) = target.host_str() else {
            continue;
        };
        if host == here || !matches!(target.scheme(), "http" | "https") {
            continue;
        }
        let entry = by_host
            .entry(host.to_string())
            .or_insert_with(|| (0, target.to_string()));
        entry.0 += 1;
    }

    let mut out: Vec<Crumb> = by_host
        .into_iter()
        .map(|(host, (links, example))| Crumb {
            host,
            links,
            example,
        })
        .collect();
    // Most-linked first: a host named twenty times is a system, and one named once is a
    // footer link to the state portal.
    out.sort_by(|a, b| b.links.cmp(&a.links).then(a.host.cmp(&b.host)));
    out.truncate(MAX_CRUMBS);
    out
}

/// A source id a person would have typed: `publicrec.hillsclerk.com` → `publicrec`.
///
/// Only ever a suggestion on a line the operator reads before running, so an address that
/// yields nothing usable gets something typeable rather than an error.
fn suggest_id(url: &url::Url) -> String {
    let host = url.host_str().unwrap_or("source");
    let label = host
        .split('.')
        .find(|l| !l.is_empty() && *l != "www")
        .unwrap_or("source");
    let id: String = label
        .chars()
        .map(|c| match c.is_ascii_alphanumeric() {
            true => c.to_ascii_lowercase(),
            false => '-',
        })
        .collect();
    match id.is_empty() {
        true => "source".to_string(),
        false => id,
    }
}

// -----------------------------------------------------------------------------------------
// How it reads
// -----------------------------------------------------------------------------------------

impl Render for InvestigateReport {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        p.title(
            &render::truncate(&self.address, p.width().saturating_sub(12)),
            &render::duration(self.elapsed_secs),
        )?;

        p.nest(|p| {
            self.seed.render(p)?;

            match self.recognised.is_empty() {
                false => {
                    for r in &self.recognised {
                        r.render(p)?;
                    }
                    if let Some(probe) = &self.probe {
                        probe.render(p)?;
                    }
                }
                true => {
                    p.section("recognised")?;
                    p.nest(|p| p.marked(Mark::Warn, p.paint("nothing", Ink::Bold)))?;
                    if let Some(lead) = &self.lead {
                        lead.render(p)?;
                    }
                }
            }

            if !self.crumbs.is_empty() {
                p.section("crumbs")?;
                p.nest(|p| {
                    // Named and not followed. The operator promotes one by investigating
                    // it in turn, which is what stops a walk of one site becoming a walk
                    // of the internet.
                    for c in &self.crumbs {
                        p.kv(
                            &c.host,
                            32,
                            p.paint(&format!("{} link(s)", c.links), Ink::Dim),
                        )?;
                    }
                    Ok(())
                })?;
            }

            if !self.warnings.is_empty() {
                p.section("warnings")?;
                p.nest(|p| {
                    for w in &self.warnings {
                        p.marked(Mark::Warn, p.paint(&render::one_line(w), Ink::Dim))?;
                    }
                    Ok(())
                })?;
            }

            p.blank()?;
            match &self.promote {
                Some(line) => p.wrapped(line, Ink::Bold),
                None => p.wrapped(
                    "no `source add` line: nothing here knows how to enumerate this \
                     address, so collecting it would store a front page and little else.",
                    Ink::Dim,
                ),
            }
        })
    }
}

impl Render for SeedSummary {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        p.section("seed")?;
        p.nest(|p| {
            let robots = match self.robots_declared {
                true => "robots.txt read",
                false => "robots.txt unreachable, rules assumed",
            };
            p.wrapped(
                &format!(
                    "{} · {} · {} · {robots}",
                    self.http_status.as_deref().unwrap_or("200"),
                    render::bytes(self.bytes as u64),
                    self.kind,
                ),
                Ink::Dim,
            )?;
            // A redirect means the address typed is not the address answered, which is the
            // one thing on this line that changes what everything below is about.
            match &self.final_url {
                Some(url) => p.kv("served from", 14, p.paint(url, Ink::Plain)),
                None => Ok(()),
            }
        })
    }
}

impl Render for Recognised {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        p.section(match self.best {
            true => "recognised",
            // Shown, because a site answering to both a product and a standard is a site
            // where the choice between them is the whole decision.
            false => "also",
        })?;
        p.nest(|p| {
            // A runner-up asserts nothing: it is context for the choice, not a verdict.
            let mark = match self.best {
                true => Mark::Ok,
                false => Mark::None,
            };
            p.marked(
                mark,
                p.paint(
                    &format!("{} — {} ({})", self.strategy, self.keyed_on, self.kind),
                    Ink::Bold,
                ),
            )?;
            if !self.best {
                return Ok(());
            }
            for n in &self.evidence {
                p.kv(
                    &n.label,
                    12,
                    p.paint(&render::one_line(&n.detail), Ink::Dim),
                )?;
            }
            for n in &self.warnings {
                p.marked(Mark::Warn, p.paint(&render::one_line(&n.detail), Ink::Dim))?;
            }
            Ok(())
        })
    }
}

impl Render for Probe {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        p.section("size")?;
        p.nest(|p| {
            // Each strategy counts a different second thing, and the noun has to come with
            // the number: "across 11" is not a fact.
            let across = self
                .figures
                .get("directories")
                .map(|n| (n, "directories"))
                .or_else(|| {
                    self.figures
                        .get("sitemaps_fetched")
                        .map(|n| (n, "sitemaps"))
                });
            let mut line = format!("{} address(es)", render::count(self.addresses as u64));
            if let Some((n, noun)) = across {
                line.push_str(&format!(" across {} {noun}", render::count(*n)));
            }
            // The word that stops a probe reading as a total.
            match self.complete {
                true => line.push_str(&format!("   (probe, {} req)", self.requests_allowed)),
                false => line.push_str(&format!(
                    "   (probe STOPPED at {} req — there is more)",
                    self.requests_allowed
                )),
            }
            p.marked(
                match self.complete {
                    true => Mark::Ok,
                    false => Mark::Warn,
                },
                p.paint(&line, Ink::Bold),
            )?;

            if let Some(total) = self.declared_total {
                p.kv(
                    "declared",
                    12,
                    p.paint(
                        &format!("the source names {}", render::count(total)),
                        Ink::Dim,
                    ),
                )?;
            }
            for a in &self.sample {
                p.wrapped(a, Ink::Dim)?;
            }
            Ok(())
        })
    }
}

impl Render for Lead {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        p.section("measured")?;
        p.nest(|p| {
            p.kv(
                "text",
                12,
                p.paint(
                    &format!(
                        "{} chars, {:.0} per KB, {:.0}% link text",
                        self.read.chars,
                        self.read.chars_per_kb,
                        self.read.link_share * 100.0
                    ),
                    Ink::Dim,
                ),
            )?;
            p.kv(
                "markup",
                12,
                p.paint(
                    &format!(
                        "{} anchors, {} of <script>",
                        self.anchors,
                        render::bytes(self.script_bytes as u64)
                    ),
                    Ink::Dim,
                ),
            )?;
            p.kv(
                "sitemap",
                12,
                p.paint(
                    match self.sitemap_declared {
                        true => "declared",
                        false => "none declared",
                    },
                    Ink::Dim,
                ),
            )?;

            if self.is_lead() {
                p.blank()?;
                p.marked(Mark::Warn, p.paint("a lead", Ink::Bold))?;
                // The read first. A site can be recognised, enumerate perfectly, and
                // still hand back a menu, and that is the finding an operator acts on.
                for f in self.read.findings.iter().chain(self.findings.iter()) {
                    p.wrapped(f, Ink::Plain)?;
                }
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed_of(body: &str, url: &str) -> Seed {
        Seed {
            page: Fetched {
                bytes: body.as_bytes().to_vec(),
                meta: BTreeMap::from([("final_url".to_string(), url.to_string())]),
            },
            robots: crate::discovery::Robots::unreachable(Default::default()),
        }
    }

    /// A seed whose site shape is unremarkable — it declares a sitemap, so the only
    /// thing a finding can be about is the text that came out.
    fn ordinary_seed(body: &str, url: &str) -> Seed {
        let mut seed = seed_of(body, url);
        seed.robots = crate::discovery::Robots::parse(
            DEFAULT_USER_AGENT,
            format!("User-agent: *\nSitemap: {url}sitemap.xml\n").as_bytes(),
        );
        seed
    }

    #[test]
    fn an_id_is_suggested_from_the_host_a_person_would_have_typed() {
        let id = |u: &str| suggest_id(&url::Url::parse(u).unwrap());
        assert_eq!(id("https://publicrec.hillsclerk.com/Civil/"), "publicrec");
        // `www` is not a name anybody would file a source under.
        assert_eq!(id("https://www.tampa.gov/"), "tampa");
        assert_eq!(
            id("https://tampagov.hylandcloud.com/251agendaonline/"),
            "tampagov"
        );
        // Anything a SourceId would refuse becomes something typeable, never an error.
        assert_eq!(id("https://a_b.gov/"), "a-b");
    }

    /// Recorded, never followed. One Source per exact host is the rule, so a link to
    /// another host is a note for the operator rather than somewhere to go next.
    #[test]
    fn off_host_links_are_counted_by_host_and_the_seeds_own_host_is_not_one() {
        let s = seed_of(
            r#"<a href="/local">a</a>
               <a href="https://hover.hillsclerk.com/x">b</a>
               <a href="https://hover.hillsclerk.com/y">c</a>
               <a href="https://publicrec.hillsclerk.com/z">d</a>
               <a href="mailto:clerk@hillsclerk.com">e</a>"#,
            "https://www.hillsclerk.com/",
        );
        let crumbs = crumbs_on(&s);
        assert_eq!(crumbs.len(), 2, "{crumbs:?}");
        // Most-linked first: a host named twice is a system, one named once is a footer.
        assert_eq!(crumbs[0].host, "hover.hillsclerk.com");
        assert_eq!(crumbs[0].links, 2);
        assert_eq!(crumbs[1].host, "publicrec.hillsclerk.com");
    }

    /// Entry 2's shape: 94 KB of page, 87 KB of it script, and the meetings inside it.
    ///
    /// The anchors are deliberately **not** zero. That page has 24 of them, all
    /// navigation, which is why the test here is a ratio — see [`measure`].
    #[tokio::test]
    async fn a_page_that_is_mostly_script_is_a_lead_even_with_links_on_it() {
        let body = format!(
            "<html><body><a href=/a>nav</a><a href=/b>nav</a><p>Welcome.</p><script>{}</script></body></html>",
            "showSearchResults(new SearchResults({\"Meetings\":[]}));".repeat(400)
        );
        let lead = measure(&seed_of(&body, "https://x.gov/"), true).await;

        assert_eq!(lead.anchors, 2, "a real page has navigation");
        assert!(lead.script_bytes * 2 > body.len());
        assert!(lead.is_lead());
        assert!(
            lead.findings
                .iter()
                .any(|f| f.contains("most of the page is script")),
            "{:?}",
            lead.findings
        );
    }

    /// The ratio must not fire on an ordinary page that merely loads some JavaScript.
    #[tokio::test]
    async fn a_page_with_ordinary_scripts_on_it_is_not_mostly_script() {
        let prose = "<p>The council will consider the resurfacing contract at its next \
                     meeting, and the item may be pulled by any member.</p>"
            .repeat(60);
        let body = format!(
            "<html><body><article><h1>Council</h1>{prose}</article>\
             <script>var analytics = 1;</script></body></html>"
        );
        let lead = measure(&seed_of(&body, "https://x.gov/"), true).await;
        assert!(
            !lead
                .findings
                .iter()
                .any(|f| f.contains("most of the page is script")),
            "{:?}",
            lead.findings
        );
    }

    /// The whole point of ungating the measurement: it now runs on every address, so it
    /// must stay silent on the ordinary ones. A page of prose is mostly prose.
    #[tokio::test]
    async fn an_ordinary_page_of_prose_is_not_a_lead() {
        let body = format!(
            "<html><body><article><h1>Council</h1>{}</article></body></html>",
            "<p>The council will consider the resurfacing contract at its next meeting, \
             and the item may be pulled for discussion by any member.</p>"
                .repeat(40)
        );
        let lead = measure(&ordinary_seed(&body, "https://x.gov/"), true).await;
        assert!(
            !lead.is_lead(),
            "an ordinary page must not be a lead: {:?} {:?}",
            lead.read.findings,
            lead.findings
        );
    }

    /// A directory index is a page of links on purpose.
    ///
    /// `publicrec.hillsclerk.com/Civil/` reads as 62% link text and is working perfectly:
    /// what gets collected there are the files it lists, not its own text. Telling an
    /// operator their working listing "is a menu, not a page" is the kind of noise that
    /// teaches people to ignore findings.
    #[tokio::test]
    async fn an_index_page_is_not_judged_on_text_nobody_collects() {
        let links: String = (0..40)
            .map(|i| format!("<a href=\"file{i}.pdf\">Filing number {i}.pdf</a><br>"))
            .collect();
        let body = format!("<html><body><h1>Index of /Civil/</h1>{links}</body></html>");
        let seed = ordinary_seed(&body, "https://x.gov/Civil/");

        let collected = measure(&seed, true).await;
        assert!(
            collected.read.is_poor(),
            "the measurement itself still fires"
        );

        let index = measure(&seed, false).await;
        assert!(!index.read.is_poor(), "{:?}", index.read.findings);
        assert!(
            index.read.link_share > 0.5,
            "the number is kept, only the judgement is withdrawn"
        );
    }

    /// The case the gate used to hide. This page is recognised, enumerates cleanly, and
    /// is still a menu — so `investigate` must say so, not stay quiet because a strategy
    /// spoke. The shape findings are empty here on purpose: nothing about the *site* is
    /// unusual, and only the read is wrong.
    #[tokio::test]
    async fn a_page_that_reads_as_a_menu_is_a_lead_even_though_its_shape_is_ordinary() {
        let nav: String = (0..60)
            .map(|i| format!("<li><a href=\"/services/{i}\">Service number {i}</a></li>"))
            .collect();
        let body = format!(
            "<html><body><nav><ul>{nav}</ul></nav>\
             <article><p>Thanks! Your application was submitted.</p></article></body></html>"
        );
        let lead = measure(&ordinary_seed(&body, "https://x.gov/"), true).await;

        assert!(lead.findings.is_empty(), "the shape is ordinary");
        assert!(lead.is_lead(), "but the read is not");
        assert!(
            lead.read
                .findings
                .iter()
                .any(|f| f.contains("This is a menu")),
            "{:?}",
            lead.read.findings
        );
    }

    #[tokio::test]
    async fn an_address_that_is_not_http_is_refused_before_anything_is_fetched() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            crate::store::Store::open(dir.path().join("s"))
                .await
                .unwrap(),
        );
        let err = investigate(
            &ctx,
            InvestigateArgs {
                target: "/etc/passwd".into(),
                no_probe: true,
                user_agent: default_ua(),
                timeout_secs: 1,
            },
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .map(|_| ())
        .unwrap_err()
        .to_string();
        assert!(err.contains("not an http(s) address"), "{err}");
    }
}
