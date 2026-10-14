//! The vector store — one row per chunk placed in a document, in LanceDB.
//!
//! ```text
//!   <root>/centinel.db        SQLite: metadata + FTS5   — the BM25 arm
//!   <root>/chunks.lance/      LanceDB: vectors          — this
//! ```
//!
//! Vectors are written here by `embed` and read here by `search`. There is no second
//! copy.
//!
//! ## What a row is
//!
//! A chunk **placed in a document**: the chunk's hash, the document's source, address and
//! derived text, the document's tags and whether it is excluded, and the vector. The same
//! passage on fifty pages is fifty rows carrying one vector, because what a search is
//! narrowed to — a source, a tag, inclusion — is a fact about the document, and a filter
//! that lives on the row is applied inside the vector query. The arm used to fetch five
//! times its depth and throw candidates away in SQLite, and under a narrow filter it came
//! back short; now it asks Lance for what qualifies.
//!
//! `embed` keeps the table to the index in both directions: [`VectorTable::append`] adds
//! the placements the index has and the table lacks, [`VectorTable::remove`] drops the
//! rows for placements the index no longer has. The tags and exclusion on a row are what
//! the workspace said when the row was written; the workspace queues every document whose
//! tags or exclusion change, and [`VectorTable::set_metadata`] rewrites those two columns
//! in place — Lance patches the columns inside the existing fragments and never touches a
//! vector. Every one of those writes, and maintenance and repair, happens under one file
//! lock, `workspace/vectors.lock`: each writing method takes a [`VectorLock`], so a write
//! without it does not compile, and each starts from the table's latest version.
//!
//! ## What a filter costs
//!
//! With an IVF index, a filter is applied **before** distance: Lance reads the probed
//! partitions, masks the rows the filter rejects, and scores the rest. The masked rows
//! cost no arithmetic, but their partitions are still read, so a query narrowed to one
//! source of thirty reads about as many bytes as an unfiltered one and returns the right
//! rows. Partitioning the table physically by source would change that, and is not done
//! here. `examples/vector_bench.rs` measures the filtered case beside the plain one.
//!
//! ## Why there is no second copy
//!
//! There used to be one: a flat append-only cache beside the static files, which SPEC
//! §5.2 called a durable Tier A artifact on the argument that *"swapping vector backends
//! is a re-import, not a re-embed."* That argument does not survive measurement. A
//! `.lance` dataset is an ordinary directory — a manifest, a transaction log, and data
//! files — so `cp -R` copies it, the copy opens and queries, and a plain scan reads every
//! vector back out. Extracting vectors from Lance is a table scan. Publishing the corpus
//! is a directory copy. Backing it up is the same.
//!
//! Each append is a Lance commit — a fragment of that batch's rows and a manifest listing
//! every fragment so far — and a query reads every fragment. Nothing in Lance removes
//! either on its own; on one 1.3-million-vector corpus that was 20,000 fragments, 20,000
//! manifests and 16 GiB of them. [`VectorTable::maintain`] is what consolidates them, and
//! it is also what builds the scalar indexes the filters are served from.
//!
//! ## The guard
//!
//! A query vector and the vectors it searches must come from the same space (SPEC §6.2).
//! Two models produce plausible, silently incomparable results, so the [`Space`] — the
//! model, and the server that ran it — lives in the table's schema metadata and a mismatch
//! is refused at [`VectorTable::open`] rather than discovered in bad rankings. The one way
//! to move a table to another space without re-embedding it is to measure that the new one
//! reproduces it: [`VectorTable::vectors_of`] gives the stored vectors to compare against,
//! and [`VectorTable::adopt`] rewrites the record once they agree. The layout is recorded
//! beside it. A store that holds only the older `vectors` table — one row per chunk, no
//! document, no filter columns — is refused with the way forward ([`refuse_old_layout`]):
//! the release before `[inference]` upgrades it with nothing sent to a model, and this one
//! no longer reads it, because a copy matched on a model name alone would skip the
//! measurement `--adopt` exists for.
//!
//! Width is guarded by the schema itself: the column is a `FixedSizeList` of exactly
//! `dims` floats, so a wrong width cannot be written at all.
//!
//! ## When a fragment cannot be read
//!
//! A data file is written once and never rewritten in place. Storage that drops a write
//! under load — the field case was a USB drive — leaves a file of the right length with
//! nothing in it: the manifest lists the fragment, Lance opens the file and finds no
//! footer, and every full scan fails. `embed`'s pre-flight read of the keys is the first
//! to go, which blocks the stage on one file among thousands of good ones.
//!
//! [`VectorTable::audit`] reads every fragment back and names the ones that cannot be
//! read; [`VectorTable::repair`] drops them from the manifest in one commit. Nothing is
//! recovered, because the bytes never landed, and nothing needs to be: a row is a chunk
//! and a document the index still has, and a dropped fragment's rows simply fall back onto
//! `embed`'s work list. This module owns the Lance side of that because it owns the
//! table; the `verify` op is the thin thing that asks.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use futures::TryStreamExt;
use lance::dataset::fragment::FileFragment;
use lance::dataset::transaction::{Operation, Transaction};
use lance::dataset::write::CommitBuilder;
use lancedb::arrow::arrow_array::builder::{ListBuilder, StringBuilder};
use lancedb::arrow::arrow_array::{
    Array, BooleanArray, FixedSizeListArray, Float32Array, RecordBatch, RecordBatchIterator,
    RecordBatchReader, StringArray, types::Float32Type,
};
use lancedb::arrow::arrow_schema::{DataType, Field, Schema, SchemaRef};
use lancedb::index::scalar::{BTreeIndexBuilder, BitmapIndexBuilder, LabelListIndexBuilder};
use lancedb::index::{Index as LanceIndex, vector::IvfFlatIndexBuilder};
use lancedb::query::{ExecutableQuery, QueryBase, Select};
use lancedb::table::OptimizeAction;
use lancedb::{DistanceType, Table};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::index::Filter;
use crate::inference::Endpoint;
use crate::op::{Cancel, Progress};
use crate::store::Store;
use crate::workspace::{DocState, DocumentId};

/// The table inside the database, which is what names `chunks.lance` on disk.
pub const TABLE: &str = "chunks";

/// The table the layout before this one wrote: `(chunk_hash, vector)`, one row per chunk.
/// Never read or written; named so a store holding only it is told what to do.
const LEGACY_TABLE: &str = "vectors";

const KEY_COLUMN: &str = "key";
const HASH_COLUMN: &str = "chunk_hash";
const SOURCE_COLUMN: &str = "source";
const RESOURCE_COLUMN: &str = "resource";
const DERIVED_COLUMN: &str = "derived_sha";
const TAGS_COLUMN: &str = "tags";
const EXCLUDED_COLUMN: &str = "excluded";
const VECTOR_COLUMN: &str = "vector";
/// Every column, in schema order, for a read that has to touch them all.
const ALL_COLUMNS: [&str; 8] = [
    KEY_COLUMN,
    HASH_COLUMN,
    SOURCE_COLUMN,
    RESOURCE_COLUMN,
    DERIVED_COLUMN,
    TAGS_COLUMN,
    EXCLUDED_COLUMN,
    VECTOR_COLUMN,
];
/// Where the model id is recorded. Namespaced, because the schema metadata is a shared
/// map and Lance writes its own keys into it.
const MODEL_KEY: &str = "centinel.model_id";
/// Where the server that ran the model is recorded — its `base_url`. Absent on a table
/// written before `[inference]`, when the model id alone said where it ran.
const ENDPOINT_KEY: &str = "centinel.endpoint";
/// Where the layout is recorded. `2` is this one.
const LAYOUT_KEY: &str = "centinel.layout";
const LAYOUT: &str = "2";
/// Lance's name for the column a vector query scores into.
const DISTANCE_COLUMN: &str = "_distance";

/// Between the parts of a row's key. A control character no URL and no hash contains, so
/// the key splits back into its parts without escaping.
const KEY_SEPARATOR: char = '\u{1f}';

/// How long a superseded table version stays on disk after [`VectorTable::maintain`].
///
/// Long enough for a `search` that opened the table a moment ago to finish reading the
/// files it opened — `serve` opens the table per query, and a query is seconds. Short
/// enough that the manifests one run leaves behind are gone by the next. The table is
/// derived, so old versions buy no rollback worth their size.
const VERSION_RETENTION: Duration = Duration::from_secs(10 * 60);

/// Below this many rows an ANN index is not worth its partition loss; exact search is
/// fast enough and loses nothing.
const ANN_MIN_ROWS: usize = 4096;

/// How many times [`VectorTable::nearest`] doubles its fetch when shared passages leave it
/// short of distinct chunks. A chunk carried by hundreds of pages fills a page of rows on
/// its own; four doublings reach sixteen times the ask before the arm settles for less.
const NEAREST_ROUNDS: u32 = 4;

/// Rows per `IN (…)` list when rows are deleted or vectors fetched by hash. Lance parses
/// the predicate, and a list of a thousand quoted hashes is what it parses comfortably.
const PREDICATE_ROWS: usize = 1_000;

/// Rows per batch when [`VectorTable::audit`] reads a fragment back.
///
/// The read is for the error, not the data, so the batch only has to be small enough that
/// a fragment is never held whole: at 2,560 dimensions this is about ten megabytes.
const AUDIT_BATCH_ROWS: usize = 1024;

/// Fragments between progress lines in [`VectorTable::audit`].
const AUDIT_PROGRESS_EVERY: usize = 500;

/// The key of the row for `chunk` placed in `document`.
pub fn key_of(chunk_hash: &str, document: &DocumentId) -> String {
    let mut key = String::with_capacity(
        chunk_hash.len()
            + document.source.len()
            + document.resource.len()
            + document.derived_sha.len()
            + 3,
    );
    key.push_str(chunk_hash);
    key.push(KEY_SEPARATOR);
    key.push_str(&document.source);
    key.push(KEY_SEPARATOR);
    key.push_str(&document.resource);
    key.push(KEY_SEPARATOR);
    key.push_str(&document.derived_sha);
    key
}

