//! Carries the workspace's tags and exclusions onto the vector rows that hold them.
//!
//! A row in `chunks.lance` says what the workspace said about its document when the row
//! was written. [`materialize`](super::classifications) queues every document whose tags
//! or exclusion change in `vector_sync`, with a revision that grows on each change. This
//! drains the queue: under the one lock every writer of the table takes, it reads a batch
//! of stale documents with their current state, rewrites the two metadata columns on
//! their rows, and acknowledges each document at the revision it read. A document
//! materialized again meanwhile holds a newer revision, the acknowledgement misses it,
//! and the next drain writes it again — so a row never ends up older than the workspace
//! without a pending revision saying so.
//!
//! `embed` reads a document's state and appends under the same lock, which is what makes
//! that bookkeeping sound: a state read before a change is always followed by a drain that
//! sees the change.

use std::sync::Arc;

use crate::index::Index;
use crate::store::Store;
use crate::vectors::{VectorTable, key_of};

use super::classifications::Classifications;

/// Documents read from the queue per pass. Each costs two small queries and its chunk
/// list; a pass of this many is a second or two, and the lock is held for one pass.
const DOCUMENTS_PER_PASS: usize = 500;

/// Rows per metadata rewrite. Lance patches the two columns inside the fragments that
/// hold the rows; a batch this size keeps each commit to a handful of them.
const ROWS_PER_WRITE: usize = 10_000;

/// After a drain that rewrote more rows than this, the table is compacted and its scalar
/// indexes brought up to date, so a corpus-wide reclassification does not leave a query
/// reading a column rewrite per fragment.
const MAINTAIN_AFTER_ROWS: usize = 50_000;

/// What one drain did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VectorSync {
    pub documents: usize,
    pub rows: usize,
}

/// Holds `workspace/vectors.lock` for as long as the guard lives: the one writer of the
/// vector table at a time. Taken on a blocking thread, because another writer may hold it
/// for the length of an append and the async runtime must not wait with it.
pub(crate) struct VectorLock {
    _file: Arc<std::fs::File>,
}

impl VectorLock {
    pub(crate) async fn take(store: &Store) -> anyhow::Result<Self> {
        std::fs::create_dir_all(store.workspace_dir())?;
        let file = Arc::new(
            std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(store.vectors_lock_path())?,
        );
        let held = Arc::clone(&file);
        tokio::task::spawn_blocking(move || held.lock()).await??;
        Ok(Self { _file: file })
    }
}

/// Rewrites the tags and exclusion on the vector rows of every document the workspace has
/// queued, and empties the queue of what it wrote. With no vector table yet there is
/// nothing to rewrite: the queue is cleared, because every row `embed` later writes
/// carries the state of that moment.
pub async fn sync_vectors(store: &Store) -> anyhow::Result<VectorSync> {
    let classifications = Classifications::open(store)?;
    if classifications.stale_vector_count()? == 0 {
        return Ok(VectorSync::default());
    }
    let mut done = VectorSync::default();
    if !store.vectors_path().exists() {
        loop {
            let stale = classifications.stale_vectors(DOCUMENTS_PER_PASS)?;
            if stale.is_empty() {
                return Ok(done);
            }
            done.documents += stale.len();
            classifications.vectors_written(&stale)?;
        }
    }

    let _lock = VectorLock::take(store).await?;
    let table = VectorTable::open_existing(&store.vectors_db()).await?;
    let index = Index::open(store.require_index()?)?;
    loop {
        let stale = classifications.stale_vectors(DOCUMENTS_PER_PASS)?;
        if stale.is_empty() {
            break;
        }
        let mut rows = Vec::new();
        for document in &stale {
            for chunk_hash in index.chunk_hashes_of(&document.id)? {
                rows.push((key_of(&chunk_hash, &document.id), document.state.clone()));
            }
        }
        for batch in rows.chunks(ROWS_PER_WRITE) {
            table.set_metadata(batch).await?;
        }
        classifications.vectors_written(&stale)?;
        done.documents += stale.len();
        done.rows += rows.len();
    }
    if done.rows > MAINTAIN_AFTER_ROWS {
        table.maintain(false).await?;
    }
    tracing::debug!(
        documents = done.documents,
        rows = done.rows,
        "vector metadata synced"
    );
    Ok(done)
}
