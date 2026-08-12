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
//! There is no `--add`. Promotion is `centinel source add`, which already exists, and this
//! report carries the arguments it would be given — see [`Promote`] — with the line to run
//! printed ready to paste. Keeping the two apart is what makes this command safe to point
//! at an address nobody has vetted: reading the evidence and acting on it stay separate
//! decisions, and the second one is always yours.
//!
//! The CLI offers to run that second command for you, once, after the evidence is on
//! screen. That offer lives **above** the op, beside the `schedule set` wizard, for the
//! reason that module gives: an op that prompts blocks an MCP call until the client times
//! out and hangs a script forever. So what changed is where the keystroke goes — `y`
//! rather than a line to copy — and not what this function does, which is still nothing.
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
use crate::crumbs::{self, Crumb};
use crate::prelude::*;
use crate::strategies::crawl::{self, Seed};
use crate::verdict::{Read, ReadQuality};

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

    #[command(flatten)]
    #[serde(default, flatten)]
    pub net: crate::ops::probe::NetArgs,
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
    /// Which strategy walked. Not always one that recognised the seed — see
    /// [`Self::by_fallback`].
    #[serde(default)]
    pub strategy: String,
    /// Nothing recognised the seed, so [`crawl::fallback`] walked it anyway.
    ///
    /// Worth its own field rather than left implicit in an empty `recognised` list, because
    /// the two facts point opposite ways and an operator needs both: *no strategy claims
    /// this site* and *here are the 4,260 addresses it will collect regardless.*
    #[serde(default)]
    pub by_fallback: bool,
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
    pub read: ReadQuality,
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

/// The `source add` this address earns, as arguments rather than as a line of shell.
///
/// A string was enough while the only thing anyone could do with it was read it and retype
/// it. The CLI now offers to run it, and a caller that has to parse a command line back
/// into arguments is a second and worse definition of what `source add` takes — so the
/// fields are the record and [`Self::command`] is one rendering of them.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Promote {
    /// The id it would be filed under — [`suggest_id`], which is what a person would have
    /// typed.
    pub id: String,
    /// The address, as given. `source add` keeps the path, because a strategy that walks a
    /// directory bounds itself by the one it was pointed at.
    pub site: String,
    /// Pinned only where a strategy **recognised** the address, never where the fallback
    /// merely walked it: pinning is the operator saying they saw the evidence and accepted
    /// it, and there is no evidence in a guess.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strategy: Option<String>,
}

impl Promote {
    /// The command that writes this block, ready to paste — and the one the CLI's offer
    /// runs, so what is printed and what would happen cannot drift apart.
    pub fn command(&self) -> String {
        let pin = match &self.strategy {
            Some(name) => format!(" --strategy={name}"),
            None => String::new(),
        };
        format!("centinel source add {} --site {}{pin}", self.id, self.site)
    }
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
    /// What adding this would take. Absent when nothing recognised it and no walk found
    /// anything, because offering it would be offering a corpus of one front page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub promote: Option<Promote>,
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

    let site =
        super::probe::site("investigate", url.as_str(), &args.net)?.with_cancel(cancel.clone());

    let (seed, mut warnings) = site.seed(progress).await?;
    cancel.check()?;

    // Every recogniser, not the first to answer. Recognition is pure over bytes already in
    // hand, so asking all of them costs nothing — and what the runners-up saw is evidence
    // worth showing when the choice between two of them matters.
    let hits = crawl::recognise(&seed);
    let recognised: Vec<Recognised> = hits
        .iter()
        .enumerate()
        .map(|(i, (_, r))| Recognised {
            strategy: r.strategy.to_string(),
            keyed_on: r.keyed_on.name().to_string(),
            kind: r.keyed_on.kind().to_string(),
            best: i == 0,
            evidence: r.evidence.clone(),
            warnings: r.warnings.clone(),
        })
        .collect();

    // Whatever `run` would do, and that is the whole point of the command. This used to
    // probe only when something recognised the seed, which made `investigate` answer a
    // different question from the one the pipeline answers: `run` falls back to
    // `crawl::fallback()` when nothing speaks, and the fallback collects most `.gov` sites
    // perfectly. So `boston.gov`, `clevelandohio.gov` and `clevelandcitycouncil.gov` — none
    // of which declares a sitemap, all of which serve one — were reported as *"nothing here
    // knows how to enumerate this address, so collecting it would store a front page and
    // little else"*, and then collected 4,260, 1,098 and 1,309 addresses seconds later.
    //
    // The registry's distinction is still worth keeping and is kept below: a walk that ran
    // because `robots.txt` declared an index is a recognition, and a walk that ran because
    // nothing else spoke is a guess. `by_fallback` carries that, so the report can say
    // which happened instead of the operator inferring it from silence.
    //
    // Asked of the source rather than re-derived from `hits`, which is what this did. Two
    // definitions of one decision is one too many, and this one had a consequence: a
    // `[[source]]` block that pins a strategy is honoured by `run` and was invisible here,
    // so `investigate` answered "what would run" differently from `run` on exactly the
    // sources somebody had already looked at once.
    let (chosen, recognition) = site.choose(&seed, &mut warnings);

