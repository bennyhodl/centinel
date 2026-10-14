//! The graph of sources and the hosts their pages link to.
//!
//! A map of where the corpus points, weighted by how many **pages** carry each link rather
//! than how many links there are. A shared footer puts the same host on every page of a
//! site — `www.tampaconnect.com` is 198,470 links on 11,442 pages of one city — so the
//! link count says how a template is built and the page count says how much of the site
//! depends on the host. The second is the one worth drawing.
//!
//! ## Derived from the ledgers, and nothing else
//!
//! The crumb ledger (`crumbs/<source>.jsonl`, see [`crate::crumbs::Ledger`]) already holds a
//! row per collected page naming each off-host host it links to. The graph is a fold over
//! those rows: per source, per host, pages and links. It opens no blob and replays no log,
//! which is what makes it a page a person can leave open rather than a report they run.
//!
//! The one thing the ledger alone cannot say is whether a host is **already a source**,
//! which is a question about the catalogue and every source's [`crate::summary::Summary`].
//! A host a source claims — its own site host, or any host its log has observed — is not
//! drawn as a host at all: the edge lands on that source, so two sources that link to each
//! other are connected rather than each pointing at a copy of the other's name.
//!
//! ## Cached by the ledger's size, like a summary by its log's
//!
//! A city's ledger is megabytes, and sixteen of them is a read the graph page would make
//! on every filter change. [`Linkage`] is one source's fold, written beside its summary
//! and stamped with the ledger's length and modification time; while the stamp matches
//! the file, the fold is current. It is the same pattern as [`crate::summary::Summaries`],
//! for the same reason: the ledger changes once per run, and the page asks far more often
//! than that. Derived, and safe to delete.
//!
//! The graph trusts the ledger's newest row per address. [`crate::ops::crumbs`] goes one
//! step further and checks each row against the blob the log says is current, because it
//! names pages as evidence; a map can afford one changed page's links being a run stale,
//! and `crumbs --rescan` compacts the ledger to exactly the current rows anyway.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::crumbs::{self, Decisions, Ledger, Recorded, Ruling, Standing, Trail};
use crate::domain::{SourceId, SourceKind};
use crate::store::Store;

/// Edges drawn before the rest are cut. Strongest first, so what is dropped is the long
/// tail of hosts one page mentioned, and the answer says it was cut.
pub const MAX_EDGES: usize = 4_000;
/// Paths named on a host's card before the rest are counted.
pub const MAX_PATHS: usize = 25;
/// Referring pages named on a host's card, across every source.
pub const MAX_REFERRERS: usize = 24;

// ── one source's ledger, added up ─────────────────────────────────────────────

/// The ledger file a [`Linkage`] was folded from, as its length and modification time.
///
/// Length alone is what the summary keys on, and for an append-only log it is enough.
/// A ledger is rewritten by `crumbs --rescan`, and a rewrite that happens to land on the
/// same length would otherwise be believed; the time closes that.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedgerStamp {
    pub len: u64,
    /// Milliseconds since the epoch, or zero when the file does not exist.
    pub modified_ms: i128,
}

/// What one source's pages point at, per host.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Linkage {
    /// The ledger this was folded from. It answers for the ledger while they still match.
    pub ledger: LedgerStamp,
    /// Pages with a row in the ledger, links or not.
    pub pages: usize,
    pub hosts: BTreeMap<String, Weight>,
}

/// How strongly one source points at one host.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Weight {
    /// Distinct pages carrying at least one link there.
    pub pages: usize,
    /// Link occurrences, summed.
    pub links: usize,
    /// The smallest address seen on the host, to look at.
    pub example: String,
}

impl Linkage {
    /// Folds a ledger's rows — the newest per address — through the same [`Trail`] the
    /// `crumbs` command uses, so the two never count a page differently.
    pub fn of<'a>(rows: impl IntoIterator<Item = &'a Recorded>, ledger: LedgerStamp) -> Self {
        let mut trail = Trail::default();
        for row in rows {
            trail.absorb(row);
        }
        Self {
            ledger,
            pages: trail.documents_read(),
            hosts: trail
                .crumbs()
                .into_iter()
                .map(|crumb| {
                    (
                        crumb.host,
                        Weight {
                            pages: crumb.pages,
                            links: crumb.links,
                            example: crumb.example,
                        },
                    )
                })
                .collect(),
        }
    }
}