/// A key back into its chunk hash and document. `None` for a key this module did not
/// write.
pub fn parse_key(key: &str) -> Option<(&str, DocumentId)> {
    let mut parts = key.split(KEY_SEPARATOR);
    let chunk_hash = parts.next()?;
    let source = parts.next()?;
    let resource = parts.next()?;
    let derived_sha = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    Some((
        chunk_hash,
        DocumentId {
            source: source.to_owned(),
            resource: resource.to_owned(),
            derived_sha: derived_sha.to_owned(),
        },
    ))
}

/// One row on its way in: a chunk placed in a document, what the workspace says about
/// the document, and the vector.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub chunk_hash: String,
    pub document: DocumentId,
    pub state: DocState,
    pub vector: Vec<f32>,
}

/// One answer from [`VectorTable::nearest`]: the chunk, the document the filter admitted
/// it through, and how near it was.
#[derive(Clone, Debug, PartialEq)]
pub struct Nearest {
    pub chunk_hash: String,
    pub document: DocumentId,
    /// Cosine similarity: higher is nearer, 1.0 is the query itself.
    pub similarity: f32,
}

/// What [`VectorTable::maintain`] did to the files behind the table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Maintenance {
    /// Per-append fragments rewritten into fewer, larger ones.
    pub fragments_removed: usize,
    pub fragments_added: usize,
    /// Superseded versions whose manifests and data files were deleted.
    pub versions_removed: u64,
    pub bytes_removed: u64,
}

/// One fragment the table cannot read back.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct UnreadableFragment {
    /// Lance's id for the fragment — what [`VectorTable::repair`] drops.
    pub id: u64,
    /// Its data files, relative to `chunks.lance/`, for whoever wants to look at the
    /// bytes themselves.
    pub data_files: Vec<String>,
    /// How many rows the manifest says the fragment held. An **estimate**: the file cannot
    /// be asked, and the manifest's figure counts rows since deleted from the fragment
    /// beside the ones still live. `None` on a table old enough to predate the field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub physical_rows: Option<usize>,
    /// What Lance said when the read failed.
    pub error: String,
}

/// What [`VectorTable::audit`] found.
#[derive(Clone, Debug)]
pub struct VectorAudit {
    /// Fragments in the table at the version audited.
    pub fragments: usize,
    pub unreadable: Vec<UnreadableFragment>,
    /// The version the fragments were read at. Private, because it exists for one reason:
    /// [`VectorTable::repair`] commits against it, so that a commit which landed in
    /// between — a compaction that rewrote these very fragments — is caught by Lance's
    /// conflict detection rather than letting a stale id drop a fragment the audit never
    /// looked at.
    version: u64,
}

impl VectorAudit {
    pub fn readable(&self) -> usize {
        self.fragments - self.unreadable.len()
    }

    pub fn is_clean(&self) -> bool {
        self.unreadable.is_empty()
    }
}

/// What [`VectorTable::repair`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VectorRepair {
    pub fragments_dropped: usize,
    /// Rows that went with them, summed from each fragment's manifest figure — an estimate
    /// for the reason [`UnreadableFragment::physical_rows`] gives. Every one of them is
    /// work for the next `embed`.
    pub estimated_rows: usize,
}

/// Which space a table's vectors are in: the model that wrote them, and where it ran.
///
/// Where it ran is part of it because the space is a property of the model *as served* —
/// the same weights at two quantizations, or behind two servers' pooling, are close and
/// not identical, and the difference shows up only as worse rankings. Two spaces are
/// treated as one only once `centinel embed --adopt` has measured that they agree.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Space {
    /// The model, as the server was asked for it.
    pub model: String,
    /// The server's `base_url`. `None` on a table written before `[inference]` existed,
    /// when `qwen3-embedding-4b` meant weights in this process and `openrouter/…` meant
    /// OpenRouter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
}

impl Space {
    /// The space an endpoint's vectors are in.
    pub fn of(endpoint: &Endpoint) -> Self {
        Self {
            model: endpoint.model.clone(),
            endpoint: Some(endpoint.base_url.clone()),
        }
    }

    fn read(schema: &Schema) -> Option<Self> {
        Some(Self {
            model: schema.metadata().get(MODEL_KEY)?.clone(),
            endpoint: schema.metadata().get(ENDPOINT_KEY).cloned(),
        })
    }
}

impl std::fmt::Display for Space {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.endpoint {
            Some(endpoint) => write!(f, "{} at {endpoint}", self.model),
            None if self.model.starts_with("openrouter/") => f.write_str(&self.model),
            None => write!(f, "{} (run in-process, before [inference])", self.model),
        }
    }
}

/// A LanceDB table of chunk placements and their vectors, for one space.
///
/// Cheap to clone — a `Table` is a handle.
#[derive(Clone)]
pub struct VectorTable {
    table: Table,
    schema: SchemaRef,
    space: Space,
    dims: usize,
}

/// Names the space and the width, not the Lance handle — which is a connection pool and
/// has nothing a reader of a test failure wants.
impl std::fmt::Debug for VectorTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VectorTable")
            .field("space", &self.space)
            .field("dims", &self.dims)
            .finish_non_exhaustive()
    }
}

/// Held for the length of one write to the table: the one writer at a time, across
/// processes — `embed` in a terminal, the web server carrying tags onto rows. Every method
/// that writes asks for one. Released when dropped.
pub struct VectorLock {
    _file: std::fs::File,
}

impl VectorLock {
    /// Waits for `workspace/vectors.lock`, on a blocking thread: another writer may hold
    /// it for the length of an append, and the async runtime must not wait with it.
    pub async fn take(store: &Store) -> anyhow::Result<Self> {
        let path = store.vectors_lock_path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)?;
        let file = tokio::task::spawn_blocking(move || {
            file.lock()?;
            Ok::<_, std::io::Error>(file)
        })
        .await??;
        Ok(Self { _file: file })
    }
}

async fn connect(dir: &Path) -> anyhow::Result<lancedb::Connection> {
    let dir = dir
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("{} is not valid UTF-8", dir.display()))?;
    Ok(lancedb::connect(dir).execute().await?)
}

async fn has_table(db: &lancedb::Connection, name: &str) -> anyhow::Result<bool> {
    Ok(db.table_names().execute().await?.iter().any(|n| n == name))
}

impl VectorTable {
    /// Opens or creates the table for a space, validating an existing one.
    ///
    /// Takes the database directory rather than the store root: where the table lives
    /// under a store is [`crate::store::Store::vectors_path`]'s business, and this module
    /// knowing that layout is how a path ends up spelled out in two places.
    pub async fn open(dir: &Path, space: &Space, dims: usize) -> anyhow::Result<Self> {
        anyhow::ensure!(dims > 0, "a model with zero dimensions cannot be stored");

        let db = connect(dir).await?;
        let schema = schema_for(space, dims);

        let table = if has_table(&db, TABLE).await? {
            db.open_table(TABLE).execute().await?
        } else {
            match db.create_empty_table(TABLE, schema.clone()).execute().await {
                Ok(table) => table,
                // Another process created it between the look and the create.
                Err(lancedb::Error::TableAlreadyExists { .. }) => {
                    db.open_table(TABLE).execute().await?
                }
                Err(error) => return Err(error.into()),
            }
        };
        validate(table.schema().await?.as_ref(), space, dims)?;

        Ok(Self {
            table,
            schema,
            space: space.clone(),
            dims,
        })
    }

    /// Opens an existing table, taking the space and the width **from it**.
    ///
    /// For readers. `search` has no business being told which embedder to use: the answer
    /// is a property of the table, and a reader that was configured differently would
    /// have its query refused by [`Self::open`]'s guard and quietly fall back to one arm.
    /// Asking the table removes the question.
    pub async fn open_existing(dir: &Path) -> anyhow::Result<Self> {
        let db = connect(dir).await?;
        if !has_table(&db, TABLE).await? {
            refuse_old_layout(dir).await?;
            anyhow::bail!(
                "no vectors at {} — run `centinel embed` first",
                dir.join(format!("{TABLE}.lance")).display()
            );
        }

        let table = db.open_table(TABLE).execute().await?;
        let schema = table.schema().await?;
        let dims = vector_width(&schema)?;
        let space = Space::read(&schema).ok_or_else(|| {
            anyhow::anyhow!(
                "`{TABLE}` does not record which model wrote it — \
                 delete it and re-run `centinel embed`"
            )
        })?;

        Ok(Self {
            table,
            schema,
            space,
            dims,
        })
    }

    pub fn dims(&self) -> usize {
        self.dims
    }

    pub fn space(&self) -> &Space {
        &self.space
    }

    /// Records that this table's vectors are in `space` from now on. One manifest commit;
    /// no row is touched.
    ///
    /// Only for a space measured to reproduce this one — see `centinel embed --adopt`,
    /// which is the one caller. The rest of the schema metadata is carried over, because
    /// the write replaces the whole map and Lance keeps its own keys in it.
    pub async fn adopt(&mut self, _lock: &VectorLock, space: &Space) -> anyhow::Result<()> {
        self.table.checkout_latest().await?;
        let mut metadata = self.table.schema().await?.metadata().clone();
        metadata.insert(MODEL_KEY.to_string(), space.model.clone());
        match &space.endpoint {
            Some(endpoint) => metadata.insert(ENDPOINT_KEY.to_string(), endpoint.clone()),
            None => metadata.remove(ENDPOINT_KEY),
        };
        self.table
            .as_native()
            .ok_or_else(not_a_local_table)?
            .replace_schema_metadata(metadata)
            .await?;
        tracing::info!(from = %self.space, to = %space, "vector table adopted");
        self.space = space.clone();
        self.schema = schema_for(space, self.dims);
        Ok(())
    }

    /// How many rows are stored.
    pub async fn len(&self) -> anyhow::Result<usize> {
        Ok(self.table.count_rows(None).await?)
    }

    pub async fn is_empty(&self) -> anyhow::Result<bool> {
        Ok(self.len().await? == 0)
    }

