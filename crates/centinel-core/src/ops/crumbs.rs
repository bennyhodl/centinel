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
//! there, immutable, and says the same thing. So the pass falls through to it, counts how
//! often it had to, and names the one command that stops the cost. A slow answer is a fine
//! failure mode; a quietly incomplete one is not, which is why the count is on the report
//! rather than in a log line.
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

    /// Config file to read the `[[source]]` list from. Defaults to the usual search path.
    #[arg(long, value_name = "FILE")]
    #[serde(default)]
    pub config: Option<String>,
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
            config: None,
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
        /// Pages that had no ledger row, so their blob was read instead. Zero on a corpus
        /// collected by this build; the whole page count on one collected before it.
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
        args.config.as_deref(),
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
    let field = survey(ctx, source, args.config.as_deref(), args.rescan, p, cancel).await?;

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
    /// Pages whose links had to be read out of a blob because no row recorded them.
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

/// The hosts the operator's own `[[source]]` blocks claim.
///
/// The **intent** half of "already a Source"; [`survey`] adds the evidence half out of the
/// logs it is reading anyway. A block for a host nothing has collected yet still answers the
/// question — offering it back would ask the operator to decide something they decided an hour
/// ago — and one that is `enabled = false` answers it too: they chose the host and then chose
/// not to run it, and neither of those decisions was "ask me again".
///
/// **A channel claims no host.** `youtube.com` holds every channel there is, so a source for
/// one of them says nothing about a crumb pointing at another. Marking the host answered would
/// hide it; leaving it open costs one `crumbs ignore youtube.com` and hides nothing. The config
/// knows a channel by which field is set, and the store by
/// [`crate::sources::channel::claims`].
fn claimed_hosts(config: &crate::config::Config) -> BTreeSet<String> {
    config
        .sources
        .iter()
        .filter_map(|s| s.site.as_deref().and_then(host_of))
        .collect()
}