/// Reads and rebuilds linkages. Thin, like [`crate::summary::Summaries`]: the paths belong
/// to [`Store`], and everything here is one `stat`, one small read, and when the ledger
/// has moved, one fold.
pub struct Linkages<'a> {
    store: &'a Store,
}

impl<'a> Linkages<'a> {
    pub fn new(store: &'a Store) -> Self {
        Self { store }
    }

    /// The linkage for this source's ledger as it is now. A source with no ledger has an
    /// empty one, which is an answer rather than an error: nothing has been written down.
    pub async fn get(&self, source: &SourceId) -> anyhow::Result<Linkage> {
        let stamp = self.stamp(source).await?;
        crate::summary::cached(
            &self.store.linkage_path(source),
            |linkage: &Linkage| linkage.ledger == stamp,
            async {
                let rows = Ledger::new(self.store, source.clone()).current().await?;
                Ok(Linkage::of(rows.values(), stamp))
            },
        )
        .await
    }

    async fn stamp(&self, source: &SourceId) -> anyhow::Result<LedgerStamp> {
        let meta = match tokio::fs::metadata(self.store.crumbs_path(source)).await {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(LedgerStamp::default()),
            Err(e) => return Err(e.into()),
        };
        let modified_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i128)
            .unwrap_or_default();
        Ok(LedgerStamp {
            len: meta.len(),
            modified_ms,
        })
    }
}

// ── the graph ─────────────────────────────────────────────────────────────────

/// A node: a source the store defines, or a host its pages link to that no source claims.
///
/// `id` is the handle an edge names, and it says which it is: `source:<id>` or
/// `host:<host>`, so a renderer can tell the two apart from the string alone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Node {
    Source {
        id: String,
        source: String,
        acquisition: SourceKind,
        /// The address the definition names.
        target: String,
        /// The host it collects. A channel has none: `youtube.com` is not its.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        host: Option<String>,
        enabled: bool,
        /// Addresses its log knows, which is what the Sources page reports too.
        addresses: usize,
    },
    Host {
        id: String,
        host: String,
        /// Pages linking there, over every source.
        pages: usize,
        links: usize,
        /// How many sources link there.
        sources: usize,
        /// Open or refused. Never promoted: a claimed host is drawn as its source.
        standing: Standing,
    },
}

impl Node {
    pub fn id(&self) -> &str {
        match self {
            Self::Source { id, .. } | Self::Host { id, .. } => id,
        }
    }

    fn source_id(source: &str) -> String {
        format!("source:{source}")
    }

    fn host_id(host: &str) -> String {
        format!("host:{host}")
    }
}

/// One source's pages pointing at one node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub pages: usize,
    pub links: usize,
}

/// The whole graph's numbers, before any filter: so a filtered view can say what it left out.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Totals {
    pub sources: usize,
    pub hosts: usize,
    pub edges: usize,
    pub links: usize,
    pub open: usize,
    pub refused: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub totals: Totals,
    /// Whether edges past [`MAX_EDGES`] were dropped.
    pub truncated: bool,
}

/// What a reader wants left out.
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct Filter {
    /// Edges carried by fewer pages are dropped, and so is a host no edge reaches.
    #[serde(default = "one")]
    pub min_pages: usize,
    #[serde(default)]
    pub hide_refused: bool,
}

fn one() -> usize {
    1
}

impl Default for Filter {
    fn default() -> Self {
        Self {
            min_pages: 1,
            hide_refused: false,
        }
    }
}

