//! The vector store — `chunk_hash → vector`, in LanceDB.
//!
//! ```text
//!   <root>/centinel.db        SQLite: metadata + FTS5   — the BM25 arm
//!   <root>/vectors.lance/     LanceDB: vectors          — this
//! ```
//!
//! Vectors are written here by `embed` and read here by `search`. There is no second
//! copy.
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
//! What the cache cost was a second write path and a pipeline stage with its own skip
//! predicate, on a corpus where a wrong skip predicate is the defect that has bitten this
//! codebase most often. What it bought was one property — append-only bytes the query
//! engine never rewrites — which is not worth ~4 GiB and a stage.
//!
//! ## The guard
//!
//! A query vector and the vectors it searches must come from the same model (SPEC §6.2).
//! Two models produce plausible, silently incomparable results, so the model id lives in
//! the table's schema metadata and a mismatch is refused at [`VectorTable::open`] rather
//! than discovered in bad rankings.
//!
//! Width is guarded by the schema itself: the column is a `FixedSizeList` of exactly
//! `dims` floats, so a wrong width cannot be written at all.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use futures::TryStreamExt;
use lancedb::arrow::arrow_array::{
    Array, FixedSizeListArray, Float32Array, RecordBatch, RecordBatchIterator, RecordBatchReader,
    StringArray, types::Float32Type,
};
use lancedb::arrow::arrow_schema::{DataType, Field, Schema, SchemaRef};
use lancedb::query::{ExecutableQuery, QueryBase, Select};
use lancedb::{DistanceType, Table};

/// The table inside the database, which is what names `vectors.lance` on disk.
pub const TABLE: &str = "vectors";

const HASH_COLUMN: &str = "chunk_hash";
const VECTOR_COLUMN: &str = "vector";
/// Where the model id is recorded. Namespaced, because the schema metadata is a shared
/// map and Lance writes its own keys into it.
const MODEL_KEY: &str = "centinel.model_id";
/// Lance's name for the column a vector query scores into.
const DISTANCE_COLUMN: &str = "_distance";

/// A LanceDB table of `(chunk_hash, vector)` for one model.
///
/// Cheap to clone — a `Table` is a handle.
#[derive(Clone)]
pub struct VectorTable {
    table: Table,
    schema: SchemaRef,
    model_id: String,
    dims: usize,
}

/// Names the model and the width, not the Lance handle — which is a connection pool and
/// has nothing a reader of a test failure wants.
impl std::fmt::Debug for VectorTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VectorTable")
            .field("model_id", &self.model_id)
            .field("dims", &self.dims)
            .finish_non_exhaustive()
    }
}

impl VectorTable {
    /// Opens or creates the table for a model, validating an existing one.
    ///
    /// Takes the database directory rather than the store root: where the table lives
    /// under a store is [`crate::store::Store::vectors_path`]'s business, and this module
    /// knowing that layout is how a path ends up spelled out in two places.
    pub async fn open(dir: &Path, model_id: &str, dims: usize) -> anyhow::Result<Self> {
        anyhow::ensure!(dims > 0, "a model with zero dimensions cannot be stored");

        let dir = dir
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("{} is not valid UTF-8", dir.display()))?;
        let db = lancedb::connect(dir).execute().await?;
        let schema = schema_for(model_id, dims);

        let table = if db.table_names().execute().await?.iter().any(|n| n == TABLE) {
            let table = db.open_table(TABLE).execute().await?;
            validate(table.schema().await?.as_ref(), model_id, dims)?;
            table
        } else {
            db.create_empty_table(TABLE, schema.clone())
                .execute()
                .await?
        };