    let probe = match args.no_probe {
        true => None,
        false => {
            let (def, by_fallback) = (chosen, recognition.is_none());
            let found = site.run(def, &seed, progress).await?;
            // The walk is the long-running part this op is declared for, and it used to be
            // uninterruptible: `cancel` was checked once, before it. The Source now stops
            // fetching between requests, and this is where that stop becomes an error
            // rather than a silently short answer.
            cancel.check()?;
            warnings.extend(found.warnings);
            Some(Probe {
                addresses: found.addresses.len(),
                // The walk's own account of stopping early, asked rather than inferred. A
                // probe that ran out is the one case where a count must never be read as a
                // total, and this was previously recovered by grepping warning text —
                // which missed the walk that fills its ceiling and then exits cleanly.
                complete: !found.truncated,
                strategy: def.name.to_string(),
                by_fallback,
                requests_allowed: super::probe::REQUESTS,
                sample: found.addresses.iter().take(SAMPLE).cloned().collect(),
                figures: found.figures,
                declared_total: found.declared_total,
            })
        }
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
    //
    // Asked of the strategy the registry handed back, rather than of one looked up again
    // by name — the same round-trip `choose` used to make, reaching a different answer on
    // failure.
    let seed_is_collected = hits
        .first()
        .is_none_or(|(def, _)| !matches!(def.it.addresses_are(), crawl::Addresses::Documents));
    let lead = Some(measure(&seed, seed_is_collected).await);

    // Offered on evidence of addresses, not on evidence of recognition. The old rule —
    // print it only when something recognised the seed — withheld it from every site the
    // fallback collects, which is most of them, and paired that silence with a line saying
    // collecting the address "would store a front page and little else". A walk that just
    // returned 4,260 addresses is the strongest possible argument for the opposite.
    //
    // It pins the strategy when one **recognised** the address, and not when the fallback
    // merely walked it: pinning is the operator saying they saw the evidence and accepted
    // it, and there is no evidence in a guess. The key had no writer at all before this —
    // `source.rs` carried a comment saying this command would write it, while this command
    // is documented as writing nothing, so pinning meant hand-editing `centinel.toml`.
    let promote =
        (!hits.is_empty() || probe.as_ref().is_some_and(|p| p.addresses > 0)).then(|| Promote {
            id: suggest_id(&url),
            site: url.to_string(),
            strategy: recognition.is_some().then(|| chosen.name.to_string()),
        });

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
    // The question is chosen, not asked and then unasked. Clearing `findings` afterwards
    // suppressed the printed line and left `--json` claiming an ordinary read beside a
    // link share of 0.62.
    let read = ReadQuality::on(
        &seed.page.bytes,
        &extracted_text(kind, seed).await,
        match seed_is_collected {
            true => Read::of(kind),
            false => Read::Index,
        },
    );

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
///
/// The scan itself is [`crate::crumbs`], which `crumbs` runs over a corpus of blobs and this
/// runs over one page that was just fetched. There used to be a second copy of it here, and
/// two copies of a scan is what [`crate::html`] was assembled out of.
///
/// The standing on each stays [`crumbs::Standing::Open`], because this command reads no store:
/// whether a host has already been refused or already collected is a question `crumbs`
/// answers, and answering it from here would need a corpus this command does not open.
fn crumbs_on(seed: &Seed) -> Vec<Crumb> {
    let Some(base) = seed.final_url() else {
        return Vec::new();
    };

    let mut trail = crumbs::Trail::default();
    trail.read(&seed.text(), crumbs::Carrier::at_address(base.to_string()));

    let mut out = trail.crumbs();
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

            // Three independent answers, printed unconditionally and in this order: what
            // claims the site, what a walk actually found, and what the read looks like.
            // Each used to hide behind another. The probe printed only when something
            // recognised the seed, so a fallback walk of 4,260 addresses was invisible; the
            // lead printed only when *nothing* did, so a recognised site with a ruined read
            // had nowhere to say so. Both are the same mistake — treating recognition as a
            // verdict on the whole site rather than an answer to one question.
            match self.recognised.is_empty() {
                false => {
                    for r in &self.recognised {
                        r.render(p)?;
                    }
                }
                true => {
                    p.section("recognised")?;
                    p.nest(|p| p.marked(Mark::Warn, p.paint("nothing", Ink::Bold)))?;
                }
            }
            if let Some(probe) = &self.probe {
                probe.render(p)?;
            }
            if let Some(lead) = &self.lead {
                lead.render(p)?;
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
                // Printed even where the CLI is about to offer to run it. The offer needs
                // a terminal and this line does not, so a piped or scripted investigation
                // still ends with the command that acts on it.
                Some(promote) => p.wrapped(&promote.command(), Ink::Bold),
                // Reworded with the gate. The old text claimed nothing knew how to
                // enumerate the address, which was said of `boston.gov` moments before
                // `run` enumerated 4,260 of it. Now this line is only reached when a walk
                // actually ran and came back empty — so it can say the narrower, true
                // thing, and `--no-probe` gets its own wording because it did not look.
                None => p.wrapped(
                    match self.probe.is_some() {
                        true => {
                            "no `source add` line: nothing recognised this address and \
                                 a fallback walk found no addresses behind it, so \
                                 collecting it would store this page and little else."
                        }
                        false => {
                            "no `source add` line: nothing recognised this address, \
                                  and `--no-probe` means no walk was tried. Run without it \
                                  to see whether the fallback finds anything."
                        }
                    },
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
                    // Never invented. `seed` builds an empty page with a warning when the
                    // front door refuses, and the meta it hands back carries no status — so
                    // `unwrap_or("200")` rendered a WAF 403 as `200 · 0 B · other`, with
                    // the truth demoted to a warning further down. An unknown status says
                    // it is unknown, and the zero bytes beside it then read correctly.
                    self.http_status.as_deref().unwrap_or("no status"),
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
            // The word that stops a probe reading as a total. It does **not** name which
            // ceiling stopped the walk, because this line does not know: the walk has two,
            // and it used to blame the request budget for an address cap either way —
            // `buffalony.gov` printed "STOPPED at 25 req" having actually filled 500
            // addresses inside one sitemap. The warning below names the real one.
            match self.complete {
                true => line.push_str(&format!("   (probe, {} req)", self.requests_allowed)),
                false => line.push_str(&format!(
                    "   (probe STOPPED, {} req allowed — there is more)",
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

            // Said out loud, because the number above is the answer to "is this worth
            // collecting?" and the reader has just been told nothing recognised the site.
            // Without this line those two facts look like a contradiction.
            if self.by_fallback {
                p.kv(
                    "walked by",
                    12,
                    p.paint(
                        &format!(
                            "`{}`, as a fallback — nothing recognised this address, and \
                             `run` would do the same",
                            self.strategy
                        ),
                        Ink::Dim,
                    ),
                )?;
            }

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

    // The strategy registry's own harness, which already builds both of these. This file
    // had a second copy of each, differing only in whitespace.
    use crate::strategies::crawl::tests::{seed as seed_of, seed_with_robots};

    /// A seed whose site shape is unremarkable — it declares a sitemap, so the only
    /// thing a finding can be about is the text that came out.
    fn ordinary_seed(body: &str, url: &str) -> Seed {
        Seed {
            page: seed_of(body, url).page,
            ..seed_with_robots(url, &format!("User-agent: *\nSitemap: {url}sitemap.xml\n"))
        }
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

    /// The printed line and the offer the CLI makes are one thing, so the command has to
    /// be derived from the fields rather than written beside them.
    #[test]
    fn the_command_is_the_fields_and_the_pin_is_only_there_when_something_recognised_it() {
        let promote = Promote {
            id: "publicrec".into(),
            site: "https://publicrec.hillsclerk.com/Civil/".into(),
            strategy: Some("listing".into()),
        };
        assert_eq!(
            promote.command(),
            "centinel source add publicrec --site https://publicrec.hillsclerk.com/Civil/ \
             --strategy=listing"
        );

        let guessed = Promote {
            strategy: None,
            ..promote
        };
        assert!(!guessed.command().contains("--strategy"), "{guessed:?}");
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
        // No article at all, so the whole page is genuinely all there is to read and the
        // menu is genuinely what came out. This used to carry one sentence of content as
        // well — see the test below, which is what the reader now does with that.
        let nav: String = (0..60)
            .map(|i| format!("<li><a href=\"/services/{i}\">Service number {i}</a></li>"))
            .collect();
        let body = format!("<html><body><nav><ul>{nav}</ul></nav></body></html>");
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

    /// `hillsclerk.com/marriage-license-application-success-kiosk`, and the fix for it.
    ///
    /// One sentence of real content inside a large menu. This is the page that put 23,213
    /// characters of navigation into the corpus, because the sentence fell under the old
    /// character floor and the whole page replaced it. The sentence is now kept, the menu
    /// is not, and so there is no longer a bad read here for the verdict to report.
    #[tokio::test]
    async fn one_sentence_of_content_beats_the_menu_that_surrounds_it() {
        let nav: String = (0..60)
            .map(|i| format!("<li><a href=\"/services/{i}\">Service number {i}</a></li>"))
            .collect();
        let body = format!(
            "<html><body><nav><ul>{nav}</ul></nav>\
             <article><p>Thanks! Your application was submitted.</p></article></body></html>"
        );
        let lead = measure(&ordinary_seed(&body, "https://x.gov/"), true).await;

        assert!(
            lead.read.findings.is_empty(),
            "the read is good now: {:?}",
            lead.read.findings
        );
        assert!(!lead.is_lead(), "so the page is not a lead at all");
        assert!(
            lead.read.chars < 200,
            "and what was kept is the sentence, not the menu: {} chars",
            lead.read.chars
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
                net: crate::ops::probe::NetArgs {
                    timeout_secs: 1,
                    ..Default::default()
                },
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

    // ── rendering ─────────────────────────────────────────────────────────────────
    //
    // Every other op renders into a `Painter` and asserts on the text. This one had 285
    // lines of rendering and no test over any of it, which is how a fabricated HTTP status
    // survived in the one line an operator reads first.

    fn render_to_string(report: &InvestigateReport) -> String {
        let mut buf: Vec<u8> = Vec::new();
        {
            let mut p = Painter::new(&mut buf, false, 100);
            report.render(&mut p).unwrap();
        }
        String::from_utf8(buf).unwrap()
    }

    fn report() -> InvestigateReport {
        InvestigateReport {
            address: "https://x.gov/".into(),
            seed: SeedSummary {
                http_status: Some("200 OK".into()),
                bytes: 91_000,
                kind: "html".into(),
                final_url: None,
                robots_declared: true,
            },
            recognised: Vec::new(),
            probe: None,
            lead: None,
            crumbs: Vec::new(),
            promote: None,
            warnings: Vec::new(),
            elapsed_secs: 0.4,
        }
    }

    fn probe() -> Probe {
        Probe {
            addresses: 500,
            complete: false,
            strategy: "sitemap".into(),
            by_fallback: true,
            requests_allowed: super::super::probe::REQUESTS,
            sample: vec!["https://x.gov/a".into()],
            figures: BTreeMap::new(),
            declared_total: None,
        }
    }

    /// A seed that refused carries no status, and inventing one put `200 · 0 B` on the
    /// line an operator reads to decide whether the fetch worked. `seed` builds exactly
    /// this — empty bytes, a warning, and no `http_status` — whenever the front door 403s.
    #[test]
    fn a_seed_that_never_answered_does_not_report_a_status_it_never_gave() {
        let mut r = report();
        r.seed.http_status = None;
        r.seed.bytes = 0;
        r.seed.kind = "other".into();

        let out = render_to_string(&r);
        assert!(
            !out.contains("200"),
            "a status nothing returned was rendered:\n{out}"
        );
        assert!(out.contains("no status"), "{out}");
    }

    /// A probe that filled its ceiling must not read as a total — §4.3 at probe scale.
    #[test]
    fn a_probe_that_ran_out_is_marked_rather_than_reported_as_a_size() {
        let mut r = report();
        r.probe = Some(probe());

        let out = render_to_string(&r);
        assert!(out.contains('!') || out.contains('⚠'), "unmarked:\n{out}");
        assert!(
            out.contains("at least") || out.contains("more"),
            "the ceiling was not said out loud:\n{out}"
        );
    }

    /// The two facts point opposite ways and an operator needs both: *nothing recognised
    /// this* and *here are the addresses `run` will collect anyway*.
    #[test]
    fn a_fallback_walk_says_that_run_would_do_the_same() {
        let mut r = report();
        r.probe = Some(probe());

        let out = render_to_string(&r);
        assert!(out.contains("fallback"), "{out}");
        assert!(out.contains("`run` would do the same"), "{out}");
    }

    /// The offer the CLI makes needs a terminal; this line does not. A piped or scripted
    /// investigation has to end with the command that acts on it.
    #[test]
    fn the_line_that_adds_it_is_printed_from_the_promotion_itself() {
        let mut r = report();
        r.promote = Some(Promote {
            id: "agartha".into(),
            site: "https://www.agartha.gov/".into(),
            strategy: Some("sitemap".into()),
        });

        let out = render_to_string(&r);
        assert!(
            out.contains("centinel source add agartha --site https://www.agartha.gov/"),
            "{out}"
        );
        assert!(out.contains("--strategy=sitemap"), "{out}");
    }

    /// A recognised walk is not a guess and must not be labelled one.
    #[test]
    fn a_recognised_walk_carries_no_fallback_line() {
        let mut r = report();
        r.probe = Some(Probe {
            by_fallback: false,
            complete: true,
            ..probe()
        });

        let out = render_to_string(&r);
        assert!(!out.contains("as a fallback"), "{out}");
    }
}