impl Graph {
    /// The whole graph, from every source's linkage and the operator's rulings.
    pub async fn read(store: &Store) -> anyhow::Result<Self> {
        let defined = crate::catalog::Sources::new(store).list().await?;
        let claimants = crumbs::claimants(store).await?;
        let summaries = crate::summary::Summaries::new(store);
        let linkages = Linkages::new(store);
        let decisions = Decisions::new(store).current().await?;

        let mut nodes = Vec::with_capacity(defined.len());
        // Keyed by (from, to), because two hosts one source claims — `tampa.gov` and
        // `www.tampa.gov` — are one edge to that source.
        let mut edges: BTreeMap<(String, String), Edge> = BTreeMap::new();
        let mut hosts: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new();

        for source in &defined {
            let id = SourceId::new(source.id.clone())?;
            let acquisition = source.acquisition()?;
            nodes.push(Node::Source {
                id: Node::source_id(&source.id),
                source: source.id.clone(),
                acquisition: acquisition.kind(),
                target: acquisition.target().to_string(),
                host: source.site_host(),
                enabled: source.is_enabled(),
                addresses: summaries.get(&id).await?.resources,
            });

            let from = Node::source_id(&source.id);
            for (host, weight) in linkages.get(&id).await?.hosts {
                let targets: Vec<String> = match claimants.get(&host) {
                    // A link into its own host is `enumerate`'s business, not an edge.
                    Some(owners) => owners
                        .iter()
                        .filter(|owner| owner.as_str() != source.id)
                        .map(|owner| Node::source_id(owner.as_str()))
                        .collect(),
                    None => {
                        let tally = hosts.entry(host.clone()).or_default();
                        tally.0 += weight.pages;
                        tally.1 += weight.links;
                        tally.2 += 1;
                        vec![Node::host_id(&host)]
                    }
                };
                for to in targets {
                    let edge = edges.entry((from.clone(), to.clone())).or_insert(Edge {
                        from: from.clone(),
                        to,
                        pages: 0,
                        links: 0,
                    });
                    edge.pages += weight.pages;
                    edge.links += weight.links;
                }
            }
        }

        let mut host_nodes: Vec<Node> = hosts
            .into_iter()
            .map(|(host, (pages, links, sources))| Node::Host {
                id: Node::host_id(&host),
                standing: match decisions.get(&host).map(|d| d.ruling) {
                    Some(Ruling::Ignore) => Standing::Ignored,
                    _ => Standing::Open,
                },
                host,
                pages,
                links,
                sources,
            })
            .collect();
        // Busiest first, and a stable order after that, so two reads draw the same.
        host_nodes.sort_by(|a, b| match (a, b) {
            (Node::Host { pages: p, host: h, .. }, Node::Host { pages: q, host: g, .. }) => {
                q.cmp(p).then(h.cmp(g))
            }
            _ => std::cmp::Ordering::Equal,
        });
        let mut edges: Vec<Edge> = edges.into_values().collect();
        edges.sort_by(|a, b| {
            b.pages
                .cmp(&a.pages)
                .then(a.from.cmp(&b.from))
                .then(a.to.cmp(&b.to))
        });

        let totals = Totals {
            sources: defined.len(),
            hosts: host_nodes.len(),
            edges: edges.len(),
            links: edges.iter().map(|e| e.links).sum(),
            open: host_nodes
                .iter()
                .filter(|n| matches!(n, Node::Host { standing: Standing::Open, .. }))
                .count(),
            refused: host_nodes
                .iter()
                .filter(|n| matches!(n, Node::Host { standing: Standing::Ignored, .. }))
                .count(),
        };
        nodes.extend(host_nodes);
        Ok(Self {
            nodes,
            edges,
            totals,
            truncated: false,
        })
    }

