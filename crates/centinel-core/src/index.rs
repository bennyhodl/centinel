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

use std::collections::{HashMap, HashSet};
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

-- Covers `chunk_hashes_by_length`'s `ORDER BY chars, id` — chunk_hash trails so the
-- scan never touches the table's `text` column. Without it that query is a full
-- table scan into a temp b-tree sort, and `embed` pays that on every run, not just
-- the first.
CREATE INDEX IF NOT EXISTS chunk_by_chars ON chunk(chars, id, chunk_hash);

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
    PRIMARY KEY (chunk_hash, source, resource, derived_sha, ordinal)
);

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

/// Kept apart from [`SCHEMA`] because a migration that replaces `placement` drops these
/// with it, and the one copy is what stops the rebuilt table from quietly losing an index.
const PLACEMENT_INDEXES: &str = r#"
CREATE INDEX IF NOT EXISTS placement_by_chunk   ON placement(chunk_hash);
CREATE INDEX IF NOT EXISTS placement_by_source  ON placement(source);
CREATE INDEX IF NOT EXISTS placement_by_derived ON placement(derived_sha);

-- Serves the resume predicate in `ops::build_index`. The primary key leads with
-- `chunk_hash`, so it cannot answer "is this derived text already placed at this address".
CREATE INDEX IF NOT EXISTS placement_by_address ON placement(source, resource, derived_sha);
"#;

/// The shape of `placement`'s primary key, and the only thing versioned here.
///
/// `2` — the address joined the key. At `1` it was `(chunk_hash, derived_sha, ordinal)`,
/// which silently discarded every address after the first whenever two of them shared one
/// derived blob.
const SCHEMA_VERSION: i64 = 2;

pub struct Index {
    conn: Connection,
}

impl Index {
    /// Opens (and migrates) the index at `path`.
    pub fn open(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        let index = Self { conn };
        index.migrate()?;
        Ok(index)
    }