    /// Every stored key, handed to `visit` one at a time and never held whole.
    ///
    /// One column, so the scan reads keys and never touches the vectors beside them. On
    /// a full corpus the keys are a hundred megabytes and the vectors are gigabytes, and
    /// `embed` asks this at the start of every run. Returns how many there were.
    pub async fn keys(&self, mut visit: impl FnMut(&str)) -> anyhow::Result<usize> {
        // No `limit`, which on a plain scan means every row. Lance only applies its
        // default top-k to vector and full-text queries.
        let mut stream = self
            .table
            .query()
            .select(Select::Columns(vec![KEY_COLUMN.to_string()]))
            .execute()
            .await?;

        let mut count = 0;
        while let Some(batch) = stream.try_next().await? {
            let keys = strings(&batch, KEY_COLUMN)?;
            for i in 0..keys.len() {
                visit(keys.value(i));
                count += 1;
            }
        }
        Ok(count)
    }

    /// Appends rows in one durable commit.
    ///
    /// Lance commits a version per call, so an interrupted run keeps every batch that
    /// landed before the kill and the work list is recomputed from what is there. That is
    /// what makes `embed` resumable without a checkpoint file.
    pub async fn append(&self, _lock: &VectorLock, rows: &[Row]) -> anyhow::Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        self.table.checkout_latest().await?;
        for row in rows {
            anyhow::ensure!(
                row.vector.len() == self.dims,
                "vector for {} has {} dimensions, expected {}",
                row.chunk_hash,
                row.vector.len(),
                self.dims
            );
        }