    /// The graph a reader asked for. Sources always stay; a host stays while an edge
    /// reaches it. The totals are the whole graph's, so the view can say what it hides.
    pub fn filtered(mut self, filter: &Filter) -> Self {
        let refused: BTreeSet<&str> = self
            .nodes
            .iter()
            .filter(|n| matches!(n, Node::Host { standing: Standing::Ignored, .. }))
            .map(Node::id)
            .collect();
        let mut edges = std::mem::take(&mut self.edges);
        edges.retain(|e| {
            e.pages >= filter.min_pages && !(filter.hide_refused && refused.contains(e.to.as_str()))
        });
        if edges.len() > MAX_EDGES {
            edges.truncate(MAX_EDGES);
            self.truncated = true;
        }
        let reached: BTreeSet<&str> = edges.iter().map(|e| e.to.as_str()).collect();
        let nodes = std::mem::take(&mut self.nodes);
        self.nodes = nodes
            .into_iter()
            .filter(|n| matches!(n, Node::Source { .. }) || reached.contains(n.id()))
            .collect();
        self.edges = edges;
        self
    }
}

// ── one host, in detail ───────────────────────────────────────────────────────

/// One host as its card shows it: who links there, where on the host, and from which pages.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostDetail {
    pub host: String,
    pub standing: Standing,
    /// Why the operator refused it, when they said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub pages: usize,
    pub links: usize,
    /// The smallest address seen on the host. Absent when nothing links there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub example: Option<String>,
    /// Each source that links there, most pages first.
    pub sources: Vec<LinkedFrom>,
    /// Where on the host the links point, by the path of each page's example address,
    /// most pages first and cut at [`MAX_PATHS`].
    pub paths: Vec<PathCount>,
    pub paths_more: usize,
    /// Pages that carried the link, cut at [`MAX_REFERRERS`].
    pub referrers: Vec<Referrer>,
    pub referrers_more: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkedFrom {
    pub source: String,
    pub pages: usize,
    pub links: usize,
    pub example: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathCount {
    pub path: String,
    pub pages: usize,
}

/// A page that linked to the host, and the blob it was read out of — the handle
/// `centinel open` takes back.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Referrer {
    pub source: String,
    pub address: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blob: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
}

/// The path of an address on the host, which is where on the host a page pointed.
fn path_of(address: &str) -> String {
    url::Url::parse(address)
        .map(|u| u.path().to_string())
        .unwrap_or_else(|_| address.to_string())
}