        Ok(Self {
            table,
            schema,
            model_id: model_id.to_string(),
            dims,
        })
    }

    /// Opens an existing table, taking the model and the width **from it**.
    ///
    /// For readers. `search` has no business being told which embedder to use: the answer
    /// is a property of the table, and a reader that was configured differently would
    /// have its query refused by [`Self::open`]'s guard and quietly fall back to one arm.
    /// Asking the table removes the question.
    pub async fn open_existing(dir: &Path) -> anyhow::Result<Self> {
        let dir_str = dir
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("{} is not valid UTF-8", dir.display()))?;
        let db = lancedb::connect(dir_str).execute().await?;
        anyhow::ensure!(
            db.table_names().execute().await?.iter().any(|n| n == TABLE),
            "no vectors at {} — run `centinel embed` first",
            dir.join(format!("{TABLE}.lance")).display()
        );

        let table = db.open_table(TABLE).execute().await?;
        let schema = table.schema().await?;
        let dims = match schema
            .field_with_name(VECTOR_COLUMN)
            .map_err(|_| anyhow::anyhow!("`{TABLE}` has no `{VECTOR_COLUMN}` column"))?
            .data_type()
        {
            DataType::FixedSizeList(_, n) => *n as usize,
            other => anyhow::bail!("`{VECTOR_COLUMN}` is {other}, not a fixed-size list of floats"),
        };
        let model_id = schema
            .metadata()
            .get(MODEL_KEY)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "`{TABLE}` does not record which model wrote it — \
                     delete it and re-run `centinel embed`"
                )
            })?
            .clone();

        Ok(Self {
            table,
            schema,
            model_id,
            dims,
        })
    }

    pub fn dims(&self) -> usize {
        self.dims
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// How many vectors are stored.
    pub async fn len(&self) -> anyhow::Result<usize> {
        Ok(self.table.count_rows(None).await?)
    }

    pub async fn is_empty(&self) -> anyhow::Result<bool> {
        Ok(self.len().await? == 0)
    }

    /// Every stored chunk hash.
    ///
    /// One column, so the scan reads hashes and never touches the vectors beside them.
    /// That question is asked at the start of every `embed` run, and on a full corpus the
    /// difference is tens of megabytes against four gigabytes.
    pub async fn hashes(&self) -> anyhow::Result<HashSet<String>> {
        // No `limit`, which on a plain scan means every row. Lance only applies its
        // default top-k to vector and full-text queries.
        let mut stream = self
            .table
            .query()
            .select(Select::Columns(vec![HASH_COLUMN.to_string()]))
            .execute()
            .await?;

        let mut out = HashSet::new();
        while let Some(batch) = stream.try_next().await? {
            for hash in hash_column(&batch)? {
                out.insert(hash);
            }
        }
        Ok(out)
    }

    /// Appends vectors. The only mutation this type performs.
    ///
    /// Lance commits a version per call, so an interrupted run keeps every batch that
    /// landed before the kill and the work list is recomputed from what is there. That is
    /// what makes `embed` resumable without a checkpoint file.
    pub async fn append(&self, entries: &[(String, Vec<f32>)]) -> anyhow::Result<()> {
        if entries.is_empty() {
            return Ok(());
        }
        for (hash, vector) in entries {
            anyhow::ensure!(
                vector.len() == self.dims,
                "vector for {hash} has {} dimensions, expected {}",
                vector.len(),
                self.dims
            );
        }

        let hashes = StringArray::from_iter_values(entries.iter().map(|(h, _)| h.as_str()));
        let vectors = FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
            entries
                .iter()
                .map(|(_, v)| Some(v.iter().copied().map(Some).collect::<Vec<_>>())),
            self.dims as i32,
        );
        let batch = RecordBatch::try_new(
            self.schema.clone(),
            vec![Arc::new(hashes), Arc::new(vectors)],
        )?;

        let reader = Box::new(RecordBatchIterator::new(
            vec![Ok(batch)],
            self.schema.clone(),
        )) as Box<dyn RecordBatchReader + Send>;
        self.table.add(reader).execute().await?;
        Ok(())
    }

    /// The `limit` nearest chunks to `query`, best first.
    ///
    /// Returns cosine **similarity**, not Lance's distance — higher is better, matching
    /// the BM25 arm, so a caller fusing the two never has to remember which way one of
    /// them points.
    pub async fn nearest(&self, query: &[f32], limit: usize) -> anyhow::Result<Vec<(String, f32)>> {
        anyhow::ensure!(
            query.len() == self.dims,
            "query vector has {} dimensions, this table holds {}",
            query.len(),
            self.dims
        );
        if limit == 0 || self.is_empty().await? {
            return Ok(Vec::new());
        }

        let mut stream = self
            .table
            .query()
            .nearest_to(query.to_vec())?
            // Explicit, though the vectors are L2-normalized and so rank identically
            // under L2. Relying on that would make the ranking depend on a property of
            // the embedder that this module cannot see.
            .distance_type(DistanceType::Cosine)
            .limit(limit)
            .execute()
            .await?;

        let mut out = Vec::with_capacity(limit);
        while let Some(batch) = stream.try_next().await? {
            let hashes = hash_column(&batch)?;
            let distances = batch
                .column_by_name(DISTANCE_COLUMN)
                .and_then(|c| c.as_any().downcast_ref::<Float32Array>())
                .ok_or_else(|| anyhow::anyhow!("no `{DISTANCE_COLUMN}` in a vector result"))?;
            for (i, hash) in hashes.into_iter().enumerate() {
                out.push((hash, 1.0 - distances.value(i)));
            }
        }
        Ok(out)
    }
}