        let keys =
            StringArray::from_iter_values(rows.iter().map(|r| key_of(&r.chunk_hash, &r.document)));
        let hashes = StringArray::from_iter_values(rows.iter().map(|r| r.chunk_hash.as_str()));
        let sources =
            StringArray::from_iter_values(rows.iter().map(|r| r.document.source.as_str()));
        let resources =
            StringArray::from_iter_values(rows.iter().map(|r| r.document.resource.as_str()));
        let derived =
            StringArray::from_iter_values(rows.iter().map(|r| r.document.derived_sha.as_str()));
        let (tags, excluded) = metadata_columns(rows.iter().map(|r| &r.state));
        let vectors = FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
            rows.iter()
                .map(|r| Some(r.vector.iter().copied().map(Some).collect::<Vec<_>>())),
            self.dims as i32,
        );
        let batch = RecordBatch::try_new(
            self.schema.clone(),
            vec![
                Arc::new(keys),
                Arc::new(hashes),
                Arc::new(sources),
                Arc::new(resources),
                Arc::new(derived),
                Arc::new(tags),
                Arc::new(excluded),
                Arc::new(vectors),
            ],
        )?;

        let reader = Box::new(RecordBatchIterator::new(
            vec![Ok(batch)],
            self.schema.clone(),
        )) as Box<dyn RecordBatchReader + Send>;
        let started = std::time::Instant::now();
        self.table.add(reader).execute().await?;
        tracing::debug!(
            rows = rows.len(),
            ms = started.elapsed().as_millis() as u64,
            "appended to the vector table"
        );
        Ok(())
    }

    /// Deletes the rows with these keys.
    ///
    /// The other half of [`Self::append`]: that adds what the index has and the table
    /// lacks, this removes what the table has and the index lacks. By key list rather than
    /// by a merge against everything to keep, because what the index lacks is usually a
    /// cleared source — thousands of rows against a million kept — and the million is the
    /// expensive side to send.
    pub async fn remove(&self, _lock: &VectorLock, keys: &[String]) -> anyhow::Result<()> {
        self.table.checkout_latest().await?;
        for chunk in keys.chunks(PREDICATE_ROWS) {
            self.table
                .delete(&format!("{KEY_COLUMN} IN ({})", quoted_list(chunk)))
                .await?;
        }
        Ok(())
    }

    /// Rewrites the tags and exclusion of the rows with these keys, and nothing else.
    ///
    /// A merge on the key with a source that carries only the two metadata columns. Lance
    /// treats a partial-schema source as a column rewrite: the two columns are patched
    /// inside the fragments that hold the rows, and the vector column is never read or
    /// written. Keys the table does not hold are ignored — a document's chunks may not be
    /// embedded yet, and when they are, they arrive with the state of that moment.
    pub async fn set_metadata(
        &self,
        _lock: &VectorLock,
        rows: &[(String, DocState)],
    ) -> anyhow::Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        self.table.checkout_latest().await?;
        let schema = Arc::new(Schema::new(vec![
            Field::new(KEY_COLUMN, DataType::Utf8, false),
            tags_field(),
            Field::new(EXCLUDED_COLUMN, DataType::Boolean, false),
        ]));
        let keys = StringArray::from_iter_values(rows.iter().map(|(key, _)| key.as_str()));
        let (tags, excluded) = metadata_columns(rows.iter().map(|(_, state)| state));
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![Arc::new(keys), Arc::new(tags), Arc::new(excluded)],
        )?;
        let reader = Box::new(RecordBatchIterator::new(vec![Ok(batch)], schema))
            as Box<dyn RecordBatchReader + Send>;

        let mut merge = self.table.merge_insert(&[KEY_COLUMN]);
        merge.when_matched_update_all(None);
        let result = merge.execute(reader).await?;
        tracing::debug!(
            rows = rows.len(),
            updated = result.num_updated_rows,
            "vector metadata rewritten"
        );
        Ok(())
    }

    /// The vector stored for each of `hashes` that has a row, whichever document's.
    ///
    /// For `embed`: a chunk already embedded under one document is not embedded again for
    /// another, its vector is copied. Served by the scalar index on the hash once
    /// [`Self::maintain`] has built one; a full scan before then.
    pub async fn vectors_of(&self, hashes: &[String]) -> anyhow::Result<HashMap<String, Vec<f32>>> {
        let mut out = HashMap::new();
        for chunk in hashes.chunks(PREDICATE_ROWS) {
            let mut stream = self
                .table
                .query()
                .only_if(format!("{HASH_COLUMN} IN ({})", quoted_list(chunk)))
                .select(Select::Columns(vec![
                    HASH_COLUMN.to_string(),
                    VECTOR_COLUMN.to_string(),
                ]))
                .execute()
                .await?;
            while let Some(batch) = stream.try_next().await? {
                for (hash, vector) in hash_vectors(&batch)? {
                    out.entry(hash).or_insert(vector);
                }
            }
        }
        Ok(out)
    }

    /// Consolidates what [`Self::append`] and [`Self::remove`] leave behind, and builds
    /// the indexes the filters are served from.
    ///
    /// Compaction rewrites the per-append fragments into few and writes deleted rows out
    /// for good; the scalar indexes — the key the merges join on, the hash `embed` copies
    /// by, and the source, tags and exclusion a search filters on — are created when
    /// missing and brought up to date when present; then every version older than
    /// `VERSION_RETENTION` (ten minutes) is dropped with its files. Creating an ANN index
    /// stays opt-in through `build_index` — small corpora keep exact search, and IVF_FLAT
    /// keeps full-precision vectors, so only partition selection is approximate. An index
    /// that exists is brought up to date either way.
    pub async fn maintain(
        &self,
        lock: &VectorLock,
        build_index: bool,
    ) -> anyhow::Result<Maintenance> {
        self.maintain_keeping(lock, build_index, VERSION_RETENTION)
            .await
    }

    /// [`Self::maintain`] with the retention spelled out, so a test can watch versions go.
    async fn maintain_keeping(
        &self,
        _lock: &VectorLock,
        build_index: bool,
        retention: Duration,
    ) -> anyhow::Result<Maintenance> {
        self.table.checkout_latest().await?;
        let compacted = self
            .table
            .optimize(OptimizeAction::Compact {
                options: Default::default(),
                remap_options: None,
            })
            .await?;

        let rows = self.len().await?;
        let existing: HashSet<String> = self
            .table
            .list_indices()
            .await?
            .into_iter()
            .flat_map(|index| index.columns)
            .collect();
        // A scalar index over nothing has nothing to say; the first append is soon.
        if rows > 0 {
            for column in [
                KEY_COLUMN,
                HASH_COLUMN,
                SOURCE_COLUMN,
                TAGS_COLUMN,
                EXCLUDED_COLUMN,
            ] {
                if !existing.contains(column) {
                    self.table
                        .create_index(&[column], scalar_index(column))
                        .execute()
                        .await?;
                }
            }
        }
        let indexed = existing.contains(VECTOR_COLUMN);
        if !indexed && build_index && rows >= ANN_MIN_ROWS {
            self.table
                .create_index(
                    &[VECTOR_COLUMN],
                    LanceIndex::IvfFlat(
                        IvfFlatIndexBuilder::default()
                            .distance_type(DistanceType::Cosine)
                            .num_partitions((rows as f64).sqrt() as u32),
                    ),
                )
                .execute()
                .await?;
        }
        if rows > 0 {
            self.table
                .optimize(OptimizeAction::Index(Default::default()))
                .await?;
        }

        // Last, so the versions the compaction and the indexes just superseded are among
        // what goes.
        // Lance measures the retention in chrono's `Duration`, which it re-exports; the
        // conversion fails only for a span longer than chrono can count, which ten
        // minutes is not.
        let pruned = self
            .table
            .optimize(OptimizeAction::Prune {
                older_than: Some(lancedb::table::Duration::from_std(retention)?),
                delete_unverified: None,
                error_if_tagged_old_versions: None,
            })
            .await?;

        let (fragments_removed, fragments_added) = compacted
            .compaction
            .map(|c| (c.fragments_removed, c.fragments_added))
            .unwrap_or_default();
        let (versions_removed, bytes_removed) = pruned
            .prune
            .map(|p| (p.old_versions, p.bytes_removed))
            .unwrap_or_default();
        tracing::debug!(
            rows,
            indexed = indexed || (build_index && rows >= ANN_MIN_ROWS),
            fragments_removed,
            fragments_added,
            versions_removed,
            bytes_removed,
            "vector table maintained"
        );
        Ok(Maintenance {
            fragments_removed,
            fragments_added,
            versions_removed,
            bytes_removed,
        })
    }

    /// Reads every fragment back, every column, and names the ones that cannot be read.
    ///
    /// The same question every full scan asks, put one fragment at a time so the failure
    /// is attributed rather than merely reproduced. Every column, because a damaged file
    /// can fail in one and not another; batch by batch, so no fragment is held whole. The
    /// answer carries the version it was read at, for [`Self::repair`].
    pub async fn audit(&self, progress: &Progress, cancel: &Cancel) -> anyhow::Result<VectorAudit> {
        // Through the table rather than by reopening the path, so the audit reads the
        // version this handle sees — the one `repair` will then commit against.
        let dataset = self
            .table
            .dataset()
            .ok_or_else(not_a_local_table)?
            .get()
            .await?;
        let fragments = dataset.get_fragments();
        let total = fragments.len() as u64;

        let mut unreadable = Vec::new();
        for (i, fragment) in fragments.iter().enumerate() {
            cancel.check()?;
            // Every step is a log line, and the field table had 25,003 fragments.
            if i % AUDIT_PROGRESS_EVERY == 0 {
                progress.step("reading fragments back", i as u64, total);
            }
            if let Err(error) = read_back(fragment).await {
                let meta = fragment.metadata();
                // Said as it is found, since the progress lines above are sparse.
                tracing::warn!(fragment = meta.id, %error, "vector fragment cannot be read");
                unreadable.push(UnreadableFragment {
                    id: meta.id,
                    data_files: meta
                        .files
                        .iter()
                        .map(|f| format!("data/{}", f.path))
                        .collect(),
                    physical_rows: meta.physical_rows,
                    error: error.to_string(),
                });
            }
        }
        progress.step("reading fragments back", total, total);

        let version = dataset.version().version;
        tracing::debug!(
            fragments = total,
            unreadable = unreadable.len(),
            version,
            "vector table audited"
        );
        Ok(VectorAudit {
            fragments: fragments.len(),
            unreadable,
            version,
        })
    }

    /// Drops the fragments an [`audit`](Self::audit) could not read, in one commit.
    ///
    /// Touches the manifest and no data file: the fragment ids go, and with them the
    /// rows those fragments held, which puts them back on `embed`'s work list. Nothing
    /// here compacts or prunes; `embed` does both afterwards in its own `maintain`.
    ///
    /// The commit's read version is the audit's. If anything rewrote those fragments in
    /// between — a compaction from a concurrent `embed` — Lance's conflict detection
    /// refuses the commit and the caller audits again, rather than a stale id deleting a
    /// fragment the audit never saw. A clean audit commits nothing.
    pub async fn repair(
        &self,
        _lock: &VectorLock,
        audit: &VectorAudit,
        cancel: &Cancel,
    ) -> anyhow::Result<VectorRepair> {
        if audit.is_clean() {
            return Ok(VectorRepair::default());
        }
        cancel.check()?;

        let ids: Vec<u64> = audit.unreadable.iter().map(|f| f.id).collect();
        let transaction = Transaction::new(
            audit.version,
            Operation::Delete {
                updated_fragments: Vec::new(),
                deleted_fragment_ids: ids.clone(),
                // Descriptive, not a filter: these rows were never evaluated against one,
                // they were named by fragment because nothing could read them to be
                // filtered. It is what the transaction log shows for this version.
                predicate: format!("centinel verify --repair: unreadable fragments {ids:?}"),
            },
            None,
        );

        // The handle's type lives in a private module of lancedb, so it is used here and
        // never named: `get` for the dataset to commit against, `update` for the result.
        let handle = self.table.dataset().ok_or_else(not_a_local_table)?;
        let repaired = CommitBuilder::new(handle.get().await?)
            .execute(transaction)
            .await?;
        // lancedb refreshes its handle after its own writes; a commit it did not make has
        // to be handed to it, or a `keys()` on this same handle would go on reading the
        // fragment that is gone.
        handle.update(repaired);

        tracing::debug!(fragments = ids.len(), "unreadable fragments dropped");
        Ok(VectorRepair {
            fragments_dropped: ids.len(),
            estimated_rows: audit
                .unreadable
                .iter()
                .filter_map(|f| f.physical_rows)
                .sum(),
        })
    }

    /// The `limit` nearest **distinct chunks** to `query` among the rows the filter
    /// admits, best first, each with the document it was admitted through.
    ///
    /// Returns cosine **similarity**, not Lance's distance — higher is better, matching
    /// the BM25 arm, so a caller fusing the two never has to remember which way one of
    /// them points. The filter is applied inside the query, before distance; see the
    /// module notes for what that costs.
    pub async fn nearest(
        &self,
        query: &[f32],
        limit: usize,
        filter: &Filter<'_>,
    ) -> anyhow::Result<Vec<Nearest>> {
        self.nearest_with_index(query, limit, filter, true).await
    }

    /// Exhaustive cosine search, including when an ANN index exists.
    pub async fn nearest_exact(
        &self,
        query: &[f32],
        limit: usize,
        filter: &Filter<'_>,
    ) -> anyhow::Result<Vec<Nearest>> {
        self.nearest_with_index(query, limit, filter, false).await
    }

    /// Rows come back one per placement, so a passage shared by many documents fills many
    /// of the top `k`. The answer is deduplicated by chunk, keeping the first — the
    /// nearest — row of each; when that leaves fewer than `limit` and the fetch came back
    /// full, the fetch doubles, up to [`NEAREST_ROUNDS`] times.
    async fn nearest_with_index(
        &self,
        query: &[f32],
        limit: usize,
        filter: &Filter<'_>,
        use_index: bool,
    ) -> anyhow::Result<Vec<Nearest>> {
        anyhow::ensure!(
            query.len() == self.dims,
            "query vector has {} dimensions, this table holds {}",
            query.len(),
            self.dims
        );
        // Deliberately not an emptiness check. `count_rows` reads the whole table's
        // metadata, `search` already asks whether the table is empty before it queries,
        // and asking twice made a second full count the price of re-checking a condition
        // the caller had already rejected. An empty table simply returns no rows.
        if limit == 0 {
            return Ok(Vec::new());
        }
        let predicate = filter_sql(filter);
        let mut fetch = limit;
        let mut round = 0;
        loop {
            let rows = self
                .nearest_rows(query, fetch, &predicate, use_index)
                .await?;
            let full = rows.len() >= fetch;
            let mut seen = HashSet::new();
            let mut unique: Vec<Nearest> = rows
                .into_iter()
                .filter(|row| seen.insert(row.chunk_hash.clone()))
                .collect();
            round += 1;
            if unique.len() >= limit || !full || round >= NEAREST_ROUNDS {
                unique.truncate(limit);
                return Ok(unique);
            }
            fetch *= 2;
        }
    }

    async fn nearest_rows(
        &self,
        query: &[f32],
        limit: usize,
        predicate: &str,
        use_index: bool,
    ) -> anyhow::Result<Vec<Nearest>> {
        let search = self
            .table
            .query()
            .nearest_to(query.to_vec())?
            // Explicit, though the vectors are L2-normalized and so rank identically
            // under L2. Relying on that would make the ranking depend on a property of
            // the embedder that this module cannot see.
            .distance_type(DistanceType::Cosine)
            // The backend's 20-partition default lost too many candidates in the
            // synthetic recall benchmark. Search more partitions before reranking.
            .nprobes(64)
            // Before distance, which is lancedb's default for a filter on a vector query;
            // said here because the arm's correctness under a narrow filter depends on it.
            .only_if(predicate)
            .select(Select::Columns(vec![
                HASH_COLUMN.to_string(),
                SOURCE_COLUMN.to_string(),
                RESOURCE_COLUMN.to_string(),
                DERIVED_COLUMN.to_string(),
            ]))
            .limit(limit);
        let search = if use_index {
            search
        } else {
            search.bypass_vector_index()
        };
        let mut stream = search.execute().await?;

        let mut out = Vec::with_capacity(limit);
        while let Some(batch) = stream.try_next().await? {
            let hashes = strings(&batch, HASH_COLUMN)?;
            let sources = strings(&batch, SOURCE_COLUMN)?;
            let resources = strings(&batch, RESOURCE_COLUMN)?;
            let derived = strings(&batch, DERIVED_COLUMN)?;
            let distances = batch
                .column_by_name(DISTANCE_COLUMN)
                .and_then(|c| c.as_any().downcast_ref::<Float32Array>())
                .ok_or_else(|| anyhow::anyhow!("no `{DISTANCE_COLUMN}` in a vector result"))?;
            for i in 0..batch.num_rows() {
                out.push(Nearest {
                    chunk_hash: hashes.value(i).to_string(),
                    document: DocumentId {
                        source: sources.value(i).to_string(),
                        resource: resources.value(i).to_string(),
                        derived_sha: derived.value(i).to_string(),
                    },
                    similarity: 1.0 - distances.value(i),
                });
            }
        }
        Ok(out)
    }
}

/// Refuses a store whose only vectors are in the layout before this one — `vectors.lance`,
/// one row per chunk — naming the two ways forward. `Ok` when there is no such table.
///
/// For `embed` as much as for `search`: without it, `embed` would start the whole corpus
/// over beside the old table, which on a paid server is a bill nobody chose.
pub async fn refuse_old_layout(dir: &Path) -> anyhow::Result<()> {
    let db = connect(dir).await?;
    if has_table(&db, TABLE).await? || !has_table(&db, LEGACY_TABLE).await? {
        return Ok(());
    }
    anyhow::bail!(
        "the vectors at {} are in the one-row-per-chunk layout, which this release no longer \
         reads. Upgrade them with the release before this one — its `centinel embed --limit \
         0` fills chunks.lance with nothing sent to a model — or delete {0} and run \
         `centinel embed` to embed again",
        dir.join(format!("{LEGACY_TABLE}.lance")).display()
    )
}