impl HostDetail {
    /// Reads every source's ledger for the rows that name this host. The one read in
    /// this module that is not cached: it is one host, asked for by a click, and the
    /// paths and referrers it wants are the rows themselves rather than a count of them.
    pub async fn read(store: &Store, host: &str) -> anyhow::Result<Self> {
        let host = crumbs::as_host(host)?;
        let defined = crate::catalog::Sources::new(store).list().await?;
        let claimed = crumbs::claimants(store).await?.contains_key(&host);
        let decision = Decisions::new(store).current().await?.remove(&host);

        let mut sources = Vec::new();
        let mut paths: BTreeMap<String, usize> = BTreeMap::new();
        let mut carriers_by_source: BTreeMap<String, (Vec<crumbs::Carrier>, usize)> = BTreeMap::new();
        let mut example: Option<String> = None;
        for source in &defined {
            let id = SourceId::new(source.id.clone())?;
            let rows = Ledger::new(store, id).current().await?;
            let mut trail = Trail::default();
            let mut links = 0;
            for row in rows.values() {
                let Some(linked) = row.links.iter().find(|l| l.host == host) else {
                    continue;
                };
                links += linked.links;
                *paths.entry(path_of(&linked.example)).or_default() += 1;
                trail.absorb(row);
            }
            let (carriers, dropped) = trail.carriers_of(&host);
            if carriers.is_empty() {
                continue;
            }
            let own = trail
                .crumbs()
                .into_iter()
                .find(|c| c.host == host)
                .map(|c| c.example)
                .unwrap_or_default();
            if example.as_ref().is_none_or(|e| own < *e) {
                example = Some(own.clone());
            }
            sources.push(LinkedFrom {
                source: source.id.clone(),
                pages: trail.documents_read(),
                links,
                example: own,
            });
            carriers_by_source.insert(source.id.clone(), (carriers, dropped));
        }
        sources.sort_by(|a, b| b.pages.cmp(&a.pages).then(a.source.cmp(&b.source)));

        // In the order the sources are listed, so the busiest source's pages come first.
        let mut referrers = Vec::new();
        let mut referrers_more = 0;
        for linked in &sources {
            let Some((carriers, dropped)) = carriers_by_source.remove(&linked.source) else {
                continue;
            };
            referrers_more += dropped;
            referrers.extend(carriers.into_iter().map(|c| Referrer {
                source: linked.source.clone(),
                address: c.address,
                blob: c.blob,
                at: c.at,
            }));
        }
        if referrers.len() > MAX_REFERRERS {
            referrers_more += referrers.len() - MAX_REFERRERS;
            referrers.truncate(MAX_REFERRERS);
        }

        let mut paths: Vec<PathCount> = paths
            .into_iter()
            .map(|(path, pages)| PathCount { path, pages })
            .collect();
        paths.sort_by(|a, b| b.pages.cmp(&a.pages).then(a.path.cmp(&b.path)));
        let paths_more = paths.len().saturating_sub(MAX_PATHS);
        paths.truncate(MAX_PATHS);

        Ok(Self {
            // Already a source beats already refused, as `crumbs` reads it: the promotion
            // happened, and an old ruling on the host is stale rather than binding.
            standing: match (claimed, decision.as_ref().map(|d| d.ruling)) {
                (true, _) => Standing::Promoted,
                (false, Some(Ruling::Ignore)) => Standing::Ignored,
                _ => Standing::Open,
            },
            note: decision.and_then(|d| d.note),
            pages: sources.iter().map(|s| s.pages).sum(),
            links: sources.iter().map(|s| s.links).sum(),
            example,
            host,
            sources,
            paths,
            paths_more,
            referrers,
            referrers_more,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SourceConfig;
    use crate::crumbs::{Carrier, Decision, scan_page};

    async fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        (dir, store)
    }

    async fn define(store: &Store, source: SourceConfig) {
        crate::catalog::Sources::new(store).add(&source).await.unwrap();
    }

    /// One page's row, as `collect` writes it down.
    async fn page(store: &Store, source: &str, address: &str, html: &str) {
        let row = scan_page(
            html,
            Carrier {
                address: address.into(),
                at: Some("2026-08-01T00:00:00Z".into()),
                blob: Some(format!("{:0>64}", address.len())),
            },
        );
        Ledger::new(store, SourceId::new(source).unwrap())
            .append(&row)
            .await
            .unwrap();
    }

    fn edge<'a>(graph: &'a Graph, from: &str, to: &str) -> Option<&'a Edge> {
        graph.edges.iter().find(|e| e.from == from && e.to == to)
    }

    fn host_ids(graph: &Graph) -> Vec<&str> {
        graph
            .nodes
            .iter()
            .filter(|n| matches!(n, Node::Host { .. }))
            .map(Node::id)
            .collect()
    }

    /// The weight is pages, not links: a template linking a host four times from one
    /// page is one page. Two sources that link the same host both get an edge to one node.
    #[tokio::test]
    async fn edges_count_pages_and_two_sources_share_one_host_node() {
        let (_d, store) = store().await;
        define(&store, SourceConfig::site("tampa", "https://www.tampa.gov")).await;
        define(&store, SourceConfig::site("hills", "https://www.hcfl.gov")).await;
        let widget = r#"<a href="https://hover.hillsclerk.com/a">a</a>
                        <a href="https://hover.hillsclerk.com/b">b</a>"#;
        page(&store, "tampa", "https://www.tampa.gov/one", widget).await;
        page(&store, "tampa", "https://www.tampa.gov/two", widget).await;
        page(&store, "hills", "https://www.hcfl.gov/x", widget).await;

        let graph = Graph::read(&store).await.unwrap();
        let from_tampa = edge(&graph, "source:tampa", "host:hover.hillsclerk.com").unwrap();
        assert_eq!((from_tampa.pages, from_tampa.links), (2, 4));
        let from_hills = edge(&graph, "source:hills", "host:hover.hillsclerk.com").unwrap();
        assert_eq!((from_hills.pages, from_hills.links), (1, 2));

        assert_eq!(host_ids(&graph), ["host:hover.hillsclerk.com"]);
        let Node::Host { pages, links, sources, standing, .. } = &graph.nodes[2] else {
            panic!("{:?}", graph.nodes);
        };
        assert_eq!((*pages, *links, *sources, *standing), (3, 6, 2, Standing::Open));
        assert_eq!(
            graph.totals,
            Totals { sources: 2, hosts: 1, edges: 2, links: 6, open: 1, refused: 0 }
        );
    }

