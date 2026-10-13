//! `crumbs` — the hosts this corpus points at and does not collect.
//!
//! The other end of [`crate::ops::investigate`]. That command names the crumbs on **one
//! page** before anything is collected; this one names them across **everything collected**,
//! which is where the interesting ones are: a records system linked from four hundred agenda
//! pages is not visible from a front door.
//!
//! ```text
//! centinel crumbs                        every host found, most-linked first
//! centinel crumbs --source tampa         only what one Source dropped
//! centinel crumbs show apps.tampagov.net the pages that linked there
//! centinel crumbs ignore facebook.com    write the ruling; stop offering it
//! centinel crumbs allow facebook.com     take it back
//! centinel crumbs --rescan               re-read the blobs, write the ledger again
//! ```
//!
//! ## A ledger read, with the blobs as the floor
//!
//! `collect` writes a row per stored artifact while the page is still in hand, so this reads
//! one file per Source: **0.05s over 5,000 pages** where reading the blobs is 8.8s.
//!
//! A row can be missing for exactly three reasons — the page was collected before the ledger
//! existed, a write failed, or somebody deleted the file — and in all three the blob is still
//! there, immutable, and says the same thing. The pass **counts** those pages and names the
//! one command that writes them down, `--rescan`; it does not read them itself. It used to,
//! and on a source collected before the ledger existed that was every blob of the source on
//! every read — nineteen thousand reads behind one click of a web page, and a web page is
//! not where a one-time backfill of a corpus belongs. The count is on the report rather than
//! in a log line because a quietly incomplete answer is the failure this codebase is most
//! consistently written against.
//!
//! Whether a host is **already a Source** is asked of every definition and every log's
//! [`crate::summary::Summary`], which is a `stat` and a small read per source rather than a
//! replay of every log to answer a question about one.
//!
//! ## `Operator`, because it writes a ruling
//!
//! Listing is a read, and `ignore` is the operator refusing a host for the life of the
//! corpus. One op, so the stricter reach governs both: an unauthenticated HTTP caller must
//! not be able to decide what this corpus will never look at.

use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::content::{ContentKind, SNIFF_BYTES};
use crate::crumbs::{
    self, Carrier, Crumb, Decision, Decisions, Ledger, Recorded, Ruling, Standing, Trail,
};
use crate::prelude::*;

/// Hosts named before the list is cut off.
fn default_max() -> usize {
    25
}

#[derive(Clone, Debug, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct CrumbsArgs {
    #[command(subcommand)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<CrumbsAction>,

    /// Limit to the crumbs one source dropped. Omit for the whole corpus.
    #[arg(long)]
    #[serde(default)]
    pub source: Option<String>,

    /// Include the hosts an earlier `ignore` refused, rather than hiding them.
    #[arg(long)]
    #[serde(default)]
    pub all: bool,

    /// Hosts to name before the list is cut off.
    #[arg(long, default_value_t = 25)]
    #[serde(default = "default_max")]
    pub max: usize,

    /// Re-read every blob and write the ledger again.
    ///
    /// For a corpus collected before `collect` wrote crumbs down, and the repair for a
    /// ledger that was damaged or deleted. Costs a read of every HTML blob in the selected
    /// sources; every pass after it is a read of one file.
    #[arg(long)]
    #[serde(default)]
    pub rescan: bool,
}

/// Hand-written, because a derived one would set `max` to zero — and a list that silently
/// truncates to nothing is the failure every cap in this codebase is documented against.
impl Default for CrumbsArgs {
    fn default() -> Self {
        Self {
            action: None,
            source: None,
            all: false,
            max: default_max(),
            rescan: false,
        }
    }
}

#[derive(Clone, Debug, clap::Subcommand, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum CrumbsAction {
    /// The pages that linked to one host.
    Show(ShowArgs),
    /// Refuse a host, so no future pass offers it.
    Ignore(RuleArgs),
    /// Take back a refusal.
    Allow(RuleArgs),
}