    /// An in-memory index, for tests.
    pub fn in_memory() -> anyhow::Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        let index = Self { conn };
        index.migrate()?;
        Ok(index)
    }

    /// Brings an older `placement` table up to [`SCHEMA_VERSION`], keeping its rows.
    ///
    /// `CREATE TABLE IF NOT EXISTS` cannot change a primary key, so a table written at
    /// version 1 keeps the narrow key until something replaces it. This does, by copy —
    /// **not** by dropping and re-deriving. The rows are still correct as far as they go;
    /// what version 1 got wrong is the addresses it never wrote, and those come back on the
    /// next `centinel index` now that the resume predicate asks about addresses. Re-deriving
    /// instead would leave every read path answering from an empty index until that run
    /// finished, which is a worse failure than the one being fixed.
    ///
    /// Chunk text is untouched, so no `chunk_hash` moves and no cached vector is orphaned.
    fn migrate(&self) -> anyhow::Result<()> {
        let recorded: Option<i64> = self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'schema_version'",
                [],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .and_then(|v| v.parse().ok());

        if recorded == Some(SCHEMA_VERSION) {
            self.conn.execute_batch(PLACEMENT_INDEXES)?;
            return Ok(());
        }

        // An unversioned but *empty* table is a fresh one this build just created at the
        // current shape. Only a populated one was written by an older build.
        let populated: i64 =
            self.conn
                .query_row("SELECT EXISTS(SELECT 1 FROM placement)", [], |r| r.get(0))?;

        if recorded.is_none() && populated != 0 {
            self.conn.execute_batch(
                r#"
                CREATE TABLE placement_migrating (
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
                    PRIMARY KEY (chunk_hash, source, resource, derived_sha, ordinal)
                );

                INSERT INTO placement_migrating
                SELECT chunk_hash, source, resource, blob_sha, derived_sha, ordinal,
                       heading, char_start, char_end, observed_at, tool, title
                FROM placement;

                DROP TABLE placement;
                ALTER TABLE placement_migrating RENAME TO placement;
                "#,
            )?;
        }

        self.conn.execute_batch(PLACEMENT_INDEXES)?;
        self.conn.execute(
            "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![SCHEMA_VERSION.to_string()],
        )?;
        Ok(())
    }

    /// Opens a write batch: one transaction covering however many rows the caller adds.
    ///
    /// **The unit of batching is the caller's unit of resumption.** A commit here is a WAL
    /// checkpoint and an FTS5 index flush, and FTS5 is built to accumulate rows and flush
    /// in bulk — so committing once per row does not merely pay the commit cost 450,000
    /// times, it defeats the design of the thing being written to. A document's worth of
    /// rows in one transaction is safe because [`Self::has_placement`] subtracts placements
    /// *per address*: a batch lost to a crash is a document the next run simply redoes.
    pub fn batch(&mut self) -> anyhow::Result<Batch<'_>> {
        Ok(Batch {
            tx: self.conn.transaction()?,
        })
    }

    /// Inserts a chunk and one placement, in a transaction of its own.
    ///
    /// Convenience over [`Self::batch`] for a caller with one row to write. Anything
    /// writing a whole document should open a batch instead — see its note.
    pub fn insert(&mut self, chunk: &Chunk, placement: &Placement) -> anyhow::Result<bool> {
        let mut batch = self.batch()?;
        let body_is_new = batch.insert(chunk, placement)?;
        batch.commit()?;
        Ok(body_is_new)
    }

    /// The chunk geometry this index's hashes were built with, if it has been recorded.
    ///
    /// A `chunk_hash` is the hash of the chunk's *text*, and the text is decided by the
    /// geometry — so re-chunking at a different size produces a wholly different set of
    /// hashes. Nothing in the index or the vector table can tell the two sets apart, and
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

    /// True when this derived text is already placed at this address.
    ///
    /// Keyed on the **address**, not on the derived blob alone. One derived blob can be
    /// the extracted text of many addresses — two proclamations issued on the same day
    /// reduce to the same bytes, a PDF is linked from two pages — and `WHERE derived_sha =
    /// ?` calls the second address done the moment the first is written. On the corpus that
    /// found this, that silently dropped 285 of 1005 pages: collected, extracted, and
    /// absent from every search, with another page's URL cited in place of each.
    pub fn has_placement(
        &self,
        source: &str,
        resource: &str,
        derived_sha: &str,
    ) -> anyhow::Result<bool> {
        // `prepare_cached`, because this runs once per (derivation × address) on the
        // indexing path — the same reason `Batch::insert` is cached. `query_row` compiles
        // its SQL afresh every call, and this is the hottest statement `index` issues.
        let mut stmt = self.conn.prepare_cached(
            "SELECT EXISTS(SELECT 1 FROM placement
                           WHERE source = ?1 AND resource = ?2 AND derived_sha = ?3)",
        )?;
        let n: i64 = stmt.query_row(params![source, resource, derived_sha], |r| r.get(0))?;
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

        let ranked = rows.collect::<Result<Vec<_>, _>>()?;

        // Hydrated in one query rather than one per row. This loop used to call
        // `placements_of` per hit, so a search at `ARM_DEPTH` fired a hundred round trips
        // where the placements of all hundred chunks are a single `IN (…)` — the shape
        // `in_source` next door has always used on the same table.
        let hashes: Vec<String> = ranked.iter().map(|(h, _, _)| h.clone()).collect();
        let mut placements = self.placements_ofs(&hashes)?;

        Ok(ranked
            .into_iter()
            .map(|(chunk_hash, text, score)| Hit {
                placements: placements.remove(&chunk_hash).unwrap_or_default(),
                chunk_hash,
                text,
                score,
            })
            .collect())
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

    /// Every chunk hash, shortest text first.
    ///
    /// The order `embed` wants its work list in. A batch decodes as physical passes of
    /// whole sequences, so texts of one length pack them evenly — and the oversized
    /// tail, each of which forces its whole group through a bespoke context, arrives
    /// together at the end instead of taxing every group it would otherwise visit.
    /// The order is free to vary: resumability is a set difference against the vector
    /// table, not a cursor into this list.
    pub fn chunk_hashes_by_length(&self) -> anyhow::Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT chunk_hash FROM chunk ORDER BY chars, id")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// The text of specific chunks, in the order requested.
    ///
    /// Batched deliberately — `embed` walks the corpus a batch at a time so that only a
    /// batch's worth of text is resident, however large the corpus.
    pub fn chunk_texts(&self, hashes: &[String]) -> anyhow::Result<Vec<String>> {
        if hashes.is_empty() {
            return Ok(Vec::new());
        }
        // One `IN (…)`, then reordered here — a statement per hash was the shape this
        // batched signature existed to avoid, and `fuse` calls it with a single hash per
        // vector-only survivor, which made it a round trip each.
        let holes = std::iter::repeat_n("?", hashes.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!("SELECT chunk_hash, text FROM chunk WHERE chunk_hash IN ({holes})");

        let mut stmt = self.conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::ToSql> =
            hashes.iter().map(|h| h as &dyn rusqlite::ToSql).collect();
        let rows = stmt.query_map(params.as_slice(), |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;

        let mut found: HashMap<String, String> = HashMap::new();
        for row in rows {
            let (hash, text) = row?;
            found.insert(hash, text);
        }
        // In the order requested, and still an error rather than a gap: `embed` pairs
        // these with the hashes it asked for, so a silent hole would attach a vector to
        // the wrong chunk.
        hashes
            .iter()
            .map(|h| {
                found
                    .remove(h)
                    .ok_or_else(|| anyhow::anyhow!("chunk {h} is not in the index"))
            })
            .collect()
    }

    /// Which of `hashes` have a placement in `source`.
    ///
    /// The vector arm's `--source` filter. Lance carries no source column — a chunk has
    /// many placements, across sources — so the filter is applied after retrieval, and
    /// applying it one `placements_of` call per candidate would be a query per hit.
    pub fn in_source(&self, hashes: &[String], source: &str) -> anyhow::Result<HashSet<String>> {
        if hashes.is_empty() {
            return Ok(HashSet::new());
        }
        let holes = std::iter::repeat_n("?", hashes.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT DISTINCT chunk_hash FROM placement
             WHERE source = ?1 AND chunk_hash IN ({holes})"
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(hashes.len() + 1);
        params.push(&source);
        for h in hashes {
            params.push(h);
        }
        let rows = stmt.query_map(params.as_slice(), |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<Result<HashSet<_>, _>>()?)
    }

    /// Where a chunk sits, everywhere it sits.
    ///
    /// `prepare_cached` because the hybrid arms reach 100 deep each, so this runs a
    /// hundred times per query rather than the ten it did when `search` was the only
    /// caller and took `limit` directly. Compiling the same statement each time measured
    /// 1.11 ms against 0.61 ms per query on a 460,000-placement corpus — small beside a
    /// reranker pass, and free to not pay.
    pub fn placements_of(&self, chunk_hash: &str) -> anyhow::Result<Vec<Placement>> {
        let mut stmt = self.conn.prepare_cached(
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

    /// Where each of several chunks sits — the batched form of [`Self::placements_of`].
    ///
    /// One `IN (…)` instead of one round trip per chunk. Both retrieval arms reach
    /// `ARM_DEPTH` deep, so the caller-per-hit version ran a hundred statements per query
    /// on the way to a result that keeps forty of them. A chunk with no placements is
    /// simply absent from the map, which is the same answer an empty `Vec` gave.
    pub fn placements_ofs(
        &self,
        hashes: &[String],
    ) -> anyhow::Result<HashMap<String, Vec<Placement>>> {
        if hashes.is_empty() {
            return Ok(HashMap::new());
        }
        let holes = std::iter::repeat_n("?", hashes.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT chunk_hash, source, resource, blob_sha, derived_sha, ordinal, heading,
                    char_start, char_end, observed_at, tool, title
             FROM placement WHERE chunk_hash IN ({holes})
             ORDER BY source, resource"
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::ToSql> =
            hashes.iter().map(|h| h as &dyn rusqlite::ToSql).collect();
        let rows = stmt.query_map(params.as_slice(), |r| {
            Ok((
                r.get::<_, String>(0)?,
                Placement {
                    source: r.get(1)?,
                    resource: r.get(2)?,
                    blob_sha: r.get(3)?,
                    derived_sha: r.get(4)?,
                    ordinal: r.get::<_, i64>(5)? as usize,
                    heading: r.get(6)?,
                    char_start: r.get::<_, i64>(7)? as usize,
                    char_end: r.get::<_, i64>(8)? as usize,
                    observed_at: r.get(9)?,
                    tool: r.get(10)?,
                    title: r.get(11)?,
                },
            ))
        })?;

        let mut out: HashMap<String, Vec<Placement>> = HashMap::new();
        for row in rows {
            let (hash, placement) = row?;
            out.entry(hash).or_default().push(placement);
        }
        Ok(out)
    }

    /// How many distinct chunks the index holds.
    ///
    /// Separate from [`Self::stats`] because that one sums `chars`, and summing a column
    /// means reading every row: on a 397,830-chunk corpus holding 330 MB of text it
    /// measured **6.0 s cold** against **0.29 s** for the count alone, which `COUNT(*)`
    /// answers from an index without touching the text.
    ///
    /// `search` needs the count on every query — it is the denominator of the vector
    /// arm's coverage — and needs none of the other three figures. Paying six seconds
    /// for a number in a report footer made the corpus size the most expensive part of
    /// asking the corpus a question.
    pub fn chunk_count(&self) -> anyhow::Result<usize> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM chunk", [], |r| r.get(0))?;
        Ok(n as usize)
    }

    /// Every figure about the index, including the expensive one.
    ///
    /// For `index`, which reports on the store it just built. A caller that wants only
    /// the chunk count wants [`Self::chunk_count`].
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

/// Rows on their way into the index, under one transaction. Dropped without
/// [`Self::commit`], it rolls back — which is `rusqlite`'s default and the one we want.
pub struct Batch<'a> {
    tx: rusqlite::Transaction<'a>,
}

impl Batch<'_> {
    /// Adds a chunk and one placement. Answers whether the chunk *body* was new.
    ///
    /// The body is written once; a repeat of the same text from another page adds only a
    /// placement row. The return value is what `ON CONFLICT DO NOTHING` already knows —
    /// one row changed means the body had not been seen before. Reporting it is not a
    /// convenience: the caller's alternative is counting the table before and after, and
    /// [`Index::stats`] is three full scans, which per chunk makes indexing quadratic in
    /// the size of the corpus.
    ///
    /// `prepare_cached` because these two statements are the hottest SQL in the codebase
    /// and `Connection::execute` re-parses its SQL on every call. The cache lives on the
    /// connection, so it outlives any one batch.
    pub fn insert(&mut self, chunk: &Chunk, placement: &Placement) -> anyhow::Result<bool> {
        let body_is_new = self
            .tx
            .prepare_cached(
                "INSERT INTO chunk (chunk_hash, text, chars) VALUES (?1, ?2, ?3)
                 ON CONFLICT(chunk_hash) DO NOTHING",
            )?
            .execute(params![
                chunk.chunk_hash,
                chunk.text,
                chunk.text.chars().count() as i64
            ])?
            == 1;

        self.tx
            .prepare_cached(
                "INSERT INTO placement
                   (chunk_hash, source, resource, blob_sha, derived_sha, ordinal,
                    heading, char_start, char_end, observed_at, tool, title)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
                 ON CONFLICT(chunk_hash, source, resource, derived_sha, ordinal) DO NOTHING",
            )?
            .execute(params![
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
            ])?;

        Ok(body_is_new)
    }

    pub fn commit(self) -> anyhow::Result<()> {
        self.tx.commit()?;
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

    /// The work-list order `embed` batches by: texts of one length ride together, and
    /// the long tail comes last.
    #[test]
    fn hashes_by_length_run_shortest_first() {
        let long = "A much longer passage. ".repeat(20);
        let idx = indexed(&[
            ("https://example.gov/long", long.as_str()),
            ("https://example.gov/short", "Tiny."),
            (
                "https://example.gov/mid",
                "A middling passage about drainage.",
            ),
        ]);

        let ordered = idx.chunk_hashes_by_length().unwrap();
        let texts = idx.chunk_texts(&ordered).unwrap();
        let lengths: Vec<usize> = texts.iter().map(String::len).collect();
        let mut sorted = lengths.clone();
        sorted.sort_unstable();
        assert_eq!(lengths, sorted, "not shortest-first: {lengths:?}");
        assert_eq!(
            ordered.len(),
            idx.chunk_hashes().unwrap().len(),
            "an order must never drop a chunk"
        );
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

    /// The vector arm's `--source` filter. Lance has no source column, so retrieval
    /// over-fetches and this narrows the candidates in one query rather than one per hit.
    #[test]
    fn in_source_keeps_only_the_chunks_placed_in_that_source() {
        let mut idx = Index::in_memory().unwrap();
        let mut hashes = Vec::new();
        for (i, (source, text)) in [
            ("tampa", "the stormwater improvement fee funds new inlets"),
            ("pinellas", "the county drainage district met on tuesday"),
        ]
        .iter()
        .enumerate()
        {
            let derived = format!("{:064x}", i);
            let chunk = Chunk::new(text.to_string(), 0, String::new(), 0);
            hashes.push(chunk.chunk_hash.clone());
            idx.insert(
                &chunk,
                &placement(source, &format!("https://x/{i}"), &derived),
            )
            .unwrap();
        }

        let kept = idx.in_source(&hashes, "tampa").unwrap();
        assert_eq!(kept, HashSet::from([hashes[0].clone()]));
        assert!(idx.in_source(&hashes, "nowhere").unwrap().is_empty());
        assert!(idx.in_source(&[], "tampa").unwrap().is_empty());
    }

    /// The same passage under two sources is one chunk with two placements, so filtering
    /// by either source must keep it.
    #[test]
    fn in_source_finds_a_shared_chunk_under_each_of_its_sources() {
        let mut idx = Index::in_memory().unwrap();
        let chunk = Chunk::new("identical boilerplate notice".into(), 0, String::new(), 0);
        for (i, source) in ["tampa", "pinellas"].iter().enumerate() {
            idx.insert(
                &chunk,
                &placement(source, &format!("https://x/{i}"), &format!("{:064x}", i)),
            )
            .unwrap();
        }

        let hashes = vec![chunk.chunk_hash.clone()];
        assert_eq!(idx.in_source(&hashes, "tampa").unwrap().len(), 1);
        assert_eq!(idx.in_source(&hashes, "pinellas").unwrap().len(), 1);
    }

    /// `chunk_count` has to agree with `stats`, because it exists only to avoid the
    /// `SUM(chars)` that makes `stats` read every row.
    #[test]
    fn chunk_count_agrees_with_stats_without_summing_text() {
        let idx = indexed(&[
            (
                "https://x/a",
                "# A\n\nThe stormwater plan for the coming year.",
            ),
            (
                "https://x/b",
                "# B\n\nA notice of public hearing on rezoning.",
            ),
        ]);
        assert_eq!(idx.chunk_count().unwrap(), idx.stats().unwrap().chunks);
        assert!(idx.chunk_count().unwrap() > 0);
        assert_eq!(Index::in_memory().unwrap().chunk_count().unwrap(), 0);
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

    /// The signal the report's `chunks written` / `chunks deduplicated` split is built on.
    ///
    /// Asserted on `insert`'s own answer rather than through the report, because the
    /// alternative way to learn this — counting the table before and after — is what made
    /// indexing quadratic, and a test that goes through `stats` would keep passing if the
    /// return value were wired up wrongly.
    #[test]
    fn insert_says_whether_the_chunk_body_was_new() {
        let mut idx = Index::in_memory().unwrap();
        let chunks = chunk_markdown(
            "# Notice\n\nReproduced without charge under the public records law.",
            &ChunkConfig::default(),
        );
        let c = &chunks[0];

        assert!(
            idx.insert(c, &placement("tampa", "page-one", &"1".repeat(64)))
                .unwrap(),
            "the first sight of a body is new"
        );
        assert!(
            !idx.insert(c, &placement("tampa", "page-two", &"2".repeat(64)))
                .unwrap(),
            "the same text under a second address adds a placement, not a body"
        );
        assert!(
            !idx.insert(c, &placement("tampa", "page-one", &"1".repeat(64)))
                .unwrap(),
            "and re-inserting the very same placement adds nothing at all"
        );

        let stats = idx.stats().unwrap();
        assert_eq!(stats.chunks, 1);
        assert_eq!(stats.placements, 2, "the repeat placement was ignored");
    }

    /// The batch boundary is only safe if an abandoned batch leaves *nothing* behind: a
    /// half-written document that the resume predicate then called done would be a page
    /// collected, extracted, and absent from every search.
    #[test]
    fn a_batch_dropped_without_committing_writes_nothing() {
        let mut idx = Index::in_memory().unwrap();
        let chunks = chunk_markdown(
            "# Minutes\n\nThe board approved the stormwater assessment.",
            &ChunkConfig::default(),
        );

        {
            let mut batch = idx.batch().unwrap();
            for c in &chunks {
                batch
                    .insert(c, &placement("tampa", "abandoned", &"1".repeat(64)))
                    .unwrap();
            }
            // No `commit`, so the guard rolls back as it goes out of scope.
        }

        let stats = idx.stats().unwrap();
        assert_eq!(stats.chunks, 0, "no body survived");
        assert_eq!(stats.placements, 0, "and no placement claims it is indexed");
        assert!(
            !idx.has_placement("tampa", "abandoned", &"1".repeat(64))
                .unwrap(),
            "so the next run still counts the document as outstanding"
        );

        // And the same batch's work, committed, does land — the rollback above is the
        // guard doing its job, not the inserts silently failing.
        let mut batch = idx.batch().unwrap();
        for c in &chunks {
            batch
                .insert(c, &placement("tampa", "kept", &"1".repeat(64)))
                .unwrap();
        }
        batch.commit().unwrap();
        assert!(
            idx.has_placement("tampa", "kept", &"1".repeat(64)).unwrap(),
            "a committed batch is durable"
        );
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
    fn has_placement_tracks_what_is_already_indexed() {
        let idx = indexed(&[(
            "u",
            "# T\n\nSome indexed content that is long enough to keep.",
        )]);
        let derived = format!("{:064x}", 0);
        assert!(idx.has_placement("tampa", "u", &derived).unwrap());
        assert!(!idx.has_placement("tampa", "u", &"ff".repeat(32)).unwrap());
    }

    /// The defect that made 285 of one corpus's 1005 pages unsearchable. Two addresses
    /// whose pages extract to identical text share a derived blob, and the resume
    /// predicate must still call the second one outstanding.
    #[test]
    fn a_second_address_sharing_a_derived_blob_is_not_already_indexed() {
        let mut idx = Index::in_memory().unwrap();
        let shared = "# Proclamation\n\nIssued on Tuesday, March 1, 2022 by the mayor.";
        let derived = "dd".repeat(32);

        for c in chunk_markdown(shared, &ChunkConfig::default()) {
            idx.insert(
                &c,
                &placement("tampa", "https://tampa.gov/red-cross", &derived),
            )
            .unwrap();
        }

        assert!(
            !idx.has_placement("tampa", "https://tampa.gov/irish-heritage", &derived)
                .unwrap(),
            "a different address has not been placed just because the text was"
        );
        assert!(
            !idx.has_placement("pinellas", "https://tampa.gov/red-cross", &derived)
                .unwrap(),
            "an address is a source and a natural key, not a natural key alone"
        );
    }

    /// Both addresses survive the insert. Under the version-1 key the second one collided
    /// with the first and was silently dropped.
    #[test]
    fn two_addresses_sharing_a_derived_blob_both_get_placements() {
        let mut idx = Index::in_memory().unwrap();
        let shared = "# Proclamation\n\nIssued on Tuesday, March 1, 2022 by the mayor.";
        let derived = "dd".repeat(32);

        for url in [
            "https://tampa.gov/red-cross",
            "https://tampa.gov/irish-heritage",
        ] {
            for c in chunk_markdown(shared, &ChunkConfig::default()) {
                idx.insert(&c, &placement("tampa", url, &derived)).unwrap();
            }
        }

        let hits = idx.search("proclamation", 10, None).unwrap();
        assert_eq!(hits.len(), 1, "one chunk, because the text is identical");
        let cited: Vec<_> = hits[0].placements.iter().map(|p| &p.resource).collect();
        assert_eq!(cited.len(), 2, "both addresses are citable: {cited:?}");
        assert_eq!(
            idx.stats().unwrap().chunks,
            1,
            "the text is still stored once"
        );
    }

    /// The version-1 table, written by a build that keyed placements on the derived blob.
    fn v1_index_at(path: &std::path::Path) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE chunk (
                id INTEGER PRIMARY KEY, chunk_hash TEXT NOT NULL UNIQUE,
                text TEXT NOT NULL, chars INTEGER NOT NULL
            );
            CREATE TABLE placement (
                chunk_hash TEXT NOT NULL, source TEXT NOT NULL, resource TEXT NOT NULL,
                blob_sha TEXT NOT NULL, derived_sha TEXT NOT NULL, ordinal INTEGER NOT NULL,
                heading TEXT NOT NULL, char_start INTEGER NOT NULL, char_end INTEGER NOT NULL,
                observed_at TEXT NOT NULL, tool TEXT NOT NULL, title TEXT,
                PRIMARY KEY (chunk_hash, derived_sha, ordinal)
            );
            INSERT INTO chunk (chunk_hash, text, chars) VALUES ('c1', 'a shared notice', 15);
            INSERT INTO placement VALUES
                ('c1','tampa','https://tampa.gov/red-cross','aa','dd',0,'',0,15,'t','htmd','T');
            "#,
        )
        .unwrap();
    }

    /// An older index keeps its rows. Re-deriving instead would leave every read path
    /// answering from an empty index until the next `centinel index` finished.
    #[test]
    fn migrating_from_version_one_keeps_the_placements_it_had() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("centinel.db");
        v1_index_at(&path);

        let idx = Index::open(&path).unwrap();
        assert_eq!(idx.stats().unwrap().placements, 1, "the row survived");
        assert!(
            idx.has_placement("tampa", "https://tampa.gov/red-cross", "dd")
                .unwrap()
        );
    }

    /// And the address it never recorded is reported as outstanding, so the next index
    /// run writes it. Under version 1 this answered `true` and the page stayed invisible.
    #[test]
    fn migrating_from_version_one_reopens_the_addresses_it_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("centinel.db");
        v1_index_at(&path);

        let idx = Index::open(&path).unwrap();
        assert!(
            !idx.has_placement("tampa", "https://tampa.gov/irish-heritage", "dd")
                .unwrap(),
            "the second address is outstanding, not done"
        );
    }

    /// Migrating twice is not a second migration.
    #[test]
    fn reopening_a_migrated_index_leaves_it_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("centinel.db");
        v1_index_at(&path);

        Index::open(&path).unwrap();
        let idx = Index::open(&path).unwrap();
        assert_eq!(idx.stats().unwrap().placements, 1);
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
