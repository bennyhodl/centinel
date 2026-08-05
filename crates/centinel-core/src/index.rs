//! The search index: SQLite with FTS5.
//!
//! **Derived and disposable.** Delete `centinel.db` and it rebuilds from the blob pool
//! and the log in minutes (SPEC §5). Nothing here is truth.
//!
//! SQLite because FTS5 gives real BM25 — Postgres `ts_rank` does not — and because it is
//! a file rather than a service, which keeps the local-only constraint intact.
//!
//! ## Chunks and placements are separate tables
//!
//! A chunk is identified by the hash of its text, so the same passage appearing on fifty
//! pages is **one** row in `chunk` and fifty rows in `placement`. That is what stops a
//! site's boilerplate from crowding the index, and it means the eventual embedding step
//! embeds each distinct passage once rather than once per page.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::chunk::Chunk;

/// Where a chunk was found, and everything needed to cite it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placement {
    pub source: String,
    /// The address — a URL, a vendor id.
    pub resource: String,
    /// The original bytes as served. Evidentiary identity.
    pub blob_sha: String,
    /// The derived text blob this chunk came from.
    pub derived_sha: String,
    pub ordinal: usize,
    pub heading: String,
    pub char_start: usize,
    pub char_end: usize,
    pub observed_at: String,
    /// Which extraction pipeline produced the text.
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// A search result: a passage plus where it came from.
///
/// SPEC §6 requires results be ranked passages with full provenance, not document ids.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hit {
    pub chunk_hash: String,
    pub text: String,
    /// Higher is better. FTS5's `bm25()` is negated so callers never have to remember
    /// that SQLite ranks ascending.
    pub score: f64,
    /// Every place this passage appears. Usually one; more when it is shared text.
    pub placements: Vec<Placement>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct IndexStats {
    pub chunks: usize,
    pub placements: usize,
    pub sources: usize,
    pub chars: usize,
}

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;