#[derive(Clone, Debug, Default, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct ShowArgs {
    /// The host, or any URL on it.
    #[arg(value_name = "HOST")]
    pub host: String,

    /// Limit to one source.
    #[arg(long)]
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Clone, Debug, Default, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct RuleArgs {
    /// The host, or any URL on it.
    #[arg(value_name = "HOST")]
    pub host: String,

    /// Why — read back whenever the ruling is reported.
    #[arg(long)]
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum CrumbsReport {
    List {
        /// The Sources whose pages were read.
        sources: Vec<String>,
        /// Documents accounted for — the denominator for every count below.
        ///
        /// Documents rather than pages, because a PDF was examined and cannot carry a crumb:
        /// see [`crate::crumbs::Trail::documents_read`].
        documents: usize,
        crumbs: Vec<Crumb>,
        /// Hosts an earlier `ignore` kept out of the list. `--all` shows them.
        hidden: usize,
        /// Hosts past `--max`.
        truncated: usize,
        /// Pages with no ledger row for their current blob, so their links are not in these
        /// counts. Zero on a corpus collected by this build; the whole page count on one
        /// collected before the ledger existed, until `--rescan` writes them down.
        #[serde(default)]
        unrecorded: usize,
        /// Pages read out of their blob and written down. Only `--rescan` reads a blob.
        from_blobs: usize,
        /// Blobs that could not be read, so their links are missing from these counts. A
        /// pass over every blob in a corpus is the first thing to notice a damaged pool, and
        /// a number that is quietly short is worse than one that says it is.
        unread: usize,
        /// Whether the ledgers were written again.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        rescanned: bool,
        elapsed_secs: f64,
    },
    Show {
        host: String,
        links: usize,
        pages: usize,
        standing: Standing,
        /// Why the operator refused it, when they said.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
        /// The pages that carried it, capped by [`crate::crumbs::MAX_CARRIERS`].
        carried_by: Vec<Carrier>,
        /// Carrying pages past the cap.
        dropped: usize,
        /// One address on the host, to look at. Absent when nothing links there.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        example: Option<String>,
        elapsed_secs: f64,
    },
    Ruled {
        host: String,
        ruling: Ruling,
        at: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
        /// The ruling this one replaced, if any. So `ignore` twice reads differently from
        /// `ignore` after an `allow`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        previous: Option<String>,
    },
}

/// The off-host hosts this corpus links to, and the operator's rulings on them.
#[op(reach = "operator", group = "corpus", long_running)]
pub async fn crumbs(
    ctx: &Ctx,
    args: CrumbsArgs,
    p: &Progress,
    cancel: &Cancel,
) -> anyhow::Result<CrumbsReport> {
    match args.action.clone() {
        Some(CrumbsAction::Ignore(a)) => rule(ctx, a, Ruling::Ignore).await,
        Some(CrumbsAction::Allow(a)) => rule(ctx, a, Ruling::Allow).await,
        Some(CrumbsAction::Show(a)) => show(ctx, &args, a, p, cancel).await,
        None => list(ctx, &args, p, cancel).await,
    }
}

async fn list(
    ctx: &Ctx,
    args: &CrumbsArgs,
    p: &Progress,
    cancel: &Cancel,
) -> anyhow::Result<CrumbsReport> {
    let started = std::time::Instant::now();
    let field = survey(
        ctx,
        args.source.as_deref(),
        args.rescan,
        p,
        cancel,
    )
    .await?;

    let mut crumbs = field.trail.crumbs();
    for crumb in &mut crumbs {
        crumb.standing = field.standing_of(&crumb.host);
    }

    // Counted before they are dropped, so a list of two reads as "two, and forty you have
    // already refused" rather than as a corpus that links almost nowhere.
    let before = crumbs.len();
    if !args.all {
        crumbs.retain(|c| c.standing != Standing::Ignored);
    }
    let hidden = before - crumbs.len();

    let truncated = crumbs.len().saturating_sub(args.max);
    crumbs.truncate(args.max);

    Ok(CrumbsReport::List {
        sources: field.sources,
        documents: field.trail.documents_read(),
        crumbs,
        hidden,
        truncated,
        unrecorded: field.unrecorded,
        from_blobs: field.from_blobs,
        unread: field.unread,
        rescanned: args.rescan,
        elapsed_secs: started.elapsed().as_secs_f64(),
    })
}

async fn show(
    ctx: &Ctx,
    args: &CrumbsArgs,
    which: ShowArgs,
    p: &Progress,
    cancel: &Cancel,
) -> anyhow::Result<CrumbsReport> {
    let started = std::time::Instant::now();
    let host = as_host(&which.host)?;
    let source = which.source.as_deref().or(args.source.as_deref());
    let field = survey(ctx, source, args.rescan, p, cancel).await?;

    let crumb = field.trail.crumbs().into_iter().find(|c| c.host == host);
    let (carried_by, dropped) = field.trail.carriers_of(&host);

    Ok(CrumbsReport::Show {
        links: crumb.as_ref().map(|c| c.links).unwrap_or_default(),
        pages: crumb.as_ref().map(|c| c.pages).unwrap_or_default(),
        example: crumb.map(|c| c.example),
        standing: field.standing_of(&host),
        note: field.decisions.get(&host).and_then(|d| d.note.clone()),
        host,
        carried_by,
        dropped,
        elapsed_secs: started.elapsed().as_secs_f64(),
    })
}

/// Writes one ruling. Reads no blobs: refusing a host is a decision about a name.
async fn rule(ctx: &Ctx, args: RuleArgs, ruling: Ruling) -> anyhow::Result<CrumbsReport> {
    let host = as_host(&args.host)?;
    let decisions = Decisions::new(&ctx.store);

    let previous = decisions
        .current()
        .await?
        .get(&host)
        .map(|d| format!("{} on {}", d.ruling.as_str(), d.at));

    let at = jiff::Timestamp::now();
    decisions
        .append(&Decision {
            host: host.clone(),
            ruling,
            at,
            note: args.note.clone(),
        })
        .await?;

    Ok(CrumbsReport::Ruled {
        host,
        ruling,
        at: at.to_string(),
        note: args.note,
        previous,
    })
}

/// One pass over the store: what the pages point at, and what is already collected.
struct Field {
    sources: Vec<String>,
    trail: Trail,
    /// Hosts a Source already covers, so a crumb pointing at one is not a candidate.
    promoted: BTreeSet<String>,
    decisions: BTreeMap<String, Decision>,
    /// Pages with no row for their current blob. Counted, never read.
    unrecorded: usize,
    /// Pages read out of a blob and written down, which only `--rescan` does.
    from_blobs: usize,
    unread: usize,
}

impl Field {
    /// Already a Source beats already refused: a host that was promoted is answered, and an
    /// old `ignore` on it is stale rather than binding.
    fn standing_of(&self, host: &str) -> Standing {
        if self.promoted.contains(host) {
            return Standing::Promoted;
        }
        match self.decisions.get(host).map(|d| d.ruling) {
            Some(Ruling::Ignore) => Standing::Ignored,
            _ => Standing::Open,
        }
    }
}

/// The hosts the store's source definitions claim.
///
/// The **intent** half of "already a Source"; [`survey`] adds the evidence half out of the
/// logs it is reading anyway. A block for a host nothing has collected yet still answers the
/// question — offering it back would ask the operator to decide something they decided an hour
/// ago — and one that is `enabled = false` answers it too: they chose the host and then chose
/// not to run it, and neither of those decisions was "ask me again".
///
/// **A channel claims no host.** `youtube.com` holds every channel there is, so a source for
/// one of them says nothing about a crumb pointing at another. Marking the host answered would
/// hide it; leaving it open costs one `crumbs ignore youtube.com` and hides nothing. A
/// definition knows a channel by which field is set, and the log by
/// [`crate::sources::channel::claims`].
fn claimed_hosts(defined: &[crate::config::SourceConfig]) -> BTreeSet<String> {
    defined
        .iter()
        .filter_map(|s| s.site.as_deref().and_then(crumbs::host_of))
        .collect()
}

/// Every page of the selected Sources, from the ledger.
///
/// **The ledger is the fast path and the blobs are the floor — and only `--rescan` walks
/// the floor.** `collect` writes a row per stored artifact, so a corpus collected by this
/// build answers out of one file per Source. A page with no row for its current blob is
/// counted as `unrecorded` and left out, and the report names the command that writes it
/// down once. Reading it here instead would put a one-time backfill of a corpus behind
/// every read of it, which on the web is a click.
///
/// Rows are keyed by the page's **address**, and a row answers only for the blob the log
/// says is current ([`Recorded::is_of`]). Keyed on the blob alone, two pages with identical
/// bytes — one blob — got each other's row, and a page re-collected with new content was
/// answered by the links it used to have.
///
/// Whether a host is **already a Source** is a corpus-wide question, so it is asked of every
/// definition and every Source's summary rather than only of the selected Sources — a crumb
/// Tampa dropped may be a Source somebody added last week. See [`claimed_hosts`] for the
/// definition half; the evidence half is [`crate::summary::Summary::claimed_hosts`].
async fn survey(
    ctx: &Ctx,
    source: Option<&str>,
    rescan: bool,
    p: &Progress,
    cancel: &Cancel,
) -> anyhow::Result<Field> {
    let known = ctx.store.sources().await?;

    let selected: Vec<SourceId> = match source {
        Some(id) => {
            let id = SourceId::new(id.to_string())?;
            // A typo that answers "no crumbs" is a wrong answer to a question the operator
            // did not ask. Name what the store holds instead.
            if !known.contains(&id) {
                let names: Vec<&str> = known.iter().map(|s| s.as_str()).collect();
                anyhow::bail!(
                    "no source `{id}` in this store — it holds {}",
                    match names.is_empty() {
                        true => "none".to_string(),
                        false => names.join(", "),
                    }
                );
            }
            vec![id]
        }
        None => known.clone(),
    };

    // The config first, so a malformed one fails before an hour of blob reads rather than
    // after them.
    let mut promoted = claimed_hosts(&crate::catalog::Sources::new(&ctx.store).list().await?);
    let summaries = crate::summary::Summaries::new(&ctx.store);
    for id in &known {
        promoted.extend(summaries.get(id).await?.claimed_hosts);
    }

    // The newest Observation per Resource of each selected Source, never the history: one
    // page holds one set of links, and an older version of it holds the links the site has
    // since dropped. One replay per selected Source, which is the one log this question is
    // about.
    let mut work: Vec<(SourceId, Vec<Observation>)> = Vec::with_capacity(selected.len());
    for id in &selected {
        let mut pages: Vec<Observation> = ctx
            .store
            .replay(id)
            .await?
            .latest_observations()
            .into_values()
            .collect();
        // Deterministic order, so two passes over one store read the same.
        pages.sort_by(|a, b| a.resource.natural_key.cmp(&b.resource.natural_key));
        work.push((id.clone(), pages));
    }

    let total: u64 = work.iter().map(|(_, pages)| pages.len() as u64).sum();
    let mut trail = Trail::default();
    let mut done = 0u64;
    let mut unrecorded = 0;
    let mut from_blobs = 0;
    let mut unread = 0;

    for (id, pages) in &work {
        let ledger = Ledger::new(&ctx.store, id.clone());
        let recorded: BTreeMap<String, Recorded> = match rescan {
            true => BTreeMap::new(),
            false => ledger.current().await?,
        };

        let mut rows: Vec<Recorded> = Vec::with_capacity(pages.len());
        for obs in pages {
            cancel.check()?;
            p.step(obs.resource.natural_key.clone(), done, total);
            done += 1;

            if let Some(row) = recorded
                .get(&obs.resource.natural_key)
                .filter(|row| row.is_of(&obs.blob_sha))
            {
                trail.absorb(row);
                rows.push(row.clone());
                continue;
            }
            if !rescan {
                unrecorded += 1;
                continue;
            }
            match scan_blob(ctx, obs).await {
                Some(row) => {
                    from_blobs += 1;
                    trail.absorb(&row);
                    rows.push(row);
                }
                None => unread += 1,
            }
        }

        // Only on `--rescan`: an op that quietly rewrote a file on every read would be a
        // surprise, and the pass above is already correct without it.
        if rescan {
            ledger.rewrite(&rows).await?;
        }
    }

    Ok(Field {
        sources: selected.iter().map(|s| s.to_string()).collect(),
        trail,
        promoted,
        decisions: Decisions::new(&ctx.store).current().await?,
        unrecorded,
        from_blobs,
        unread,
    })
}

/// One page's row, read out of the pool. `None` when the blob cannot be read at all.
///
/// A pass over every blob in a corpus is the first thing that would notice a damaged pool, so
/// the failure is returned rather than swallowed — the caller counts it and says so.
async fn scan_blob(ctx: &Ctx, obs: &Observation) -> Option<Recorded> {
    let page = Carrier {
        address: obs.resource.natural_key.clone(),
        at: Some(obs.at.to_string()),
        blob: Some(obs.blob_sha.as_str().to_string()),
    };

    // The head decides the kind; reading whole PDFs to find out would be gigabytes to answer
    // what four kilobytes settle.
    let head = ctx.store.blob_head(&obs.blob_sha, SNIFF_BYTES).await.ok()?;
    if ContentKind::classify(&obs.meta, &head) != ContentKind::Html {
        return Some(Recorded {
            page,
            links: Vec::new(),
        });
    }

    // Verified, because these bytes are about to be shown to a person as evidence.
    let bytes = ctx.store.get_blob(&obs.blob_sha).await.ok()?;
    Some(crumbs::scan_page(&String::from_utf8_lossy(&bytes), page))
}

/// A host, from a host or from any URL on one.
///
/// Both are what the operator has in hand — the list prints hosts and a crumb's example is a
/// URL — so either is a reasonable thing to paste back. Anything that is neither is an error
/// rather than a stored "host" with a slash in it that no crumb will ever equal.
fn as_host(input: &str) -> anyhow::Result<String> {
    let trimmed = input.trim();
    if let Some(host) = url::Url::parse(trimmed).ok().and_then(|u| {
        u.host_str()
            .filter(|_| matches!(u.scheme(), "http" | "https"))
            .map(str::to_ascii_lowercase)
    }) {
        return Ok(host);
    }

    // A bare host has no scheme, so `Url::parse` refuses it outright.
    let host = trimmed.trim_end_matches('/').to_ascii_lowercase();
    let plausible = !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_'));
    match plausible {
        true => Ok(host),
        false => anyhow::bail!("`{input}` is not a host, or a http(s) URL on one"),
    }
}

// -----------------------------------------------------------------------------------------
// How it reads
// -----------------------------------------------------------------------------------------

impl Render for CrumbsReport {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        match self {
            Self::List {
                sources,
                documents,
                crumbs,
                hidden,
                truncated,
                unrecorded,
                from_blobs,
                unread,
                rescanned: _,
                elapsed_secs,
            } => {
                let scanned = format!(
                    "{} in {} · {}",
                    render::plural(*documents, "document", "documents"),
                    render::plural(sources.len(), "source", "sources"),
                    render::duration(*elapsed_secs),
                );
                p.title("crumbs", &scanned)?;

                p.nest(|p| {
                    if crumbs.is_empty() {
                        p.wrapped(
                            match (*documents, *hidden) {
                                (0, _) => {
                                    "nothing collected yet, so nothing has dropped a \
                                           crumb. Run `centinel run` first."
                                }
                                (_, 0) => "nothing in this corpus links off its own host.",
                                _ => "every host this corpus links to has been refused.",
                            },
                            Ink::Dim,
                        )?;
                    } else {
                        let mut table = Table::new(&[
                            ("", Align::Left),
                            ("host", Align::Left),
                            ("links", Align::Right),
                            ("pages", Align::Right),
                            ("", Align::Left),
                        ]);
                        for c in crumbs {
                            // Dim for a row already dealt with: the operator is scanning
                            // this list for what still wants a decision.
                            let ink = match c.standing.is_open() {
                                true => Ink::Plain,
                                false => Ink::Dim,
                            };
                            table.push(vec![
                                Cell::mark(c.standing.mark()),
                                Cell::new(render::truncate(&c.host, 40), ink),
                                Cell::new(render::count(c.links as u64), ink),
                                Cell::new(render::count(c.pages as u64), Ink::Dim),
                                Cell::dim(c.standing.label()),
                            ]);
                        }
                        p.table(&table)?;
                    }

                    if *hidden > 0 {
                        p.note(format!(
                            "{} refused earlier, hidden — `--all` lists them",
                            render::count(*hidden as u64)
                        ))?;
                    }
                    if *truncated > 0 {
                        p.note(format!(
                            "… and {} more, raise --max to see them",
                            render::count(*truncated as u64)
                        ))?;
                    }
                    if *unread > 0 {
                        p.marked(
                            Mark::Warn,
                            p.paint(
                                &format!(
                                    "{} could not be read, so these counts are a floor",
                                    render::plural(*unread, "blob", "blobs")
                                ),
                                Ink::Dim,
                            ),
                        )?;
                    }
                    if *from_blobs > 0 {
                        p.note(format!(
                            "{} read from blobs and written down",
                            render::plural(*from_blobs, "page", "pages")
                        ))?;
                    }
                    // What is missing, and the one command that fixes it. `collect` writes
                    // a row per page, so a non-zero count here is a backlog — pages
                    // collected before the ledger existed — and the counts above are a
                    // floor until it is written down.
                    match *unrecorded {
                        0 => Ok(()),
                        n => p.marked(
                            Mark::Warn,
                            p.paint(
                                &format!(
                                    "{} have no record and are not counted — \
                                     `centinel crumbs --rescan{}` writes them down once",
                                    render::plural(n, "page", "pages"),
                                    match sources.as_slice() {
                                        [one] => format!(" --source {one}"),
                                        _ => String::new(),
                                    }
                                ),
                                Ink::Dim,
                            ),
                        ),
                    }
                })?;

                // The next command, ready to paste — for the first host still open, because
                // promotion is one host at a time and always the operator's.
                if let Some(next) = crumbs.iter().find(|c| c.standing.is_open()) {
                    p.blank()?;
                    p.wrapped(&format!("centinel investigate {}", next.example), Ink::Bold)?;
                    p.nest(|p| {
                        p.wrapped(
                            "each crumb you promote becomes a Source that walks its own host \
                             and drops its own crumbs; each one you refuse is what stops the \
                             walk. Neither happens on its own.",
                            Ink::Dim,
                        )
                    })?;
                }
                Ok(())
            }

            Self::Show {
                host,
                links,
                pages,
                standing,
                note,
                carried_by,
                dropped,
                example,
                elapsed_secs,
            } => {
                let counts = match links {
                    0 => render::duration(*elapsed_secs),
                    _ => format!(
                        "{} · {} · {}",
                        render::plural(*links, "link", "links"),
                        render::plural(*pages, "page", "pages"),
                        render::duration(*elapsed_secs),
                    ),
                };
                p.title(host, &counts)?;

                p.nest(|p| {
                    if !standing.is_open() {
                        let mut line = standing.label().to_string();
                        if let Some(why) = note {
                            line.push_str(" · ");
                            line.push_str(&render::one_line(why));
                        }
                        p.marked(standing.mark(), p.paint(&line, Ink::Dim))?;
                    }
                    if *links == 0 {
                        return p.wrapped(
                            "no page in this corpus links there. Check the spelling, or the \
                             source it was dropped by.",
                            Ink::Dim,
                        );
                    }

                    p.section("carried by")?;
                    p.nest(|p| {
                        for c in carried_by {
                            p.line(render::truncate(&c.address, p.width().saturating_sub(24)))?;
                            // The blob is the openable handle for the page a link was read
                            // out of: `centinel open <hash>` takes it back.
                            let mut aside = String::new();
                            if let Some(at) = &c.at {
                                aside.push_str(&render::short_time(at));
                            }
                            if let Some(blob) = &c.blob {
                                if !aside.is_empty() {
                                    aside.push_str(" · ");
                                }
                                aside.push_str(&render::short_sha(blob));
                            }
                            if !aside.is_empty() {
                                p.nest(|p| p.line(p.paint(&aside, Ink::Dim)))?;
                            }
                        }
                        if *dropped > 0 {
                            p.line(p.paint(
                                &format!(
                                    "… and {} more",
                                    render::plural(*dropped, "page", "pages")
                                ),
                                Ink::Dim,
                            ))?;
                        }
                        Ok(())
                    })
                })?;

                if let Some(example) = example.as_ref().filter(|_| standing.is_open()) {
                    p.blank()?;
                    p.wrapped(&format!("centinel investigate {example}"), Ink::Bold)?;
                }
                Ok(())
            }

            Self::Ruled {
                host,
                ruling,
                at,
                note,
                previous,
            } => {
                let said = match ruling {
                    Ruling::Ignore => "refused",
                    Ruling::Allow => "allowed",
                };
                p.marked(Mark::Ok, format!("{said} {}", p.paint(host, Ink::Bold)))?;
                p.nest(|p| {
                    if let Some(why) = note {
                        p.wrapped(&render::one_line(why), Ink::Dim)?;
                    }
                    if let Some(previous) = previous {
                        p.wrapped(&format!("replaces {previous}"), Ink::Dim)?;
                    }
                    // The ruling is append-only truth, so it survives `centinel.db` being
                    // deleted — and so does the record of every ruling it replaced.
                    p.wrapped(
                        &match ruling {
                            Ruling::Ignore => format!(
                                "recorded at {} · `centinel crumbs allow {host}` takes it back",
                                render::short_time(at)
                            ),
                            Ruling::Allow => format!(
                                "recorded at {} · it will be offered again",
                                render::short_time(at)
                            ),
                        },
                        Ink::Dim,
                    )
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crumbs::MAX_CARRIERS;
    use crate::domain::{Observation, Resource};
    use crate::store::Store;

    async fn ctx() -> (tempfile::TempDir, Ctx) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        (dir, Ctx::new(store))
    }

    /// One document into the store, as `collect` leaves it: an Observation in the log and
    /// a row in the ledger.
    async fn stored(ctx: &Ctx, source: &str, address: &str, bytes: &[u8], content_type: &str) {
        let (resource, obs) = observed(ctx, source, address, bytes, content_type).await;
        let kind = ContentKind::classify(&obs.meta, &bytes[..bytes.len().min(SNIFF_BYTES)]);
        Ledger::new(&ctx.store, resource.source.clone())
            .record(&resource, bytes, kind, &obs)
            .await;
    }

    /// One document into the log and **not** the ledger: a page collected before the
    /// ledger existed, or whose row failed to write.
    async fn observed(
        ctx: &Ctx,
        source: &str,
        address: &str,
        bytes: &[u8],
        content_type: &str,
    ) -> (Resource, Observation) {
        let id = SourceId::new(source.to_string()).unwrap();
        let resource = Resource::new(id, address);
        let obs = ctx
            .store
            .record_observation(
                &resource,
                bytes,
                "2026-08-01T00:00:00Z".parse().unwrap(),
                BTreeMap::from([("content-type".to_string(), content_type.to_string())]),
            )
            .await
            .unwrap();
        (resource, obs)
    }

    async fn page(ctx: &Ctx, source: &str, address: &str, html: &str) {
        stored(ctx, source, address, html.as_bytes(), "text/html").await;
    }

    async fn run(ctx: &Ctx, args: CrumbsArgs) -> CrumbsReport {
        crumbs(ctx, args, &Progress::none(), &Cancel::none())
            .await
            .unwrap()
    }

    /// Defines a source in the store, as `source add` would.
    async fn define(ctx: &Ctx, source: crate::config::SourceConfig) {
        crate::catalog::Sources::new(&ctx.store).add(&source).await.unwrap();
    }

    fn listed(report: &CrumbsReport) -> &Vec<Crumb> {
        match report {
            CrumbsReport::List { crumbs, .. } => crumbs,
            other => panic!("expected a list, got {other:?}"),
        }
    }

    /// The whole point: what the corpus points at that the corpus does not hold.
    #[tokio::test]
    async fn the_hosts_the_corpus_links_to_are_named_most_linked_first() {
        let (_d, ctx) = ctx().await;
        page(
            &ctx,
            "tampa",
            "https://www.tampa.gov/clerk",
            r#"<a href="/clerk/agendas">ours</a>
               <a href="https://publicrec.hillsclerk.com/Civil/">records</a>
               <a href="https://www.facebook.com/CityofTampa">social</a>"#,
        )
        .await;
        page(
            &ctx,
            "tampa",
            "https://www.tampa.gov/council",
            r#"<a href="https://publicrec.hillsclerk.com/Probate/">more records</a>"#,
        )
        .await;

        let report = run(&ctx, CrumbsArgs::default()).await;
        let crumbs = listed(&report);
        assert_eq!(crumbs.len(), 2);
        assert_eq!(crumbs[0].host, "publicrec.hillsclerk.com");
        assert_eq!((crumbs[0].links, crumbs[0].pages), (2, 2));
        assert_eq!(crumbs[1].host, "www.facebook.com");
        assert!(crumbs.iter().all(|c| c.standing.is_open()));

        let CrumbsReport::List {
            documents,
            unread,
            from_blobs,
            ..
        } = &report
        else {
            unreachable!()
        };
        assert_eq!((*documents, *unread), (2, 0));
        // Both pages were written down as they were stored, so nothing was read out of
        // a blob and nothing is missing.
        assert_eq!(*from_blobs, 0);
    }

    /// A page with no row is counted and named, never read: a one-time backfill of a
    /// corpus is `--rescan`'s job, not every reader's. The blob is removed from the pool
    /// to prove it — a reader that opened it would count it as `unread` instead.
    #[tokio::test]
    async fn a_page_with_no_row_is_counted_and_not_read() {
        let (_d, ctx) = ctx().await;
        page(
            &ctx,
            "tampa",
            "https://www.tampa.gov/recorded",
            r#"<a href="https://x.example.com/a">x</a>"#,
        )
        .await;
        let (_, obs) = observed(
            &ctx,
            "tampa",
            "https://www.tampa.gov/unrecorded",
            br#"<a href="https://lost.example.com/">gone</a>"#,
            "text/html",
        )
        .await;
        std::fs::remove_file(ctx.store.blob_path_of(&obs.blob_sha)).unwrap();

        let report = run(&ctx, CrumbsArgs::default()).await;
        let hosts: Vec<&str> = listed(&report).iter().map(|c| c.host.as_str()).collect();
        assert_eq!(hosts, ["x.example.com"]);
        let CrumbsReport::List {
            unrecorded,
            unread,
            from_blobs,
            documents,
            ..
        } = &report
        else {
            unreachable!()
        };
        assert_eq!((*unrecorded, *unread, *from_blobs), (1, 0, 0), "the blob was opened");
        assert_eq!(*documents, 1, "an unrecorded page is not accounted for");
    }

    /// The bug this guards: two pages with identical bytes share one blob, and a ledger
    /// keyed on the blob alone handed each of them the other's row — `show` named one page
    /// twice and the other never. Rows are keyed on the address, so each keeps its own.
    #[tokio::test]
    async fn two_pages_with_identical_bytes_keep_their_own_rows() {
        let (_d, ctx) = ctx().await;
        let same = r#"<a href="https://vendor.example.com/login">login</a>"#;
        page(&ctx, "tampa", "https://www.tampa.gov/a", same).await;
        page(&ctx, "tampa", "https://www.tampa.gov/b", same).await;

        let report = run(
            &ctx,
            CrumbsArgs {
                action: Some(CrumbsAction::Show(ShowArgs {
                    host: "vendor.example.com".into(),
                    source: None,
                })),
                ..Default::default()
            },
        )
        .await;
        let CrumbsReport::Show { carried_by, pages, .. } = &report else {
            panic!("expected a show, got {report:?}");
        };
        let addresses: Vec<&str> = carried_by.iter().map(|c| c.address.as_str()).collect();
        assert_eq!(addresses, ["https://www.tampa.gov/a", "https://www.tampa.gov/b"]);
        assert_eq!(*pages, 2);
    }

    /// Rows written before v0.23 carry the 12-character short digest. A corpus with
    /// megabytes of them must keep answering without a rescan.
    #[tokio::test]
    async fn a_row_with_a_short_digest_still_answers() {
        let (_d, ctx) = ctx().await;
        let (resource, obs) = observed(
            &ctx,
            "tampa",
            "https://www.tampa.gov/old",
            br#"<a href="https://x.example.com/a">x</a>"#,
            "text/html",
        )
        .await;
        let ledger = Ledger::new(&ctx.store, resource.source.clone());
        ledger
            .append(&crumbs::scan_page(
                r#"<a href="https://x.example.com/a">x</a>"#,
                Carrier {
                    address: resource.natural_key.clone(),
                    at: Some(obs.at.to_string()),
                    blob: Some(render::short_sha(obs.blob_sha.as_str())),
                },
            ))
            .await
            .unwrap();

        let report = run(&ctx, CrumbsArgs::default()).await;
        assert_eq!(listed(&report)[0].host, "x.example.com");
        let CrumbsReport::List { unrecorded, .. } = &report else {
            unreachable!()
        };
        assert_eq!(*unrecorded, 0);
    }

    /// A refusal is truth, so the next pass must not offer the host again — and `--all`
    /// still shows it, because a list that hides a decision cannot be reviewed.
    #[tokio::test]
    async fn a_refused_host_stops_being_offered_and_an_allow_brings_it_back() {
        let (_d, ctx) = ctx().await;
        page(
            &ctx,
            "tampa",
            "https://www.tampa.gov/x",
            r#"<a href="https://www.facebook.com/CityofTampa">social</a>"#,
        )
        .await;

        run(
            &ctx,
            CrumbsArgs {
                action: Some(CrumbsAction::Ignore(RuleArgs {
                    host: "www.facebook.com".into(),
                    note: Some("a social network".into()),
                })),
                ..Default::default()
            },
        )
        .await;

        let report = run(&ctx, CrumbsArgs::default()).await;
        assert!(listed(&report).is_empty(), "a refused host was offered");
        let CrumbsReport::List { hidden, .. } = &report else {
            unreachable!()
        };
        assert_eq!(*hidden, 1, "hidden hosts must still be counted");

        let all = run(
            &ctx,
            CrumbsArgs {
                all: true,
                ..Default::default()
            },
        )
        .await;
        assert_eq!(listed(&all)[0].standing, Standing::Ignored);

        run(
            &ctx,
            CrumbsArgs {
                action: Some(CrumbsAction::Allow(RuleArgs {
                    host: "https://www.facebook.com/CityofTampa".into(),
                    note: None,
                })),
                ..Default::default()
            },
        )
        .await;
        assert_eq!(
            listed(&run(&ctx, CrumbsArgs::default()).await).len(),
            1,
            "an allow must reverse the refusal"
        );
    }

    /// A ruling names what it replaced, so `ignore` after an `allow` reads differently from
    /// `ignore` twice.
    #[tokio::test]
    async fn a_ruling_reports_the_one_it_replaced() {
        let (_d, ctx) = ctx().await;
        let ignore = |host: &str| CrumbsArgs {
            action: Some(CrumbsAction::Ignore(RuleArgs {
                host: host.into(),
                note: None,
            })),
            ..Default::default()
        };

        let first = run(&ctx, ignore("fonts.googleapis.com")).await;
        let CrumbsReport::Ruled { previous, host, .. } = &first else {
            panic!("expected a ruling");
        };
        assert_eq!(host, "fonts.googleapis.com");
        assert!(previous.is_none());

        let second = run(&ctx, ignore("fonts.googleapis.com")).await;
        let CrumbsReport::Ruled { previous, .. } = &second else {
            unreachable!()
        };
        assert!(
            previous.as_deref().is_some_and(|p| p.contains("ignore")),
            "{previous:?}"
        );
    }

    /// A definition answers the question on its own, before a single page of it has been
    /// collected. `source add` therefore needs no crumb-related code at all: standing is
    /// derived on every read, so the crumb stops being a candidate the moment the definition
    /// exists and there is nothing to keep in step.
    #[tokio::test]
    async fn a_host_a_definition_already_claims_is_not_offered_as_a_candidate() {
        let (_d, ctx) = ctx().await;
        page(
            &ctx,
            "tampa",
            "https://www.tampa.gov/x",
            r#"<a href="https://publicrec.hillsclerk.com/Civil/">records</a>
               <a href="https://www.facebook.com/CityofTampa">social</a>"#,
        )
        .await;

        let before = run(&ctx, CrumbsArgs::default()).await;
        assert!(
            listed(&before).iter().all(|c| c.standing.is_open()),
            "nothing claims either host yet"
        );

        // What `centinel source add hillsclerk --site … --disabled` writes. Nothing has been
        // collected from it, and disabled on top of that: the operator decided twice.
        define(
            &ctx,
            crate::config::SourceConfig {
                enabled: Some(false),
                ..crate::config::SourceConfig::site("hillsclerk", "https://publicrec.hillsclerk.com/Civil/")
            },
        )
        .await;
        let after = run(&ctx, CrumbsArgs::default()).await;

        let promoted: Vec<&Crumb> = listed(&after)
            .iter()
            .filter(|c| c.standing == Standing::Promoted)
            .collect();
        assert_eq!(promoted.len(), 1, "{:?}", listed(&after));
        assert_eq!(promoted[0].host, "publicrec.hillsclerk.com");
    }

    /// A channel is not a host. One source for `@CityofTampa` says nothing about a crumb
    /// pointing at a different channel, and marking the host answered would hide it.
    #[tokio::test]
    async fn a_channel_source_does_not_answer_for_its_whole_host() {
        let (_d, ctx) = ctx().await;
        page(
            &ctx,
            "tampa",
            "https://www.tampa.gov/x",
            r#"<a href="https://www.youtube.com/@CityofTampa">watch</a>"#,
        )
        .await;

        define(
            &ctx,
            crate::config::SourceConfig::channel("tampa-tv", "https://www.youtube.com/@CityofTampa"),
        )
        .await;
        let report = run(&ctx, CrumbsArgs::default()).await;

        let crumbs = listed(&report);
        assert_eq!(crumbs[0].host, "www.youtube.com");
        assert!(
            crumbs[0].standing.is_open(),
            "a channel claimed the whole of youtube.com"
        );
    }

    /// The promotion already happened, so the crumb is not one — whatever an old ruling
    /// says about it.
    #[tokio::test]
    async fn a_host_this_store_already_collects_is_not_offered_as_a_candidate() {
        let (_d, ctx) = ctx().await;
        page(
            &ctx,
            "tampa",
            "https://www.tampa.gov/x",
            r#"<a href="https://publicrec.hillsclerk.com/Civil/">records</a>"#,
        )
        .await;
        page(
            &ctx,
            "hillsclerk",
            "https://publicrec.hillsclerk.com/Civil/",
            "<p>collected</p>",
        )
        .await;

        let report = run(&ctx, CrumbsArgs::default()).await;
        let crumbs = listed(&report);
        assert_eq!(crumbs[0].host, "publicrec.hillsclerk.com");
        assert_eq!(crumbs[0].standing, Standing::Promoted);

        // And the other direction: the second source's own page holds no off-host link.
        let one = run(
            &ctx,
            CrumbsArgs {
                source: Some("hillsclerk".into()),
                ..Default::default()
            },
        )
        .await;
        assert!(listed(&one).is_empty());
        let CrumbsReport::List { sources, .. } = &one else {
            unreachable!()
        };
        assert_eq!(sources, &["hillsclerk"]);
    }

    /// `--source` narrows which pages are read. A typo must say so rather than answer
    /// "no crumbs", which is a wrong answer to a question nobody asked.
    #[tokio::test]
    async fn an_unknown_source_names_what_the_store_holds() {
        let (_d, ctx) = ctx().await;
        page(&ctx, "tampa", "https://www.tampa.gov/x", "<p>x</p>").await;

        let err = crumbs(
            &ctx,
            CrumbsArgs {
                source: Some("orlando".into()),
                ..Default::default()
            },
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(err.contains("orlando") && err.contains("tampa"), "{err}");
    }

    #[tokio::test]
    async fn show_names_the_pages_that_carried_it_and_counts_the_rest() {
        let (_d, ctx) = ctx().await;
        for i in 0..MAX_CARRIERS + 2 {
            page(
                &ctx,
                "tampa",
                &format!("https://www.tampa.gov/page/{i}"),
                r#"<a href="https://vendor.example.com/login">login</a>"#,
            )
            .await;
        }

        let report = run(
            &ctx,
            CrumbsArgs {
                action: Some(CrumbsAction::Show(ShowArgs {
                    host: "vendor.example.com".into(),
                    source: None,
                })),
                ..Default::default()
            },
        )
        .await;

        let CrumbsReport::Show {
            links,
            pages,
            carried_by,
            dropped,
            example,
            ..
        } = &report
        else {
            panic!("expected a show, got {report:?}");
        };
        assert_eq!(*links, MAX_CARRIERS + 2);
        assert_eq!(*pages, MAX_CARRIERS + 2);
        assert_eq!(carried_by.len(), MAX_CARRIERS);
        assert_eq!(*dropped, 2);
        assert!(carried_by[0].blob.is_some(), "the page must be openable");
        assert_eq!(example.as_deref(), Some("https://vendor.example.com/login"));
    }

    /// Nothing links there. A fact, not a fault — and the report has to be readable.
    #[tokio::test]
    async fn show_on_a_host_nothing_links_to_is_an_empty_answer() {
        let (_d, ctx) = ctx().await;
        page(&ctx, "tampa", "https://www.tampa.gov/x", "<p>x</p>").await;

        let report = run(
            &ctx,
            CrumbsArgs {
                action: Some(CrumbsAction::Show(ShowArgs {
                    host: "nobody.example.com".into(),
                    source: None,
                })),
                ..Default::default()
            },
        )
        .await;
        let CrumbsReport::Show { links, example, .. } = &report else {
            unreachable!()
        };
        assert_eq!(*links, 0);
        assert!(example.is_none());
    }

    /// On a rescan, a blob a PDF is stored under must not be scanned for links, and a page
    /// whose blob has gone must not fail the whole pass.
    #[tokio::test]
    async fn a_rescan_scans_only_html_and_counts_an_unreadable_blob() {
        let (_d, ctx) = ctx().await;
        observed(
            &ctx,
            "tampa",
            "https://www.tampa.gov/page",
            br#"<a href="https://x.example.com/a">x</a>"#,
            "text/html",
        )
        .await;
        // A PDF whose bytes happen to contain something that looks like markup.
        observed(
            &ctx,
            "tampa",
            "https://www.tampa.gov/a.pdf",
            b"%PDF-1.7\n<a href=\"https://pdf.example.com/\">not a page</a>",
            "application/pdf",
        )
        .await;
        // And an Observation whose blob has been removed from the pool.
        let (_, obs) = observed(
            &ctx,
            "tampa",
            "https://www.tampa.gov/lost",
            b"<a href=\"https://lost.example.com/\">gone</a>",
            "text/html",
        )
        .await;
        std::fs::remove_file(ctx.store.blob_path_of(&obs.blob_sha)).unwrap();

        let report = run(
            &ctx,
            CrumbsArgs {
                rescan: true,
                ..Default::default()
            },
        )
        .await;
        let hosts: Vec<&str> = listed(&report).iter().map(|c| c.host.as_str()).collect();
        assert_eq!(hosts, ["x.example.com"]);

        let CrumbsReport::List {
            unread, documents, ..
        } = &report
        else {
            unreachable!()
        };
        assert_eq!(*unread, 1, "a missing blob must be counted, not fatal");
        // The page and the PDF are accounted for; the blob that has gone is not, because
        // nothing could say whether it held links.
        assert_eq!(*documents, 2);
    }

    /// The two feeds are one scan: the row `collect` writes as it stores a page and the row
    /// `--rescan` reads out of the blob later must say the same thing, and a difference
    /// between them would be invisible in either alone. A pre-ledger corpus answers nothing
    /// until the rescan, and says so.
    #[tokio::test]
    async fn a_rescan_writes_what_collect_would_have_and_a_recorded_page_reads_the_same() {
        let (_d, ctx) = ctx().await;
        let html = r#"<a href="https://publicrec.hillsclerk.com/Probate/">probate</a>
               <a href="https://publicrec.hillsclerk.com/Civil/">civil</a>
               <a href="/ours">ours</a>"#;
        observed(&ctx, "tampa", "https://www.tampa.gov/clerk", html.as_bytes(), "text/html").await;

        // A pre-ledger corpus: nothing is read, and the report says what is missing.
        let cold = run(&ctx, CrumbsArgs::default()).await;
        assert!(listed(&cold).is_empty());
        let CrumbsReport::List { unrecorded, .. } = &cold else {
            unreachable!()
        };
        assert_eq!(*unrecorded, 1);

        // `--rescan` writes the ledger the way `collect` would have.
        let written = run(
            &ctx,
            CrumbsArgs {
                rescan: true,
                ..Default::default()
            },
        )
        .await;
        let CrumbsReport::List { from_blobs, .. } = &written else {
            unreachable!()
        };
        assert_eq!(*from_blobs, 1);
        assert!(
            ctx.store
                .crumbs_path(&SourceId::new("tampa".to_string()).unwrap())
                .exists(),
            "a rescan must write the ledger down"
        );

        // And now the same answer for no blob reads at all.
        let warm = run(&ctx, CrumbsArgs::default()).await;
        let CrumbsReport::List {
            from_blobs,
            unrecorded,
            documents,
            ..
        } = &warm
        else {
            unreachable!()
        };
        assert_eq!((*from_blobs, *unrecorded, *documents), (0, 0, 1));
        assert_eq!(listed(&warm), listed(&written), "the two feeds disagree");

        // The same page stored by `collect` — the row written from bytes in hand — reads
        // the same as the row the rescan wrote from the blob.
        let (_d2, fresh) = self::ctx().await;
        page(&fresh, "tampa", "https://www.tampa.gov/clerk", html).await;
        assert_eq!(listed(&run(&fresh, CrumbsArgs::default()).await), listed(&warm));

        // The example is the smallest address on the host, so it does not depend on which
        // feed answered or on what order the pages arrived in.
        assert_eq!(
            listed(&warm)[0].example,
            "https://publicrec.hillsclerk.com/Civil/"
        );
    }

    /// A row whose blob is no longer the newest for that page must not answer for it: the
    /// site's links have changed, and the old row would report the ones it dropped.
    #[tokio::test]
    async fn a_superseded_row_does_not_answer_for_a_changed_page() {
        let (_d, ctx) = ctx().await;
        let url = "https://www.tampa.gov/clerk";
        page(
            &ctx,
            "tampa",
            url,
            r#"<a href="https://old.example.com/">old</a>"#,
        )
        .await;
        run(
            &ctx,
            CrumbsArgs {
                rescan: true,
                ..Default::default()
            },
        )
        .await;

        // The page changes: a new blob, and no row for it yet.
        let resource = Resource::new(SourceId::new("tampa".to_string()).unwrap(), url);
        let obs = ctx
            .store
            .record_observation(
                &resource,
                br#"<a href="https://new.example.com/">new</a>"#,
                "2026-08-02T00:00:00Z".parse().unwrap(),
                BTreeMap::from([("content-type".to_string(), "text/html".to_string())]),
            )
            .await
            .unwrap();

        let report = run(&ctx, CrumbsArgs::default()).await;
        assert!(listed(&report).is_empty(), "a stale row answered: {report:?}");
        let CrumbsReport::List { unrecorded, .. } = &report else {
            unreachable!()
        };
        assert_eq!(*unrecorded, 1, "the changed page has no current row");

        // Written down as `collect` would, the new row answers and the old one does not.
        Ledger::new(&ctx.store, resource.source.clone())
            .record(&resource, br#"<a href="https://new.example.com/">new</a>"#, ContentKind::Html, &obs)
            .await;
        let hosts: Vec<String> = listed(&run(&ctx, CrumbsArgs::default()).await)
            .iter()
            .map(|c| c.host.clone())
            .collect();
        assert_eq!(hosts, ["new.example.com"]);
    }

    #[test]
    fn a_host_is_taken_from_a_host_or_from_a_url_on_it() {
        assert_eq!(as_host("Facebook.com").unwrap(), "facebook.com");
        assert_eq!(as_host("  facebook.com/  ").unwrap(), "facebook.com");
        assert_eq!(
            as_host("https://www.facebook.com/CityofTampa?x=1").unwrap(),
            "www.facebook.com"
        );

        for bad in ["", "mailto:clerk@tampa.gov", "two hosts", "a/b", "https://"] {
            assert!(as_host(bad).is_err(), "`{bad}` was accepted as a host");
        }
    }
}