/// The schema, and the one place the space and layout are written into it.
fn schema_for(space: &Space, dims: usize) -> SchemaRef {
    let mut metadata = HashMap::from([
        (MODEL_KEY.to_string(), space.model.clone()),
        (LAYOUT_KEY.to_string(), LAYOUT.to_string()),
    ]);
    if let Some(endpoint) = &space.endpoint {
        metadata.insert(ENDPOINT_KEY.to_string(), endpoint.clone());
    }
    Arc::new(Schema::new_with_metadata(
        vec![
            Field::new(KEY_COLUMN, DataType::Utf8, false),
            Field::new(HASH_COLUMN, DataType::Utf8, false),
            Field::new(SOURCE_COLUMN, DataType::Utf8, false),
            Field::new(RESOURCE_COLUMN, DataType::Utf8, false),
            Field::new(DERIVED_COLUMN, DataType::Utf8, false),
            tags_field(),
            Field::new(EXCLUDED_COLUMN, DataType::Boolean, false),
            Field::new(
                VECTOR_COLUMN,
                DataType::FixedSizeList(
                    Arc::new(Field::new("item", DataType::Float32, true)),
                    dims as i32,
                ),
                true,
            ),
        ],
        metadata,
    ))
}

/// The tags column: a list of strings, never null, empty for a document with no tags.
/// The item field is spelled the way the arrow list builder spells it, so a built array
/// matches the schema exactly.
fn tags_field() -> Field {
    Field::new(
        TAGS_COLUMN,
        DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
        false,
    )
}

/// The tags and exclusion columns for a run of states, in order.
fn metadata_columns<'a>(
    states: impl Iterator<Item = &'a DocState> + Clone,
) -> (lancedb::arrow::arrow_array::ListArray, BooleanArray) {
    let mut tags = ListBuilder::new(StringBuilder::new());
    for state in states.clone() {
        for tag in &state.tags {
            tags.values().append_value(tag);
        }
        tags.append(true);
    }
    let excluded = BooleanArray::from(states.map(|state| state.excluded).collect::<Vec<_>>());
    (tags.finish(), excluded)
}

/// The width of the vector column, or why there is no such thing.
/// The scalar index a column is served from: a bitmap for the few values a filter narrows
/// by, a label list for the tags a filter asks of a list, a B-tree for the keys a merge or
/// a copy looks up one at a time.
fn scalar_index(column: &str) -> LanceIndex {
    match column {
        SOURCE_COLUMN | EXCLUDED_COLUMN => LanceIndex::Bitmap(BitmapIndexBuilder::default()),
        TAGS_COLUMN => LanceIndex::LabelList(LabelListIndexBuilder::default()),
        _ => LanceIndex::BTree(BTreeIndexBuilder::default()),
    }
}

fn vector_width(schema: &Schema) -> anyhow::Result<usize> {
    match schema
        .field_with_name(VECTOR_COLUMN)
        .map_err(|_| anyhow::anyhow!("the vector table has no `{VECTOR_COLUMN}` column"))?
        .data_type()
    {
        DataType::FixedSizeList(_, n) => Ok(*n as usize),
        other => anyhow::bail!("`{VECTOR_COLUMN}` is {other}, not a fixed-size list of floats"),
    }
}

/// Refuses a table that was written in another space, at another width, or in another
/// layout.
///
/// All three are silent failures otherwise: vectors from two models are in different
/// spaces and still return a ranked list, and a table without the filter columns answers
/// no filtered query.
fn validate(found: &Schema, space: &Space, dims: usize) -> anyhow::Result<()> {
    let width = vector_width(found)?;
    anyhow::ensure!(
        width == dims,
        "`{TABLE}` holds {width}-dimensional vectors, not {dims} — \
         delete it and re-run `centinel embed`"
    );

    let found_space = Space::read(found);
    if found_space.as_ref() != Some(space) {
        anyhow::bail!("{}", mismatch(found_space.as_ref(), space));
    }
    let layout = found.metadata().get(LAYOUT_KEY).map(String::as_str);
    anyhow::ensure!(
        layout == Some(LAYOUT),
        "`{TABLE}` is in layout {}, not {LAYOUT} — delete it and re-run `centinel embed`",
        layout.unwrap_or("none"),
    );
    Ok(())
}

/// Why a table in one space cannot take vectors from another, and the two ways forward.
///
/// One sentence for `embed` and `search` alike, so both name the same command.
pub fn mismatch(found: Option<&Space>, wanted: &Space) -> String {
    match found {
        Some(found) => format!(
            "`{TABLE}` was written by {found}, and [inference.embed] names {wanted} — vectors \
             from two spaces are not comparable. If the new server runs the same model, \
             `centinel embed --adopt` measures whether it reproduces the stored vectors and \
             adopts the table if it does; otherwise delete {TABLE}.lance and re-run \
             `centinel embed`"
        ),
        None => format!(
            "`{TABLE}` does not record which model wrote it — delete it and re-run \
             `centinel embed`"
        ),
    }
}

/// The filter as the SQL Lance applies before distance. Exclusion first, always; the
/// label-list index serves `array_has_all` and `array_has_any`, the bitmap index the
/// equality on `source`.
fn filter_sql(filter: &Filter<'_>) -> String {
    let mut parts = vec![format!("{EXCLUDED_COLUMN} = false")];
    if let Some(source) = filter.source {
        parts.push(format!("{SOURCE_COLUMN} = {}", quoted(source)));
    }
    if !filter.tags.is_empty() {
        parts.push(format!(
            "array_has_all({TAGS_COLUMN}, [{}])",
            quoted_list(filter.tags)
        ));
    }
    if !filter.not_tags.is_empty() {
        parts.push(format!(
            "NOT array_has_any({TAGS_COLUMN}, [{}])",
            quoted_list(filter.not_tags)
        ));
    }
    parts.join(" AND ")
}

/// A string as a SQL literal: single quotes doubled, as the SQL standard escapes them.
fn quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn quoted_list(values: &[String]) -> String {
    values
        .iter()
        .map(|value| quoted(value))
        .collect::<Vec<_>>()
        .join(",")
}

/// Every table this module opens is local; lancedb's `None` exists for its remote tables.
fn not_a_local_table() -> anyhow::Error {
    anyhow::anyhow!("`{TABLE}` is not a local Lance table")
}

/// Reads one fragment back, every column, batch by batch, and returns Lance's own account
/// of why it could not.
///
/// Scoped to the fragment, so convicting one costs one fragment's I/O rather than a table
/// scan per candidate. The batches are drained and dropped: the question is whether the
/// bytes decode, not what they say.
async fn read_back(fragment: &FileFragment) -> lance::Result<()> {
    let mut scanner = fragment.scan();
    scanner.project(&ALL_COLUMNS)?.batch_size(AUDIT_BATCH_ROWS);
    let mut stream = scanner.try_into_stream().await?;
    while stream.try_next().await?.is_some() {}
    Ok(())
}

fn strings<'a>(batch: &'a RecordBatch, name: &str) -> anyhow::Result<&'a StringArray> {
    batch
        .column_by_name(name)
        .and_then(|c| c.as_any().downcast_ref::<StringArray>())
        .ok_or_else(|| anyhow::anyhow!("no `{name}` in a result batch"))
}