/// Every page of the selected Sources, from the ledger where there is a row and from the
/// blob where there is not.
///
/// **The ledger is the fast path and the blobs are the floor.** `collect` writes a row per
/// stored artifact, so a corpus collected by this build answers out of one file per Source.
/// A row is missing for exactly three reasons — the page predates the ledger, a write failed,
/// or somebody deleted the file — and in all three the blob is still there, immutable, and
/// says the same thing. So a missing row costs a read rather than an answer, and the count of
/// them is reported: a number that quietly falls short is the failure this codebase is most
/// consistently written against.
///
/// Whether a host is **already a Source** is a corpus-wide question, so it is asked of the
/// config and of every log rather than only of the selected Sources — a crumb Tampa dropped
/// may be a Source somebody added last week. See [`claimed_hosts`] for the config half.
async fn survey(
    ctx: &Ctx,
    source: Option<&str>,
    config_path: Option<&str>,
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
    let (config, _) = super::load_config(config_path)?;
    let mut promoted = claimed_hosts(&config);

    // One replay per Source and one loop, because both questions are answered from it: what
    // every Source has already collected, and what the selected ones have to say.
    let mut work: Vec<(SourceId, Vec<Observation>)> = Vec::new();
    for id in &known {
        let replay = ctx.store.replay(id).await?;

        let observed: Vec<&str> = replay.observed().into_iter().collect();
        if !crate::sources::channel::claims(replay.discovery_method(), &observed) {
            promoted.extend(observed.iter().filter_map(|key| host_of(key)));
        }

        if selected.contains(id) {
            // The newest Observation per Resource, never the history: one page holds one set
            // of links, and an older version of it holds the links the site has since dropped.
            let mut pages: Vec<Observation> = replay.latest_observations().into_values().collect();
            // Deterministic order, so two passes over one store read the same.
            pages.sort_by(|a, b| a.resource.natural_key.cmp(&b.resource.natural_key));
            work.push((id.clone(), pages));
        }
    }

    let total: u64 = work.iter().map(|(_, pages)| pages.len() as u64).sum();
    let mut trail = Trail::default();
    let mut done = 0u64;
    let mut from_blobs = 0;
    let mut unread = 0;

    for (id, pages) in &work {
        let ledger = Ledger::new(&ctx.store, id.clone());
        // Keyed on the blob rather than the address: a page re-collected with new content has
        // a new blob, so its old row is superseded rather than current, and reading it would
        // report links the site has since dropped.
        let recorded: BTreeMap<String, Recorded> = match rescan {
            true => BTreeMap::new(),
            false => ledger
                .read()
                .await?
                .into_iter()
                .filter_map(|row| row.page.blob.clone().map(|blob| (blob, row)))
                .collect(),
        };

        let mut rows: Vec<Recorded> = Vec::with_capacity(pages.len());
        for obs in pages {
            cancel.check()?;
            p.step(obs.resource.natural_key.clone(), done, total);
            done += 1;

            let short = render::short_sha(obs.blob_sha.as_str());
            if let Some(row) = recorded.get(&short) {
                trail.absorb(row);
                rows.push(row.clone());
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
        blob: Some(render::short_sha(obs.blob_sha.as_str())),
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

/// The host of an address, or `None` for one that does not name one.
fn host_of(address: &str) -> Option<String> {
    url::Url::parse(address)
        .ok()?
        .host_str()
        .map(str::to_ascii_lowercase)
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
                from_blobs,
                unread,
                rescanned,
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
                    // What it cost, and how to stop paying it. `collect` writes a row per
                    // page, so a non-zero count here is a backlog — pages collected before
                    // the ledger existed — and it is a one-command fix.
                    match (*from_blobs, *rescanned) {
                        (0, _) => Ok(()),
                        (n, true) => p.note(format!(
                            "{} read from blobs and written down",
                            render::plural(n, "page", "pages")
                        )),
                        (n, false) => p.note(format!(
                            "{} had no record and were read from blobs — \
                             `centinel crumbs --rescan` writes them down once",
                            render::plural(n, "page", "pages")
                        )),
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
                                aside.push_str(blob);
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
    use crate::domain::Resource;
    use crate::store::Store;

    async fn ctx() -> (tempfile::TempDir, Ctx) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        (dir, Ctx::new(store))
    }

    /// One document into the store, as `collect` would leave it.
    async fn stored(ctx: &Ctx, source: &str, address: &str, bytes: &[u8], content_type: &str) {
        let id = SourceId::new(source.to_string()).unwrap();
        let resource = Resource::new(id, address);
        ctx.store
            .record_observation(
                &resource,
                bytes,
                "2026-08-01T00:00:00Z".parse().unwrap(),
                BTreeMap::from([("content-type".to_string(), content_type.to_string())]),
            )
            .await
            .unwrap();
    }

    async fn page(ctx: &Ctx, source: &str, address: &str, html: &str) {
        stored(ctx, source, address, html.as_bytes(), "text/html").await;
    }

    /// Runs the op against **this** store's config and never the machine's.
    ///
    /// Without the substitution `Config::locate` would find `./centinel.toml` or
    /// `~/.centinel/centinel.toml`, and every assertion about which hosts are already Sources
    /// would depend on whose laptop ran it. A test that reads the machine is not a test of the
    /// code.
    async fn run(ctx: &Ctx, args: CrumbsArgs) -> CrumbsReport {
        let args = match args.config {
            Some(_) => args,
            None => CrumbsArgs {
                config: Some(config_with(ctx, "")),
                ..args
            },
        };
        crumbs(ctx, args, &Progress::none(), &Cancel::none())
            .await
            .unwrap()
    }

    /// A config file inside the store, holding whatever the test wants to say.
    fn config_with(ctx: &Ctx, body: &str) -> String {
        let path = ctx.store.root().join("centinel.toml");
        std::fs::write(&path, body).unwrap();
        path.display().to_string()
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
        // Both pages were stored by `record_observation` directly, so no ledger row exists
        // and the blobs are the floor that answered. This is the pre-ledger corpus case.
        assert_eq!(*from_blobs, 2);
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

    /// A `[[source]]` block answers the question on its own, before a single page of it has
    /// been collected. `source add` therefore needs no crumb-related code at all: standing is
    /// derived on every read, so the crumb stops being a candidate the moment the block exists
    /// and there is nothing to keep in step.
    #[tokio::test]
    async fn a_host_the_config_already_claims_is_not_offered_as_a_candidate() {
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

        // What `centinel source add hillsclerk --site …` writes. Nothing has been collected
        // from it, and `enabled = false` on top of that: the operator decided twice.
        let config = config_with(
            &ctx,
            "[[source]]\nid = \"hillsclerk\"\n\
             site = \"https://publicrec.hillsclerk.com/Civil/\"\nenabled = false\n",
        );
        let after = run(
            &ctx,
            CrumbsArgs {
                config: Some(config),
                ..Default::default()
            },
        )
        .await;

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

        let config = config_with(
            &ctx,
            "[[source]]\nid = \"tampa-tv\"\n\
             channel = \"https://www.youtube.com/@CityofTampa\"\n",
        );
        let report = run(
            &ctx,
            CrumbsArgs {
                config: Some(config),
                ..Default::default()
            },
        )
        .await;

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

    /// A blob a PDF is stored under must not be scanned for links, and a page whose blob has
    /// gone must not fail the whole pass.
    #[tokio::test]
    async fn only_html_is_scanned_and_an_unreadable_blob_is_counted() {
        let (_d, ctx) = ctx().await;
        page(
            &ctx,
            "tampa",
            "https://www.tampa.gov/page",
            r#"<a href="https://x.example.com/a">x</a>"#,
        )
        .await;
        // A PDF whose bytes happen to contain something that looks like markup.
        stored(
            &ctx,
            "tampa",
            "https://www.tampa.gov/a.pdf",
            b"%PDF-1.7\n<a href=\"https://pdf.example.com/\">not a page</a>",
            "application/pdf",
        )
        .await;

        // And an Observation whose blob has been removed from the pool.
        let resource = Resource::new(
            SourceId::new("tampa".to_string()).unwrap(),
            "https://www.tampa.gov/lost",
        );
        let obs = ctx
            .store
            .record_observation(
                &resource,
                b"<a href=\"https://lost.example.com/\">gone</a>",
                "2026-08-01T00:00:00Z".parse().unwrap(),
                BTreeMap::new(),
            )
            .await
            .unwrap();
        std::fs::remove_file(ctx.store.blob_path_of(&obs.blob_sha)).unwrap();

        let report = run(&ctx, CrumbsArgs::default()).await;
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

    /// The point of the ledger: a pass over a recorded corpus opens no blobs at all.
    ///
    /// Written here rather than only in `acquire`, because what has to hold is that the two
    /// feeds produce **the same answer** — the row `collect` writes and the blob `--rescan`
    /// reads are one scan, and a difference between them would be invisible in either alone.
    #[tokio::test]
    async fn a_recorded_page_costs_no_blob_read_and_says_what_a_rescan_says() {
        let (_d, ctx) = ctx().await;
        page(
            &ctx,
            "tampa",
            "https://www.tampa.gov/clerk",
            r#"<a href="https://publicrec.hillsclerk.com/Probate/">probate</a>
               <a href="https://publicrec.hillsclerk.com/Civil/">civil</a>
               <a href="/ours">ours</a>"#,
        )
        .await;

        // A pre-ledger corpus: the answer comes from blobs, and says so.
        let cold = run(&ctx, CrumbsArgs::default()).await;
        let CrumbsReport::List { from_blobs, .. } = &cold else {
            unreachable!()
        };
        assert_eq!(*from_blobs, 1);

        // `--rescan` writes the ledger the way `collect` would have.
        let written = run(
            &ctx,
            CrumbsArgs {
                rescan: true,
                ..Default::default()
            },
        )
        .await;
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
            documents,
            ..
        } = &warm
        else {
            unreachable!()
        };
        assert_eq!(*from_blobs, 0, "a recorded page was read from its blob");
        assert_eq!(*documents, 1);
        assert_eq!(listed(&warm), listed(&written));
        assert_eq!(listed(&warm), listed(&cold), "the two feeds disagree");

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

        // The page changes: a new blob, and no row for it.
        let resource = Resource::new(SourceId::new("tampa".to_string()).unwrap(), url);
        ctx.store
            .record_observation(
                &resource,
                br#"<a href="https://new.example.com/">new</a>"#,
                "2026-08-02T00:00:00Z".parse().unwrap(),
                BTreeMap::from([("content-type".to_string(), "text/html".to_string())]),
            )
            .await
            .unwrap();

        let report = run(&ctx, CrumbsArgs::default()).await;
        let hosts: Vec<&str> = listed(&report).iter().map(|c| c.host.as_str()).collect();
        assert_eq!(hosts, ["new.example.com"], "a stale row answered");

        let CrumbsReport::List { from_blobs, .. } = &report else {
            unreachable!()
        };
        assert_eq!(*from_blobs, 1, "the new blob had to be read");
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