    /// A host a source claims is drawn as that source: the edge connects the pair, and
    /// no host node carries a copy of the source's name. A link to its own host is not an
    /// edge at all.
    #[tokio::test]
    async fn a_host_that_is_a_source_connects_the_two_sources() {
        let (_d, store) = store().await;
        define(&store, SourceConfig::site("tampa", "https://www.tampa.gov")).await;
        define(&store, SourceConfig::site("hillsclerk", "https://publicrec.hillsclerk.com/Civil/")).await;
        page(
            &store,
            "tampa",
            "https://www.tampa.gov/clerk",
            r#"<a href="https://publicrec.hillsclerk.com/Civil/">records</a>
               <a href="https://tampa.gov/bare">ours, spelled bare</a>"#,
        )
        .await;
        page(
            &store,
            "hillsclerk",
            "https://publicrec.hillsclerk.com/Civil/",
            r#"<a href="https://www.tampa.gov/">city</a>"#,
        )
        .await;

        let graph = Graph::read(&store).await.unwrap();
        assert!(edge(&graph, "source:tampa", "source:hillsclerk").is_some());
        assert!(edge(&graph, "source:hillsclerk", "source:tampa").is_some());
        // `tampa.gov` is not `www.tampa.gov`: nobody claims it, so it is a host.
        assert_eq!(host_ids(&graph), ["host:tampa.gov"]);
        assert_eq!(graph.totals.hosts, 1);
    }