/// A batch's `(chunk_hash, vector)` pairs.
fn hash_vectors(batch: &RecordBatch) -> anyhow::Result<Vec<(String, Vec<f32>)>> {
    let hashes = strings(batch, HASH_COLUMN)?;
    let vectors = batch
        .column_by_name(VECTOR_COLUMN)
        .and_then(|c| c.as_any().downcast_ref::<FixedSizeListArray>())
        .ok_or_else(|| anyhow::anyhow!("no `{VECTOR_COLUMN}` in a result batch"))?;
    (0..batch.num_rows())
        .map(|i| {
            let values = vectors.value(i);
            let floats = values
                .as_any()
                .downcast_ref::<Float32Array>()
                .ok_or_else(|| {
                    anyhow::anyhow!("`{VECTOR_COLUMN}` holds something other than floats")
                })?;
            Ok((hashes.value(i).to_string(), floats.values().to_vec()))
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const DIMS: usize = 4;

    fn hash(byte: u8) -> String {
        hex::encode([byte; 32])
    }

    fn doc(n: u8) -> DocumentId {
        DocumentId {
            source: if n.is_multiple_of(2) {
                "tampa"
            } else {
                "pinellas"
            }
            .into(),
            resource: format!("https://x/{n}"),
            derived_sha: hex::encode([n; 32]),
        }
    }

    /// L2-normalized, so `nearest` returns a similarity in [-1, 1].
    fn unit(v: [f32; DIMS]) -> Vec<f32> {
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        v.iter().map(|x| x / norm).collect()
    }

    fn row(chunk: u8, document: u8, vector: Vec<f32>) -> Row {
        Row {
            chunk_hash: hash(chunk),
            document: doc(document),
            state: DocState::default(),
            vector,
        }
    }

    fn tagged(chunk: u8, document: u8, vector: Vec<f32>, tags: &[&str], excluded: bool) -> Row {
        Row {
            state: DocState {
                tags: tags.iter().map(|t| t.to_string()).collect(),
                excluded,
            },
            ..row(chunk, document, vector)
        }
    }

    /// The space every table here is written in.
    pub(crate) fn space(model: &str) -> Space {
        Space {
            model: model.to_string(),
            endpoint: Some("http://127.0.0.1:8080/v1".to_string()),
        }
    }

    async fn table(dir: &Path) -> VectorTable {
        VectorTable::open(dir, &space("test-model"), DIMS)
            .await
            .unwrap()
    }

    /// The write lock for the table under `dir`, taken for one statement at a time.
    async fn lock(dir: &Path) -> VectorLock {
        VectorLock::take(&Store::at(dir)).await.unwrap()
    }

    async fn all_keys(t: &VectorTable) -> HashSet<String> {
        let mut keys = HashSet::new();
        t.keys(|key| {
            keys.insert(key.to_string());
        })
        .await
        .unwrap();
        keys
    }

    fn only(hits: &[Nearest]) -> Vec<String> {
        hits.iter().map(|h| h.chunk_hash.clone()).collect()
    }

    fn in_source(source: &str) -> Filter<'_> {
        Filter {
            source: Some(source),
            ..Default::default()
        }
    }

    /// `.manifest` files under the table — one per version Lance still holds.
    fn manifests(dir: &Path) -> usize {
        std::fs::read_dir(dir.join("chunks.lance").join("_versions"))
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|x| x == "manifest")
            })
            .count()
    }

    #[test]
    fn a_key_round_trips_through_its_parts() {
        let key = key_of(&hash(7), &doc(3));
        let (chunk, document) = parse_key(&key).unwrap();
        assert_eq!(chunk, hash(7));
        assert_eq!(document, doc(3));
        assert!(parse_key("not a key").is_none());
    }

    #[tokio::test]
    async fn maintenance_indexes_large_tables_and_preserves_appended_rows() {
        let dir = tempfile::tempdir().unwrap();
        let t = table(dir.path()).await;
        let rows: Vec<Row> = (0..4096u32)
            .map(|i| {
                // Deterministic, distinct directions without an additional RNG dependency.
                let x = (i.wrapping_mul(1664525).wrapping_add(1013904223)) as f32;
                let y = (i.wrapping_mul(22695477).wrapping_add(1)) as f32;
                Row {
                    chunk_hash: format!("{i:064x}"),
                    document: doc((i % 7) as u8),
                    state: DocState::default(),
                    vector: unit([x, y, i as f32 * 1000000.0, 1.0]),
                }
            })
            .collect();
        t.append(&lock(dir.path()).await, &rows[..2048])
            .await
            .unwrap();
        t.append(&lock(dir.path()).await, &rows[2048..])
            .await
            .unwrap();
        t.maintain(&lock(dir.path()).await, false).await.unwrap();
        async fn indexed(t: &VectorTable) -> HashSet<String> {
            t.table
                .list_indices()
                .await
                .unwrap()
                .into_iter()
                .flat_map(|i| i.columns)
                .collect()
        }
        let scalar = indexed(&t).await;
        assert!(!scalar.contains(VECTOR_COLUMN), "ANN must be opt-in");
        for column in [
            KEY_COLUMN,
            HASH_COLUMN,
            SOURCE_COLUMN,
            TAGS_COLUMN,
            EXCLUDED_COLUMN,
        ] {
            assert!(scalar.contains(column), "no scalar index on {column}");
        }
        t.maintain(&lock(dir.path()).await, true).await.unwrap();
        assert!(indexed(&t).await.contains(VECTOR_COLUMN));
        let query = unit([-1.0, 0.0, 0.0, 0.0]);
        let added = "f".repeat(64);
        t.append(
            &lock(dir.path()).await,
            &[Row {
                chunk_hash: added.clone(),
                document: doc(1),
                state: DocState::default(),
                vector: query.clone(),
            }],
        )
        .await
        .unwrap();
        // Lance must scan the unindexed tail as well as searching the ANN index.
        assert_eq!(
            t.nearest(&query, 1, &Filter::default()).await.unwrap()[0].chunk_hash,
            added
        );
        t.maintain(&lock(dir.path()).await, true).await.unwrap();
        let reopened = table(dir.path()).await;
        assert_eq!(reopened.len().await.unwrap(), 4097);
        assert_eq!(all_keys(&reopened).await.len(), 4097);
        let hits = reopened
            .nearest(&query, 1, &Filter::default())
            .await
            .unwrap();
        assert_eq!(hits[0].chunk_hash, added);
        assert!((hits[0].similarity - 1.0).abs() < 1e-5);
        let exact = reopened
            .nearest_exact(&query, 4097, &Filter::default())
            .await
            .unwrap();
        assert_eq!(
            exact.len(),
            4097,
            "exact search bypasses partition selection"
        );
        assert_eq!(exact[0].chunk_hash, added);
        // The filter is served through the index too: one source of two, nearest first.
        let pinellas = reopened
            .nearest(&query, 5, &in_source("pinellas"))
            .await
            .unwrap();
        assert_eq!(pinellas[0].chunk_hash, added);
        assert!(pinellas.iter().all(|h| h.document.source == "pinellas"));
    }

    #[tokio::test]
    async fn maintenance_leaves_small_tables_on_exact_search() {
        let dir = tempfile::tempdir().unwrap();
        let t = table(dir.path()).await;
        t.append(
            &lock(dir.path()).await,
            &[row(1, 0, unit([1.0, 0.0, 0.0, 0.0]))],
        )
        .await
        .unwrap();
        t.maintain(&lock(dir.path()).await, true).await.unwrap();
        assert!(
            !t.table
                .list_indices()
                .await
                .unwrap()
                .iter()
                .any(|i| i.columns == [VECTOR_COLUMN])
        );
        assert_eq!(
            t.nearest(&[1.0, 0.0, 0.0, 0.0], 1, &Filter::default())
                .await
                .unwrap()[0]
                .chunk_hash,
            hash(1)
        );
    }

    /// The reconciling half of `embed`: a rebuilt index or a cleared source leaves rows
    /// no placement claims, and they must go without touching the rows one does.
    #[tokio::test]
    async fn remove_deletes_exactly_the_keys_it_is_given() {
        let dir = tempfile::tempdir().unwrap();
        let t = table(dir.path()).await;
        t.append(
            &lock(dir.path()).await,
            &[
                row(1, 0, unit([1.0, 0.0, 0.0, 0.0])),
                row(2, 0, unit([0.0, 1.0, 0.0, 0.0])),
                row(3, 1, unit([0.0, 0.0, 1.0, 0.0])),
            ],
        )
        .await
        .unwrap();

        // A key the table never had is not an error and not a row.
        t.remove(
            &lock(dir.path()).await,
            &[key_of(&hash(2), &doc(0)), key_of(&hash(9), &doc(0))],
        )
        .await
        .unwrap();
        assert_eq!(
            all_keys(&t).await,
            HashSet::from([key_of(&hash(1), &doc(0)), key_of(&hash(3), &doc(1))])
        );
        assert_eq!(t.len().await.unwrap(), 2);
        // Nearer to the deleted row than to either survivor, and nearer to `3` than to
        // `1` so the answer is not a tie between the survivors.
        assert_eq!(
            t.nearest(&unit([0.0, 0.8, 0.6, 0.0]), 1, &Filter::default())
                .await
                .unwrap()[0]
                .chunk_hash,
            hash(3),
            "the deleted row no longer answers queries"
        );
        t.remove(&lock(dir.path()).await, &[]).await.unwrap();
        assert_eq!(t.len().await.unwrap(), 2);
    }

    /// Every append is a version with its own manifest, and Lance never drops one on its
    /// own — 20,000 of them on the store that motivated this. Maintenance folds the
    /// fragments and removes the versions it superseded, and what is stored survives it.
    #[tokio::test]
    async fn maintenance_folds_appends_and_drops_superseded_versions() {
        let dir = tempfile::tempdir().unwrap();
        let t = table(dir.path()).await;
        for i in 1..=5u8 {
            t.append(
                &lock(dir.path()).await,
                &[row(i, 0, unit([i as f32, 1.0, 0.0, 0.0]))],
            )
            .await
            .unwrap();
        }
        t.remove(&lock(dir.path()).await, &[key_of(&hash(5), &doc(0))])
            .await
            .unwrap();
        let before = manifests(dir.path());
        assert!(
            before >= 6,
            "one version per append and one per delete: {before}"
        );

        let done = t
            .maintain_keeping(&lock(dir.path()).await, false, Duration::ZERO)
            .await
            .unwrap();

        // Four, not five: deleting the only row of a fragment empties it, and Lance drops
        // an empty fragment with the delete rather than leaving it for compaction.
        assert!(done.fragments_removed >= 4, "{done:?}");
        assert_eq!(done.fragments_added, 1, "{done:?}");
        assert!(done.versions_removed >= 5, "{done:?}");
        assert_eq!(manifests(dir.path()), 1, "only the current version remains");
        let reopened = table(dir.path()).await;
        assert_eq!(
            all_keys(&reopened).await,
            (1..=4u8)
                .map(|i| key_of(&hash(i), &doc(0)))
                .collect::<HashSet<_>>(),
            "the deleted row stays deleted and the rest stay"
        );
        assert_eq!(
            reopened
                .nearest(&unit([4.0, 1.0, 0.0, 0.0]), 1, &Filter::default())
                .await
                .unwrap()[0]
                .chunk_hash,
            hash(4)
        );
    }

    #[tokio::test]
    async fn a_new_table_is_empty_and_self_describing() {
        let dir = tempfile::tempdir().unwrap();
        let t = table(dir.path()).await;
        assert!(t.is_empty().await.unwrap());
        assert_eq!(t.dims(), DIMS);
        assert_eq!(t.space(), &space("test-model"));
        // Reopening validates the schema it just wrote.
        assert!(
            VectorTable::open(dir.path(), &space("test-model"), DIMS)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn rows_round_trip_with_their_documents_and_state() {
        let dir = tempfile::tempdir().unwrap();
        let t = table(dir.path()).await;
        t.append(
            &lock(dir.path()).await,
            &[
                tagged(1, 0, unit([1.0, 0.0, 0.0, 0.0]), &["budget"], false),
                tagged(1, 1, unit([1.0, 0.0, 0.0, 0.0]), &[], true),
            ],
        )
        .await
        .unwrap();

        assert_eq!(t.len().await.unwrap(), 2);
        assert_eq!(
            all_keys(&t).await,
            HashSet::from([key_of(&hash(1), &doc(0)), key_of(&hash(1), &doc(1))])
        );
        let vectors = t.vectors_of(&[hash(1), hash(2)]).await.unwrap();
        assert_eq!(
            vectors.len(),
            1,
            "one vector, however many documents carry it"
        );
        assert_eq!(vectors[&hash(1)], unit([1.0, 0.0, 0.0, 0.0]));
    }

    #[tokio::test]
    async fn appends_accumulate_across_reopens() {
        let dir = tempfile::tempdir().unwrap();
        table(dir.path())
            .await
            .append(
                &lock(dir.path()).await,
                &[row(1, 0, unit([1.0, 1.0, 1.0, 1.0]))],
            )
            .await
            .unwrap();
        table(dir.path())
            .await
            .append(
                &lock(dir.path()).await,
                &[row(2, 0, unit([1.0, 0.0, 1.0, 0.0]))],
            )
            .await
            .unwrap();
        assert_eq!(table(dir.path()).await.len().await.unwrap(), 2);
    }

    /// The one thing this table exists to answer.
    #[tokio::test]
    async fn nearest_ranks_by_similarity_with_higher_being_better() {
        let dir = tempfile::tempdir().unwrap();
        let t = table(dir.path()).await;
        t.append(
            &lock(dir.path()).await,
            &[
                row(1, 0, unit([1.0, 0.0, 0.0, 0.0])),
                row(2, 0, unit([0.9, 0.1, 0.0, 0.0])),
                row(3, 0, unit([0.0, 0.0, 0.0, 1.0])),
            ],
        )
        .await
        .unwrap();

        let hits = t
            .nearest(&unit([1.0, 0.0, 0.0, 0.0]), 3, &Filter::default())
            .await
            .unwrap();
        assert_eq!(hits.len(), 3);
        assert_eq!(
            hits[0].chunk_hash,
            hash(1),
            "the identical vector ranks first"
        );
        assert_eq!(hits[1].chunk_hash, hash(2));
        assert_eq!(hits[2].chunk_hash, hash(3), "the orthogonal one ranks last");
        assert!(
            hits[0].similarity > hits[1].similarity && hits[1].similarity > hits[2].similarity,
            "scores descend: {hits:?}"
        );
        assert!(
            (hits[0].similarity - 1.0).abs() < 1e-5,
            "a self-match is 1.0: {hits:?}"
        );
        assert_eq!(
            hits[0].document,
            doc(0),
            "the row names the document it cites"
        );
    }

    #[tokio::test]
    async fn nearest_honours_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let t = table(dir.path()).await;
        for i in 1..=5u8 {
            t.append(
                &lock(dir.path()).await,
                &[row(i, 0, unit([i as f32, 1.0, 0.0, 0.0]))],
            )
            .await
            .unwrap();
        }
        assert_eq!(
            t.nearest(&unit([1.0, 1.0, 0.0, 0.0]), 2, &Filter::default())
                .await
                .unwrap()
                .len(),
            2
        );
    }

    #[tokio::test]
    async fn an_empty_table_answers_nothing_rather_than_failing() {
        let dir = tempfile::tempdir().unwrap();
        let t = table(dir.path()).await;
        assert!(
            t.nearest(&unit([1.0, 0.0, 0.0, 0.0]), 10, &Filter::default())
                .await
                .unwrap()
                .is_empty()
        );
    }

    /// The filter is answered inside the query: a source whose rows are all farther away
    /// than another source's still gets its own nearest rows, in full, where the old
    /// post-filter came back with nothing.
    #[tokio::test]
    async fn a_source_filter_returns_that_sources_nearest_rows() {
        let dir = tempfile::tempdir().unwrap();
        let t = table(dir.path()).await;
        // Even chunks sit in tampa (docs 0, 2), near the query; odd ones in pinellas, far.
        let rows: Vec<Row> = (0..40u8)
            .map(|i| {
                let near = i % 2 == 0;
                let v = if near {
                    [1.0, 0.01 * i as f32, 0.0, 0.0]
                } else {
                    [0.0, 0.0, 1.0, 0.01 * i as f32]
                };
                row(i, if near { 0 } else { 1 }, unit(v))
            })
            .collect();
        t.append(&lock(dir.path()).await, &rows).await.unwrap();

        let query = unit([1.0, 0.0, 0.0, 0.0]);
        let pinellas = t.nearest(&query, 10, &in_source("pinellas")).await.unwrap();
        assert_eq!(pinellas.len(), 10, "a full page from the far source");
        assert!(pinellas.iter().all(|h| h.document.source == "pinellas"));
        let tampa = t.nearest(&query, 10, &in_source("tampa")).await.unwrap();
        assert!(tampa.iter().all(|h| h.document.source == "tampa"));
        assert_eq!(tampa[0].chunk_hash, hash(0));
        assert!(
            t.nearest(&query, 10, &in_source("nowhere"))
                .await
                .unwrap()
                .is_empty()
        );
    }

    /// Tags narrow the rows the way the keyword arm's `--tag` does: every tag must be on
    /// the document, `--not-tag` removes, and an excluded document never answers however
    /// near its vector is. Rewriting a document's metadata changes the answers and leaves
    /// its vectors byte for byte as they were.
    #[tokio::test]
    async fn tags_and_exclusion_filter_inside_the_query_and_metadata_rewrites_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let t = table(dir.path()).await;
        let v = |i: u8| unit([1.0, 0.1 * i as f32, 0.0, 0.0]);
        t.append(
            &lock(dir.path()).await,
            &[
                tagged(1, 0, v(1), &["budget", "record_type:minutes"], false),
                tagged(2, 1, v(2), &["budget"], false),
                tagged(3, 2, v(3), &["record_type:agenda"], false),
                tagged(4, 3, v(0), &["budget"], true),
            ],
        )
        .await
        .unwrap();
        let query = unit([1.0, 0.0, 0.0, 0.0]);
        let budget = vec!["budget".to_string()];
        let minutes = vec!["record_type:minutes".to_string()];
        let both = vec!["budget".to_string(), "record_type:minutes".to_string()];

        let all = t.nearest(&query, 10, &Filter::default()).await.unwrap();
        assert_eq!(
            only(&all),
            [hash(1), hash(2), hash(3)],
            "the excluded row never answers"
        );
        let with_budget = Filter {
            tags: &budget,
            ..Default::default()
        };
        assert_eq!(
            only(&t.nearest(&query, 10, &with_budget).await.unwrap()),
            [hash(1), hash(2)]
        );
        let anded = Filter {
            tags: &both,
            ..Default::default()
        };
        assert_eq!(
            only(&t.nearest(&query, 10, &anded).await.unwrap()),
            [hash(1)]
        );
        let without = Filter {
            not_tags: &minutes,
            ..Default::default()
        };
        assert_eq!(
            only(&t.nearest(&query, 10, &without).await.unwrap()),
            [hash(2), hash(3)]
        );
        let elsewhere = Filter {
            source: Some("pinellas"),
            tags: &budget,
            not_tags: &[],
        };
        assert_eq!(
            only(&t.nearest(&query, 10, &elsewhere).await.unwrap()),
            [hash(2)]
        );

        // The excluded document is restored and tagged; the budget on document 1 is
        // taken away. Only those two rows change, and no vector does.
        let before = t
            .vectors_of(&[hash(1), hash(2), hash(3), hash(4)])
            .await
            .unwrap();
        t.set_metadata(
            &lock(dir.path()).await,
            &[
                (
                    key_of(&hash(4), &doc(3)),
                    DocState {
                        tags: vec!["budget".into(), "record_type:minutes".into()],
                        excluded: false,
                    },
                ),
                (key_of(&hash(2), &doc(1)), DocState::default()),
                (key_of(&hash(9), &doc(9)), DocState::default()),
            ],
        )
        .await
        .unwrap();
        assert_eq!(
            only(&t.nearest(&query, 10, &anded).await.unwrap()),
            [hash(4), hash(1)]
        );
        assert_eq!(
            only(&t.nearest(&query, 10, &with_budget).await.unwrap()),
            [hash(4), hash(1)]
        );
        assert_eq!(
            t.len().await.unwrap(),
            4,
            "a key the table lacks inserts nothing"
        );
        let after = t
            .vectors_of(&[hash(1), hash(2), hash(3), hash(4)])
            .await
            .unwrap();
        assert_eq!(before, after, "no vector moved");
    }

    /// A passage carried by many documents is many rows with one vector. The arm wants
    /// distinct chunks, so the answer is deduplicated and the fetch widened until it has
    /// them.
    #[tokio::test]
    async fn nearest_returns_distinct_chunks_when_one_is_shared_by_many_documents() {
        let dir = tempfile::tempdir().unwrap();
        let t = table(dir.path()).await;
        let mut rows: Vec<Row> = (0..10u8)
            .map(|d| row(1, d, unit([1.0, 0.0, 0.0, 0.0])))
            .collect();
        rows.push(row(2, 0, unit([0.9, 0.1, 0.0, 0.0])));
        rows.push(row(3, 0, unit([0.8, 0.2, 0.0, 0.0])));
        t.append(&lock(dir.path()).await, &rows).await.unwrap();

        let hits = t
            .nearest(&unit([1.0, 0.0, 0.0, 0.0]), 3, &Filter::default())
            .await
            .unwrap();
        assert_eq!(only(&hits), [hash(1), hash(2), hash(3)]);
        let fewer = t
            .nearest(&unit([1.0, 0.0, 0.0, 0.0]), 2, &in_source("tampa"))
            .await
            .unwrap();
        assert_eq!(only(&fewer), [hash(1), hash(2)]);
        assert_eq!(fewer[0].document.source, "tampa");
    }

    /// `search` opens the table without being told which model wrote it, so that a
    /// reader configured differently cannot silently fall back to one arm.
    #[tokio::test]
    async fn open_existing_takes_the_model_and_width_from_the_table() {
        let dir = tempfile::tempdir().unwrap();
        table(dir.path())
            .await
            .append(
                &lock(dir.path()).await,
                &[row(1, 0, unit([1.0, 0.0, 0.0, 0.0]))],
            )
            .await
            .unwrap();

        let found = VectorTable::open_existing(dir.path()).await.unwrap();
        assert_eq!(found.space(), &space("test-model"));
        assert_eq!(found.dims(), DIMS);
        assert_eq!(found.len().await.unwrap(), 1);
    }

    /// The message a reader sees on a corpus that is indexed but not yet embedded — the
    /// ordinary state of a fresh crawl, so it has to name the way forward.
    #[tokio::test]
    async fn open_existing_on_an_unembedded_store_names_the_command() {
        let dir = tempfile::tempdir().unwrap();
        let err = VectorTable::open_existing(dir.path())
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("centinel embed"), "{err}");
        assert!(err.contains("chunks.lance"), "and where it looked: {err}");
    }

    /// A store from before this layout holds a `vectors` table and no `chunks`. Its reader
    /// is told the two ways forward, and so is `embed`, which would otherwise embed the
    /// corpus again beside it.
    #[tokio::test]
    async fn an_old_layout_is_refused_with_the_way_forward() {
        let dir = tempfile::tempdir().unwrap();
        let db = connect(dir.path()).await.unwrap();
        let schema = Arc::new(Schema::new(vec![Field::new(
            HASH_COLUMN,
            DataType::Utf8,
            false,
        )]));
        db.create_empty_table(LEGACY_TABLE, schema)
            .execute()
            .await
            .unwrap();

        let err = VectorTable::open_existing(dir.path())
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("vectors.lance"), "{err}");
        assert!(err.contains("release before this one"), "{err}");
        assert!(err.contains("centinel embed"), "{err}");
        assert!(refuse_old_layout(dir.path()).await.is_err());

        let fresh = tempfile::tempdir().unwrap();
        assert!(
            refuse_old_layout(fresh.path()).await.is_ok(),
            "no table at all"
        );
    }

    /// Vectors from two models share no space. Reading one as the other returns a ranked
    /// list of nonsense, which is why this is refused at open.
    #[tokio::test]
    async fn a_different_model_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        table(dir.path()).await;
        let err = VectorTable::open(dir.path(), &space("other-model"), DIMS)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("not comparable"), "{err}");
        assert!(
            err.contains("centinel embed --adopt"),
            "names the fix: {err}"
        );

        // The same model behind another server is another space until measured.
        let elsewhere = Space {
            endpoint: Some("https://openrouter.ai/api/v1".into()),
            ..space("test-model")
        };
        assert!(
            VectorTable::open(dir.path(), &elsewhere, DIMS)
                .await
                .is_err()
        );
    }

    /// Adoption rewrites the record and nothing else: the rows stay, the next open in the
    /// new space succeeds, and the old space is refused from then on.
    #[tokio::test]
    async fn an_adopted_table_keeps_its_rows_and_answers_to_the_new_space() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = Space {
            model: "qwen3-embedding-4b".into(),
            endpoint: None,
        };
        let mut t = VectorTable::open(dir.path(), &legacy, DIMS).await.unwrap();
        t.append(
            &lock(dir.path()).await,
            &[row(1, 0, unit([1.0, 0.0, 0.0, 0.0]))],
        )
        .await
        .unwrap();

        t.adopt(&lock(dir.path()).await, &space("qwen3-embedding-4b"))
            .await
            .unwrap();
        assert_eq!(t.space(), &space("qwen3-embedding-4b"));
        t.append(
            &lock(dir.path()).await,
            &[row(2, 0, unit([0.0, 1.0, 0.0, 0.0]))],
        )
        .await
        .unwrap();

        let reopened = VectorTable::open_existing(dir.path()).await.unwrap();
        assert_eq!(reopened.space(), &space("qwen3-embedding-4b"));
        assert_eq!(reopened.len().await.unwrap(), 2);
        assert!(
            VectorTable::open(dir.path(), &space("qwen3-embedding-4b"), DIMS)
                .await
                .is_ok()
        );
        assert!(VectorTable::open(dir.path(), &legacy, DIMS).await.is_err());
    }

    #[tokio::test]
    async fn a_different_width_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        table(dir.path()).await;
        let err = VectorTable::open(dir.path(), &space("test-model"), DIMS + 1)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("dimensional"), "{err}");
    }

    #[tokio::test]
    async fn a_wrong_width_vector_is_refused_rather_than_written() {
        let dir = tempfile::tempdir().unwrap();
        let t = table(dir.path()).await;
        let err = t
            .append(&lock(dir.path()).await, &[row(1, 0, vec![1.0; DIMS + 3])])
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("dimensions"), "{err}");
        assert!(t.is_empty().await.unwrap(), "nothing was written");
    }

    #[tokio::test]
    async fn an_empty_append_is_a_no_op() {
        let dir = tempfile::tempdir().unwrap();
        let t = table(dir.path()).await;
        t.append(&lock(dir.path()).await, &[]).await.unwrap();
        assert!(t.is_empty().await.unwrap());
    }

    /// Overwrites the first fragment's data file with filler of the same length — the
    /// shape of the field failure: a file of the right size holding nothing Lance can
    /// read. Returns the fragment's id, the file, and the bytes it held.
    async fn damage_first_fragment(
        dir: &Path,
        t: &VectorTable,
    ) -> (u64, std::path::PathBuf, Vec<u8>) {
        let dataset = t.table.dataset().unwrap().get().await.unwrap();
        let first = &dataset.get_fragments()[0];
        let file = dir
            .join("chunks.lance")
            .join("data")
            .join(&first.metadata().files[0].path);
        let original = std::fs::read(&file).unwrap();
        std::fs::write(&file, vec![0xAA; original.len()]).unwrap();
        (first.metadata().id, file, original)
    }

    /// The repair is a commit this handle did not make through lancedb, so the handle has
    /// to be told. Without that, the `keys()` that follows on the same handle would go
    /// on reading the fragment that is gone — and `embed`, which holds one handle for the
    /// whole run, would fail its pre-flight on a table it had just repaired.
    #[tokio::test]
    async fn repair_is_seen_by_the_handle_that_made_it() {
        let dir = tempfile::tempdir().unwrap();
        let t = table(dir.path()).await;
        // Two appends, so two fragments: one to damage, one to prove untouched.
        t.append(
            &lock(dir.path()).await,
            &[row(1, 0, unit([1.0, 0.0, 0.0, 0.0]))],
        )
        .await
        .unwrap();
        t.append(
            &lock(dir.path()).await,
            &[row(2, 0, unit([0.0, 1.0, 0.0, 0.0]))],
        )
        .await
        .unwrap();
        let (bad, _, _) = damage_first_fragment(dir.path(), &t).await;

        // A fresh handle, as `verify` opens one: the damaged fragment was never read
        // through it, so nothing is cached that the disk no longer holds.
        let t = table(dir.path()).await;
        assert!(t.keys(|_| {}).await.is_err(), "the scan embed makes fails");

        let audit = t.audit(&Progress::none(), &Cancel::none()).await.unwrap();
        assert_eq!(audit.fragments, 2);
        assert_eq!(audit.readable(), 1, "the untouched fragment still reads");
        assert_eq!(audit.unreadable.len(), 1);
        assert_eq!(audit.unreadable[0].id, bad);
        assert_eq!(audit.unreadable[0].physical_rows, Some(1));

        let done = t
            .repair(&lock(dir.path()).await, &audit, &Cancel::none())
            .await
            .unwrap();
        assert_eq!(
            done,
            VectorRepair {
                fragments_dropped: 1,
                estimated_rows: 1,
            }
        );

        // The same handle, not a reopened one.
        assert_eq!(
            all_keys(&t).await,
            HashSet::from([key_of(&hash(2), &doc(0))])
        );
        // The dropped row comes back the way it would from `embed`, and maintenance
        // folds the table as if nothing had happened.
        t.append(
            &lock(dir.path()).await,
            &[row(1, 0, unit([1.0, 0.0, 0.0, 0.0]))],
        )
        .await
        .unwrap();
        t.maintain(&lock(dir.path()).await, false).await.unwrap();
        assert_eq!(all_keys(&t).await.len(), 2);
        assert_eq!(
            t.nearest(&unit([1.0, 0.0, 0.0, 0.0]), 1, &Filter::default())
                .await
                .unwrap()[0]
                .chunk_hash,
            hash(1)
        );
    }

    /// An audit names fragment ids, and ids are only meaningful at the version they were
    /// read. If a compaction rewrote the table in between, the same id could name a
    /// fragment the audit never looked at — so the repair must fail, not delete.
    #[tokio::test]
    async fn a_stale_audit_cannot_drop_fragments_it_never_saw() {
        let dir = tempfile::tempdir().unwrap();
        let t = table(dir.path()).await;
        t.append(
            &lock(dir.path()).await,
            &[row(1, 0, unit([1.0, 0.0, 0.0, 0.0]))],
        )
        .await
        .unwrap();
        t.append(
            &lock(dir.path()).await,
            &[row(2, 0, unit([0.0, 1.0, 0.0, 0.0]))],
        )
        .await
        .unwrap();
        let (_, file, original) = damage_first_fragment(dir.path(), &t).await;

        let t = table(dir.path()).await;
        let stale = t.audit(&Progress::none(), &Cancel::none()).await.unwrap();
        assert_eq!(stale.unreadable.len(), 1);

        // The bytes come back — a restore from backup, say — and before anyone acts on
        // the audit an `embed` compacts both fragments into one.
        std::fs::write(&file, original).unwrap();
        let done = table(dir.path())
            .await
            .maintain(&lock(dir.path()).await, false)
            .await
            .unwrap();
        assert_eq!(done.fragments_removed, 2, "{done:?}");

        let err = t
            .repair(&lock(dir.path()).await, &stale, &Cancel::none())
            .await
            .unwrap_err();
        assert!(
            format!("{err:#}").contains("Rewrite"),
            "Lance names the commit that got there first: {err:#}"
        );
        assert_eq!(
            all_keys(&table(dir.path()).await).await.len(),
            2,
            "nothing was dropped"
        );
    }

    /// The table name is what names the directory, so this is the layout assertion that
    /// belongs here rather than in `store`.
    #[tokio::test]
    async fn the_table_is_a_directory_named_for_itself() {
        let dir = tempfile::tempdir().unwrap();
        table(dir.path()).await;
        assert!(dir.path().join("chunks.lance").is_dir());
    }
}