/// The schema, and the one place the model id is written into it.
fn schema_for(model_id: &str, dims: usize) -> SchemaRef {
    let metadata = HashMap::from([(MODEL_KEY.to_string(), model_id.to_string())]);
    Arc::new(Schema::new_with_metadata(
        vec![
            Field::new(HASH_COLUMN, DataType::Utf8, false),
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

/// Refuses a table that was written by another model, or at another width.
///
/// Both are silent failures otherwise: vectors from two models are in different spaces
/// and still return a ranked list.
fn validate(found: &Schema, model_id: &str, dims: usize) -> anyhow::Result<()> {
    let field = found
        .field_with_name(VECTOR_COLUMN)
        .map_err(|_| anyhow::anyhow!("`{TABLE}` has no `{VECTOR_COLUMN}` column"))?;
    let width = match field.data_type() {
        DataType::FixedSizeList(_, n) => *n as usize,
        other => anyhow::bail!("`{VECTOR_COLUMN}` is {other}, not a fixed-size list of floats"),
    };
    anyhow::ensure!(
        width == dims,
        "`{TABLE}` holds {width}-dimensional vectors, not {dims} — \
         delete it and re-run `centinel embed`"
    );

    let found_id = found.metadata().get(MODEL_KEY).map(String::as_str);
    anyhow::ensure!(
        found_id == Some(model_id),
        "`{TABLE}` was written by `{}`, not `{model_id}` — vectors from two models are \
         not comparable; delete it and re-run `centinel embed`",
        found_id.unwrap_or("an unrecorded model"),
    );
    Ok(())
}

fn hash_column(batch: &RecordBatch) -> anyhow::Result<Vec<String>> {
    let column = batch
        .column_by_name(HASH_COLUMN)
        .and_then(|c| c.as_any().downcast_ref::<StringArray>())
        .ok_or_else(|| anyhow::anyhow!("no `{HASH_COLUMN}` in a result batch"))?;
    Ok((0..column.len())
        .map(|i| column.value(i).to_string())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIMS: usize = 4;

    fn hash(byte: u8) -> String {
        hex::encode([byte; 32])
    }

    /// L2-normalized, so `nearest` returns a similarity in [-1, 1].
    fn unit(v: [f32; DIMS]) -> Vec<f32> {
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        v.iter().map(|x| x / norm).collect()
    }

    async fn table(dir: &Path) -> VectorTable {
        VectorTable::open(dir, "test-model", DIMS).await.unwrap()
    }

    #[tokio::test]
    async fn a_new_table_is_empty_and_self_describing() {
        let dir = tempfile::tempdir().unwrap();
        let t = table(dir.path()).await;
        assert!(t.is_empty().await.unwrap());
        assert_eq!(t.dims(), DIMS);
        assert_eq!(t.model_id(), "test-model");
        // Reopening validates the schema it just wrote.
        assert!(
            VectorTable::open(dir.path(), "test-model", DIMS)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn vectors_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let t = table(dir.path()).await;
        t.append(&[
            (hash(1), unit([1.0, 0.0, 0.0, 0.0])),
            (hash(2), unit([0.0, 1.0, 0.0, 0.0])),
        ])
        .await
        .unwrap();

        assert_eq!(t.len().await.unwrap(), 2);
        assert_eq!(t.hashes().await.unwrap(), HashSet::from([hash(1), hash(2)]));
    }

    #[tokio::test]
    async fn appends_accumulate_across_reopens() {
        let dir = tempfile::tempdir().unwrap();
        table(dir.path())
            .await
            .append(&[(hash(1), unit([1.0, 1.0, 1.0, 1.0]))])
            .await
            .unwrap();
        table(dir.path())
            .await
            .append(&[(hash(2), unit([1.0, 0.0, 1.0, 0.0]))])
            .await
            .unwrap();
        assert_eq!(table(dir.path()).await.len().await.unwrap(), 2);
    }

    /// The one thing this table exists to answer.
    #[tokio::test]
    async fn nearest_ranks_by_similarity_with_higher_being_better() {
        let dir = tempfile::tempdir().unwrap();
        let t = table(dir.path()).await;
        t.append(&[
            (hash(1), unit([1.0, 0.0, 0.0, 0.0])),
            (hash(2), unit([0.9, 0.1, 0.0, 0.0])),
            (hash(3), unit([0.0, 0.0, 0.0, 1.0])),
        ])
        .await
        .unwrap();

        let hits = t.nearest(&unit([1.0, 0.0, 0.0, 0.0]), 3).await.unwrap();
        assert_eq!(hits.len(), 3);
        assert_eq!(hits[0].0, hash(1), "the identical vector ranks first");
        assert_eq!(hits[1].0, hash(2));
        assert_eq!(hits[2].0, hash(3), "the orthogonal one ranks last");
        assert!(
            hits[0].1 > hits[1].1 && hits[1].1 > hits[2].1,
            "scores descend: {hits:?}"
        );
        assert!(
            (hits[0].1 - 1.0).abs() < 1e-5,
            "a self-match is 1.0: {hits:?}"
        );
    }

    #[tokio::test]
    async fn nearest_honours_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let t = table(dir.path()).await;
        for i in 1..=5u8 {
            t.append(&[(hash(i), unit([i as f32, 1.0, 0.0, 0.0]))])
                .await
                .unwrap();
        }
        assert_eq!(
            t.nearest(&unit([1.0, 1.0, 0.0, 0.0]), 2)
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
            t.nearest(&unit([1.0, 0.0, 0.0, 0.0]), 10)
                .await
                .unwrap()
                .is_empty()
        );
    }

    /// `search` opens the table without being told which model wrote it, so that a
    /// reader configured differently cannot silently fall back to one arm.
    #[tokio::test]
    async fn open_existing_takes_the_model_and_width_from_the_table() {
        let dir = tempfile::tempdir().unwrap();
        table(dir.path())
            .await
            .append(&[(hash(1), unit([1.0, 0.0, 0.0, 0.0]))])
            .await
            .unwrap();

        let found = VectorTable::open_existing(dir.path()).await.unwrap();
        assert_eq!(found.model_id(), "test-model");
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
        assert!(err.contains("vectors.lance"), "and where it looked: {err}");
    }

    /// Vectors from two models share no space. Reading one as the other returns a ranked
    /// list of nonsense, which is why this is refused at open.
    #[tokio::test]
    async fn a_different_model_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        table(dir.path()).await;
        let err = VectorTable::open(dir.path(), "other-model", DIMS)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("not comparable"), "{err}");
        assert!(err.contains("centinel embed"), "names the fix: {err}");
    }

    #[tokio::test]
    async fn a_different_width_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        table(dir.path()).await;
        let err = VectorTable::open(dir.path(), "test-model", DIMS + 1)
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
            .append(&[(hash(1), vec![1.0; DIMS + 3])])
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
        t.append(&[]).await.unwrap();
        assert!(t.is_empty().await.unwrap());
    }

    /// The table name is what names the directory, so this is the layout assertion that
    /// belongs here rather than in `store`.
    #[tokio::test]
    async fn the_table_is_a_directory_named_for_itself() {
        let dir = tempfile::tempdir().unwrap();
        table(dir.path()).await;
        assert!(dir.path().join("vectors.lance").is_dir());
    }
}