    #[tokio::test]
    async fn min_pages_drops_thin_edges_and_the_hosts_they_reached() {
        let (_d, store) = store().await;
        define(&store, SourceConfig::site("tampa", "https://www.tampa.gov")).await;
        for i in 0..3 {
            page(
                &store,
                "tampa",
                &format!("https://www.tampa.gov/{i}"),
                r#"<a href="https://busy.example.com/">busy</a>"#,
            )
            .await;
        }
        page(&store, "tampa", "https://www.tampa.gov/once", r#"<a href="https://once.example.com/">once</a>"#).await;

        let whole = Graph::read(&store).await.unwrap();
        assert_eq!(host_ids(&whole), ["host:busy.example.com", "host:once.example.com"]);

        let view = whole.clone().filtered(&Filter { min_pages: 2, hide_refused: false });
        assert_eq!(host_ids(&view), ["host:busy.example.com"]);
        assert_eq!(view.edges.len(), 1);
        assert!(!view.truncated);
        assert_eq!(view.totals, whole.totals, "the totals are the whole graph's");
        // The source stays whatever the filter leaves it pointing at.
        let none = whole.filtered(&Filter { min_pages: 99, hide_refused: false });
        assert_eq!(none.nodes.len(), 1);
        assert!(none.edges.is_empty());
    }

    /// A refused host is drawn greyed by default and hidden on request; an allow brings
    /// it back. The ruling is the one `crumbs ignore` writes, so the two agree.
    #[tokio::test]
    async fn a_refused_host_is_marked_and_can_be_hidden() {
        let (_d, store) = store().await;
        define(&store, SourceConfig::site("tampa", "https://www.tampa.gov")).await;
        page(
            &store,
            "tampa",
            "https://www.tampa.gov/x",
            r#"<a href="https://www.facebook.com/CityofTampa">social</a>
               <a href="https://publicrec.hillsclerk.com/">records</a>"#,
        )
        .await;
        let decisions = Decisions::new(&store);
        decisions
            .append(&Decision {
                host: "www.facebook.com".into(),
                ruling: Ruling::Ignore,
                at: "2026-08-01T00:00:00Z".parse().unwrap(),
                note: None,
            })
            .await
            .unwrap();

        let graph = Graph::read(&store).await.unwrap();
        assert_eq!((graph.totals.open, graph.totals.refused), (1, 1));
        assert!(graph.nodes.iter().any(|n| matches!(n, Node::Host { host, standing: Standing::Ignored, .. } if host == "www.facebook.com")));
        let hidden = graph.filtered(&Filter { min_pages: 1, hide_refused: true });
        assert_eq!(host_ids(&hidden), ["host:publicrec.hillsclerk.com"]);
        assert_eq!(hidden.edges.len(), 1);

        decisions
            .append(&Decision {
                host: "www.facebook.com".into(),
                ruling: Ruling::Allow,
                at: "2026-08-02T00:00:00Z".parse().unwrap(),
                note: None,
            })
            .await
            .unwrap();
        let again = Graph::read(&store).await.unwrap().filtered(&Filter { min_pages: 1, hide_refused: true });
        assert_eq!(again.totals.refused, 0);
        assert_eq!(host_ids(&again).len(), 2);
    }

    /// The card: who links there, where on the host, and from which pages.
    #[tokio::test]
    async fn a_host_detail_groups_paths_and_names_referring_pages() {
        let (_d, store) = store().await;
        define(&store, SourceConfig::site("tampa", "https://www.tampa.gov")).await;
        define(&store, SourceConfig::site("hills", "https://www.hcfl.gov")).await;
        page(&store, "tampa", "https://www.tampa.gov/a", r#"<a href="https://vendor.example.com/login">login</a>"#).await;
        page(&store, "tampa", "https://www.tampa.gov/b", r#"<a href="https://vendor.example.com/login?x=1">login</a>"#).await;
        page(&store, "tampa", "https://www.tampa.gov/c", r#"<a href="https://vendor.example.com/docs/1">doc</a><a href="https://vendor.example.com/docs/2">doc</a>"#).await;
        page(&store, "hills", "https://www.hcfl.gov/x", r#"<a href="https://vendor.example.com/docs/1">doc</a>"#).await;
        page(&store, "hills", "https://www.hcfl.gov/y", "<p>nothing</p>").await;

        let detail = HostDetail::read(&store, "Vendor.Example.com").await.unwrap();
        assert_eq!(detail.host, "vendor.example.com");
        let from_url = HostDetail::read(&store, "https://vendor.example.com/docs/1").await.unwrap();
        assert_eq!(from_url, detail, "a URL on the host is the host, as `crumbs ignore` reads it");
        assert!(HostDetail::read(&store, "not a host").await.is_err());
        assert_eq!((detail.pages, detail.links), (4, 5));
        assert_eq!(detail.standing, Standing::Open);
        assert_eq!(detail.example.as_deref(), Some("https://vendor.example.com/docs/1"));

        let sources: Vec<(&str, usize, usize)> = detail.sources.iter().map(|s| (s.source.as_str(), s.pages, s.links)).collect();
        assert_eq!(sources, [("tampa", 3, 4), ("hills", 1, 1)]);

        let paths: Vec<(&str, usize)> = detail.paths.iter().map(|p| (p.path.as_str(), p.pages)).collect();
        assert_eq!(paths, [("/docs/1", 2), ("/login", 2)], "the query string is not a path");
        assert_eq!(detail.paths_more, 0);

        let referrers: Vec<&str> = detail.referrers.iter().map(|r| r.address.as_str()).collect();
        assert_eq!(referrers, ["https://www.tampa.gov/a", "https://www.tampa.gov/b", "https://www.tampa.gov/c", "https://www.hcfl.gov/x"]);
        assert!(detail.referrers.iter().all(|r| r.blob.is_some()), "a referrer is openable");
        assert_eq!(detail.referrers_more, 0);
    }

    /// A host nothing links to is an empty answer, and a host a source claims reads as
    /// promoted whatever an old ruling says.
    #[tokio::test]
    async fn a_host_detail_reports_standing() {
        let (_d, store) = store().await;
        define(&store, SourceConfig::site("tampa", "https://www.tampa.gov")).await;
        define(&store, SourceConfig::site("hillsclerk", "https://publicrec.hillsclerk.com/")).await;
        page(&store, "tampa", "https://www.tampa.gov/x", r#"<a href="https://publicrec.hillsclerk.com/Civil/">records</a>"#).await;
        Decisions::new(&store)
            .append(&Decision {
                host: "publicrec.hillsclerk.com".into(),
                ruling: Ruling::Ignore,
                at: "2026-08-01T00:00:00Z".parse().unwrap(),
                note: Some("refused before it was added".into()),
            })
            .await
            .unwrap();

        let promoted = HostDetail::read(&store, "publicrec.hillsclerk.com").await.unwrap();
        assert_eq!(promoted.standing, Standing::Promoted);
        assert_eq!(promoted.note.as_deref(), Some("refused before it was added"));
        assert_eq!(promoted.pages, 1);

        let nobody = HostDetail::read(&store, "nobody.example.com").await.unwrap();
        assert_eq!(nobody.pages, 0);
        assert!(nobody.sources.is_empty() && nobody.example.is_none());
    }

    /// The linkage follows the ledger: it is written on first read, answers from disk
    /// while the ledger stands, and is folded again once a page is appended.
    #[tokio::test]
    async fn a_linkage_is_cached_and_follows_the_ledger() {
        let (_d, store) = store().await;
        let id = SourceId::new("tampa").unwrap();
        page(&store, "tampa", "https://www.tampa.gov/a", r#"<a href="https://x.example.com/">x</a>"#).await;

        let linkages = Linkages::new(&store);
        let first = linkages.get(&id).await.unwrap();
        assert_eq!(first.hosts["x.example.com"].pages, 1);
        assert!(store.linkage_path(&id).exists(), "written down for the next reader");

        // Forged on disk and believed while the stamp matches: the cache is trusted,
        // which is what makes it a cache.
        let mut forged = first.clone();
        forged.hosts.get_mut("x.example.com").unwrap().pages = 99;
        std::fs::write(store.linkage_path(&id), serde_json::to_vec(&forged).unwrap()).unwrap();
        assert_eq!(linkages.get(&id).await.unwrap().hosts["x.example.com"].pages, 99);

        page(&store, "tampa", "https://www.tampa.gov/b", r#"<a href="https://x.example.com/">x</a>"#).await;
        let second = linkages.get(&id).await.unwrap();
        assert_eq!(second.hosts["x.example.com"].pages, 2, "the ledger grew, so it was folded again");
        assert_ne!(second.ledger, first.ledger);

        // No ledger at all is an empty linkage, not an error.
        let none = linkages.get(&SourceId::new("nobody").unwrap()).await.unwrap();
        assert_eq!(none, Linkage::default());
    }

    /// Past the cap the strongest edges stay, and the answer says it was cut.
    #[tokio::test]
    async fn edges_past_the_cap_are_cut_weakest_first_and_reported() {
        let (_d, store) = store().await;
        define(&store, SourceConfig::site("tampa", "https://www.tampa.gov")).await;
        let mut graph = Graph::read(&store).await.unwrap();
        graph.edges = (0..MAX_EDGES + 2)
            .map(|i| Edge { from: "source:tampa".into(), to: format!("host:h{i}"), pages: MAX_EDGES + 2 - i, links: 1 })
            .collect();
        graph.nodes.extend((0..MAX_EDGES + 2).map(|i| Node::Host {
            id: format!("host:h{i}"), host: format!("h{i}"), pages: 1, links: 1, sources: 1, standing: Standing::Open,
        }));
        let view = graph.filtered(&Filter::default());
        assert!(view.truncated);
        assert_eq!(view.edges.len(), MAX_EDGES);
        assert_eq!(view.edges.last().unwrap().pages, 3, "the weakest two were cut");
        assert_eq!(view.nodes.len(), MAX_EDGES + 1);
    }
}