CREATE TABLE IF NOT EXISTS meta (
    key         TEXT PRIMARY KEY,
    value       TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS chunk (
    id          INTEGER PRIMARY KEY,
    chunk_hash  TEXT NOT NULL UNIQUE,
    text        TEXT NOT NULL,
    chars       INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS placement (
    chunk_hash  TEXT NOT NULL,
    source      TEXT NOT NULL,
    resource    TEXT NOT NULL,
    blob_sha    TEXT NOT NULL,
    derived_sha TEXT NOT NULL,
    ordinal     INTEGER NOT NULL,
    heading     TEXT NOT NULL,
    char_start  INTEGER NOT NULL,
    char_end    INTEGER NOT NULL,
    observed_at TEXT NOT NULL,
    tool        TEXT NOT NULL,
    title       TEXT,
    PRIMARY KEY (chunk_hash, derived_sha, ordinal)
);

CREATE INDEX IF NOT EXISTS placement_by_chunk  ON placement(chunk_hash);
CREATE INDEX IF NOT EXISTS placement_by_source ON placement(source);
CREATE INDEX IF NOT EXISTS placement_by_derived ON placement(derived_sha);

-- External-content FTS5: the text lives once, in `chunk`.
CREATE VIRTUAL TABLE IF NOT EXISTS chunk_fts USING fts5(
    text,
    content='chunk',
    content_rowid='id',
    tokenize='porter unicode61'
);

CREATE TRIGGER IF NOT EXISTS chunk_ai AFTER INSERT ON chunk BEGIN
    INSERT INTO chunk_fts(rowid, text) VALUES (new.id, new.text);
END;

CREATE TRIGGER IF NOT EXISTS chunk_ad AFTER DELETE ON chunk BEGIN
    INSERT INTO chunk_fts(chunk_fts, rowid, text) VALUES('delete', old.id, old.text);
END;
"#;

pub struct Index {
    conn: Connection,
}

impl Index {
    /// Opens (and migrates) the index at `path`.
    pub fn open(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    /// An in-memory index, for tests.
    pub fn in_memory() -> anyhow::Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    /// Inserts a chunk and one placement.
    ///
    /// The chunk body is written once; a repeat of the same text from another page adds
    /// only a placement row.
    pub fn insert(&mut self, chunk: &Chunk, placement: &Placement) -> anyhow::Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO chunk (chunk_hash, text, chars) VALUES (?1, ?2, ?3)
             ON CONFLICT(chunk_hash) DO NOTHING",
            params![
                chunk.chunk_hash,
                chunk.text,
                chunk.text.chars().count() as i64
            ],
        )?;
        tx.execute(
            "INSERT INTO placement
               (chunk_hash, source, resource, blob_sha, derived_sha, ordinal,
                heading, char_start, char_end, observed_at, tool, title)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
             ON CONFLICT(chunk_hash, derived_sha, ordinal) DO NOTHING",
            params![
                chunk.chunk_hash,
                placement.source,
                placement.resource,
                placement.blob_sha,
                placement.derived_sha,
                chunk.ordinal as i64,
                chunk.heading,
                chunk.char_start as i64,
                chunk.char_end as i64,
                placement.observed_at,
                placement.tool,
                placement.title,
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// True when this derived blob has already been chunked into the index.
    /// The chunk geometry this index's hashes were built with, if it has been recorded.
    ///
    /// A `chunk_hash` is the hash of the chunk's *text*, and the text is decided by the
    /// geometry — so re-chunking at a different size produces a wholly different set of
    /// hashes. Nothing in the index or the vector cache can tell the two sets apart, and
    /// both are append-only, so mixing them leaves the old chunks in place and re-embeds
    /// the entire corpus. Recording the geometry is what makes that a question the
    /// caller gets asked instead of a bill they get later.
    pub fn geometry(&self) -> anyhow::Result<Option<(usize, usize)>> {
        let read = |key: &str| -> anyhow::Result<Option<usize>> {
            let v: Option<String> = self
                .conn
                .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
                .optional()?;
            Ok(v.and_then(|v| v.parse().ok()))
        };
        Ok(match (read("target_chars")?, read("overlap_chars")?) {
            (Some(t), Some(o)) => Some((t, o)),
            _ => None,
        })
    }

    pub fn set_geometry(&self, target_chars: usize, overlap_chars: usize) -> anyhow::Result<()> {
        for (key, value) in [
            ("target_chars", target_chars),
            ("overlap_chars", overlap_chars),
        ] {
            self.conn.execute(
                "INSERT INTO meta (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value.to_string()],
            )?;
        }
        Ok(())
    }

    pub fn has_derived(&self, derived_sha: &str) -> anyhow::Result<bool> {
        let n: i64 = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM placement WHERE derived_sha = ?1)",
            params![derived_sha],
            |r| r.get(0),
        )?;
        Ok(n != 0)
    }

    /// BM25 search.
    pub fn search(
        &self,
        query: &str,
        limit: usize,
        source: Option<&str>,
    ) -> anyhow::Result<Vec<Hit>> {
        let match_expr = to_fts_query(query);
        if match_expr.is_empty() {
            return Ok(Vec::new());
        }

        // `bm25()` ranks ascending (more negative is better), which is a trap for every
        // caller. Negate once, here.
        let sql = "
            SELECT c.chunk_hash, c.text, -bm25(chunk_fts) AS score
            FROM chunk_fts
            JOIN chunk c ON c.id = chunk_fts.rowid
            WHERE chunk_fts MATCH ?1
              AND (?2 IS NULL OR EXISTS(
                    SELECT 1 FROM placement p
                    WHERE p.chunk_hash = c.chunk_hash AND p.source = ?2))
            ORDER BY bm25(chunk_fts)
            LIMIT ?3";

        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(params![match_expr, source, limit as i64], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, f64>(2)?,
            ))
        })?;

        let mut hits = Vec::new();
        for row in rows {
            let (chunk_hash, text, score) = row?;
            let placements = self.placements_of(&chunk_hash)?;
            hits.push(Hit {
                chunk_hash,
                text,
                score,
                placements,
            });
        }
        Ok(hits)
    }

    /// Every chunk hash in the index.
    ///
    /// Hashes only, not text: this answers "what exists?" so that `embed` can subtract
    /// what is already cached. Pulling the text too would load hundreds of megabytes to
    /// compute a set difference.
    pub fn chunk_hashes(&self) -> anyhow::Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT chunk_hash FROM chunk ORDER BY id")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// The text of specific chunks, in the order requested.
    ///
    /// Batched deliberately — `embed` walks the corpus a batch at a time so that only a
    /// batch's worth of text is resident, however large the corpus.
    pub fn chunk_texts(&self, hashes: &[String]) -> anyhow::Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT text FROM chunk WHERE chunk_hash = ?1")?;
        hashes
            .iter()
            .map(|h| {
                stmt.query_row([h], |r| r.get::<_, String>(0))
                    .map_err(|e| anyhow::anyhow!("chunk {h} is not in the index: {e}"))
            })
            .collect()
    }

    pub fn placements_of(&self, chunk_hash: &str) -> anyhow::Result<Vec<Placement>> {
        let mut stmt = self.conn.prepare(
            "SELECT source, resource, blob_sha, derived_sha, ordinal, heading,
                    char_start, char_end, observed_at, tool, title
             FROM placement WHERE chunk_hash = ?1 ORDER BY source, resource",
        )?;
        let rows = stmt.query_map(params![chunk_hash], |r| {
            Ok(Placement {
                source: r.get(0)?,
                resource: r.get(1)?,
                blob_sha: r.get(2)?,
                derived_sha: r.get(3)?,
                ordinal: r.get::<_, i64>(4)? as usize,
                heading: r.get(5)?,
                char_start: r.get::<_, i64>(6)? as usize,
                char_end: r.get::<_, i64>(7)? as usize,
                observed_at: r.get(8)?,
                tool: r.get(9)?,
                title: r.get(10)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn stats(&self) -> anyhow::Result<IndexStats> {
        let (chunks, chars): (i64, i64) = self.conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(chars), 0) FROM chunk",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let placements: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM placement", [], |r| r.get(0))?;
        let sources: i64 =
            self.conn
                .query_row("SELECT COUNT(DISTINCT source) FROM placement", [], |r| {
                    r.get(0)
                })?;
        Ok(IndexStats {
            chunks: chunks as usize,
            placements: placements as usize,
            sources: sources as usize,
            chars: chars as usize,
        })
    }

    /// Drops everything. The index is derived, so this costs a rebuild and nothing else.
    pub fn clear(&mut self) -> anyhow::Result<()> {
        self.conn
            .execute_batch("DELETE FROM placement; DELETE FROM chunk;")?;
        Ok(())
    }

    /// Drops one Source's placements, and any chunk left with none.
    ///
    /// Exists because `--rebuild --source tampa` clearing the *whole* index is a
    /// surprise: a flag scoped by `--source` must not delete beyond it. Rebuilding the
    /// others costs only time, but silently making a caller re-index a corpus they did
    /// not name is the kind of thing that is noticed much later.
    ///
    /// Chunks are shared across placements — that is §6's boilerplate-collapsing property
    /// — so a chunk is removed only once nothing points at it any more.
    pub fn clear_source(&mut self, source: &str) -> anyhow::Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM placement WHERE source = ?1", [source])?;
        tx.execute(
            "DELETE FROM chunk WHERE chunk_hash NOT IN (SELECT chunk_hash FROM placement)",
            [],
        )?;
        tx.commit()?;
        Ok(())
    }
}

/// Turns user input into a safe FTS5 MATCH expression.
///
/// Every term is double-quoted. FTS5's query language treats `-`, `*`, `:`, `^`, `(`,
/// `)` and `NEAR` as syntax, so a user searching `budget-2026` or `AT&T` would otherwise
/// get a parse error rather than results. Quoting makes each term a literal phrase.
pub fn to_fts_query(input: &str) -> String {
    input
        .split_whitespace()
        // Quotes are the escape mechanism, so they cannot survive inside a term.
        .map(|t| t.replace('"', " "))
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{t}\""))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::{ChunkConfig, chunk_markdown};

    fn placement(source: &str, resource: &str, derived: &str) -> Placement {
        Placement {
            source: source.into(),
            resource: resource.into(),
            blob_sha: "aa".repeat(32),
            derived_sha: derived.into(),
            ordinal: 0,
            heading: String::new(),
            char_start: 0,
            char_end: 0,
            observed_at: "2026-08-03T00:00:00Z".into(),
            tool: "dom_smoothie+htmd".into(),
            title: Some("A page".into()),
        }
    }

    fn indexed(docs: &[(&str, &str)]) -> Index {
        let mut idx = Index::in_memory().unwrap();
        for (i, (url, md)) in docs.iter().enumerate() {
            let derived = format!("{:064x}", i);
            for c in chunk_markdown(md, &ChunkConfig::default()) {
                idx.insert(&c, &placement("tampa", url, &derived)).unwrap();
            }
        }
        idx
    }

    #[test]
    fn finds_a_passage_and_returns_its_provenance() {
        let idx = indexed(&[(
            "https://www.tampa.gov/lobbyist",
            "# Lobbyist Registration\n\nAll lobbyists must register with the city clerk \
             before contacting any council member about pending legislation.",
        )]);

        let hits = idx.search("lobbyist register", 10, None).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].text.contains("lobbyists must register"));

        let p = &hits[0].placements[0];
        assert_eq!(p.source, "tampa");
        assert_eq!(p.resource, "https://www.tampa.gov/lobbyist");
        assert_eq!(p.tool, "dom_smoothie+htmd");
        assert_eq!(p.title.as_deref(), Some("A page"));
    }

    #[test]
    fn stemming_matches_word_forms() {
        let idx = indexed(&[(
            "u",
            "# T\n\nThe commission is reviewing several rezoning applications this quarter.",
        )]);
        // `porter` stemming: review → reviewing, application → applications.
        assert_eq!(idx.search("review application", 10, None).unwrap().len(), 1);
    }

    #[test]
    fn ranking_puts_the_better_match_first() {
        let idx = indexed(&[
            (
                "a",
                "# A\n\nThe budget mentions stormwater once among many other topics here.",
            ),
            (
                "b",
                "# B\n\nStormwater stormwater stormwater drainage and stormwater management plans.",
            ),
        ]);
        let hits = idx.search("stormwater", 10, None).unwrap();
        assert!(hits.len() >= 2);
        assert!(
            hits[0].score >= hits[1].score,
            "scores must rank descending"
        );
        assert!(hits[0].placements[0].resource == "b");
    }

    /// The de-duplication property: shared text is one chunk with many placements.
    #[test]
    fn identical_text_across_pages_is_one_chunk_with_many_placements() {
        let boiler = "# Notice\n\nThis document is provided under the Florida public \
                      records law and may be reproduced without charge by any person.";
        let idx = indexed(&[("page-one", boiler), ("page-two", boiler)]);

        let stats = idx.stats().unwrap();
        assert_eq!(stats.chunks, 1, "boilerplate must not duplicate");
        assert_eq!(stats.placements, 2);

        let hits = idx.search("public records law", 10, None).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].placements.len(), 2, "both pages must be citable");
    }

    #[test]
    fn source_filter_scopes_results() {
        let mut idx = Index::in_memory().unwrap();
        for c in chunk_markdown(
            "# T\n\nCouncil approved the drainage project budget.",
            &ChunkConfig::default(),
        ) {
            idx.insert(&c, &placement("tampa", "t", &"1".repeat(64)))
                .unwrap();
            idx.insert(&c, &placement("hillsborough", "h", &"2".repeat(64)))
                .unwrap();
        }
        assert_eq!(idx.search("drainage", 10, Some("tampa")).unwrap().len(), 1);
        assert_eq!(
            idx.search("drainage", 10, Some("nowhere")).unwrap().len(),
            0
        );
    }

    #[test]
    fn reindexing_the_same_document_is_idempotent() {
        let doc = (
            "u",
            "# T\n\nThe council chamber will be closed for renovation work.",
        );
        let mut idx = indexed(&[doc]);
        let before = idx.stats().unwrap();

        for c in chunk_markdown(doc.1, &ChunkConfig::default()) {
            idx.insert(&c, &placement("tampa", "u", &format!("{:064x}", 0)))
                .unwrap();
        }
        assert_eq!(idx.stats().unwrap().chunks, before.chunks);
        assert_eq!(idx.stats().unwrap().placements, before.placements);
    }

    /// Punctuation in a query is FTS5 syntax. Unquoted, these are parse errors.
    #[test]
    fn punctuation_in_queries_does_not_blow_up() {
        let idx = indexed(&[(
            "u",
            "# T\n\nThe budget-2026 hearing covers AT&T franchise fees.",
        )]);
        for q in [
            "budget-2026",
            "AT&T",
            "fees:",
            "* wildcard",
            "NEAR(a b)",
            "\"unbalanced",
        ] {
            assert!(idx.search(q, 5, None).is_ok(), "query {q:?} errored");
        }
        assert_eq!(idx.search("budget-2026", 5, None).unwrap().len(), 1);
    }

    #[test]
    fn empty_query_returns_nothing_rather_than_everything() {
        let idx = indexed(&[(
            "u",
            "# T\n\nSome indexed content that is long enough to keep.",
        )]);
        assert!(idx.search("", 10, None).unwrap().is_empty());
        assert!(idx.search("   ", 10, None).unwrap().is_empty());
    }

    #[test]
    fn has_derived_tracks_what_is_already_indexed() {
        let idx = indexed(&[(
            "u",
            "# T\n\nSome indexed content that is long enough to keep.",
        )]);
        assert!(idx.has_derived(&format!("{:064x}", 0)).unwrap());
        assert!(!idx.has_derived(&"ff".repeat(32)).unwrap());
    }

    #[test]
    fn clearing_empties_the_index_and_its_fts_shadow() {
        let mut idx = indexed(&[(
            "u",
            "# T\n\nSome indexed content that is long enough to keep.",
        )]);
        idx.clear().unwrap();
        assert_eq!(idx.stats().unwrap().chunks, 0);
        assert!(idx.search("indexed", 10, None).unwrap().is_empty());
    }

    /// `--rebuild --source tampa` must not take the other sources with it. The index is
    /// derived so nothing is lost, but silently making someone re-index a corpus they
    /// did not name is discovered long after the fact.
    #[test]
    fn clearing_one_source_leaves_the_others_indexed() {
        let mut idx = Index::in_memory().unwrap();
        let doc = |heading: &str| {
            format!("# {heading}\n\nA passage about stormwater that is long enough to keep.")
        };
        for (source, url, md) in [
            ("tampa", "https://tampa.gov/a", doc("Tampa")),
            ("hillsborough", "https://hcfl.gov/b", doc("Hillsborough")),
        ] {
            for c in chunk_markdown(&md, &ChunkConfig::default()) {
                idx.insert(&c, &placement(source, url, &"cc".repeat(32)))
                    .unwrap();
            }
        }
        assert_eq!(idx.search("stormwater", 10, None).unwrap().len(), 2);

        idx.clear_source("tampa").unwrap();

        let hits = idx.search("stormwater", 10, None).unwrap();
        assert_eq!(hits.len(), 1, "only tampa should have gone");
        assert!(
            hits[0]
                .placements
                .iter()
                .all(|p| p.source == "hillsborough")
        );
    }

    /// Chunks are shared across placements — §6's boilerplate-collapsing property — so
    /// clearing one source must not delete text another source still points at.
    #[test]
    fn a_shared_chunk_survives_clearing_one_of_its_sources() {
        let mut idx = Index::in_memory().unwrap();
        let shared = "# Notice\n\nThis identical footer appears on every page of both sites.";
        // Distinct `derived_sha` per source, because that is what sharing looks like:
        // one chunk of text reached from two different documents. Reusing one would
        // collide on the placement primary key and store a single row.
        for (source, derived) in [("tampa", "dd"), ("hillsborough", "ee")] {
            for c in chunk_markdown(shared, &ChunkConfig::default()) {
                idx.insert(&c, &placement(source, "https://x/1", &derived.repeat(32)))
                    .unwrap();
            }
        }
        assert_eq!(idx.stats().unwrap().placements, 2);

        idx.clear_source("tampa").unwrap();

        let hits = idx.search("identical footer", 10, None).unwrap();
        assert_eq!(hits.len(), 1, "the shared text is still hillsborough's");
        assert!(idx.stats().unwrap().chunks > 0, "the chunk row must remain");
    }
}
