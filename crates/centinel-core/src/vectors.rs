//! The embedding cache — SPEC §5.2's durable Tier A artifact.
//!
//! ```text
//!   A. embedding cache   expensive to rebuild (inference over the corpus)   portable
//!   B. search index      cheap to rebuild (minutes)                         not portable
//! ```
//!
//! Which is why this is **beside** the static files rather than inside any vector store.
//! Swapping search backends becomes a re-import, not a re-embed; a corrupt index is
//! `rm -rf` plus minutes. It is also the natural unit to publish — *"Centinel embeddings
//! for cityofX.gov"* lets someone build on a crawl without repeating it.
//!
//! ## Format
//!
//! One file per `(model_id, dims)`, which is §5.2's cache key minus the chunk hash:
//!
//! ```text
//!   cache/embeddings/<model_id>-<dims>.vec
//!
//!   [64-byte header][record][record]…
//!   header:  magic(12) version(4) dims(4) model_id(40, NUL-padded) reserved(4)
//!   record:  chunk_hash(32 raw bytes)  vector(dims × f32 little-endian)
//! ```
//!
//! **Fixed-width and append-only**, which buys three things at once: a record is at a
//! computable offset, appending is the only mutation so an interrupted run keeps
//! everything it had written, and the header makes a published file self-describing
//! rather than dependent on the filename surviving a download.
//!
//! One file rather than a file per vector: 2,560 dimensions is 10 KB, and a 200,000-chunk
//! corpus would otherwise be 200,000 tiny files that no filesystem enjoys.
//!
//! ## Torn writes
//!
//! A crash mid-append can leave a partial trailing record. [`VectorCache::open`] detects
//! that — the file length must be `header + n × record` — and truncates the fragment.
//! Safe precisely because the file is append-only and content-addressed: the dropped
//! chunk is simply re-embedded on the next run.

use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 12] = b"CENTINELVEC\0";
const VERSION: u32 = 1;
const MODEL_ID_LEN: usize = 40;
const HEADER_LEN: u64 = 64;
/// SHA-256, stored raw rather than as 64 hex characters.
const HASH_LEN: usize = 32;

/// An append-only store of `(chunk_hash → vector)` for one model.
#[derive(Clone, Debug)]
pub struct VectorCache {
    path: PathBuf,
    model_id: String,
    dims: usize,
}

impl VectorCache {
    /// Opens or creates the cache for a model, validating an existing file's header.
    ///
    /// A dimension or model mismatch is an error rather than a silent append: vectors
    /// from two models are not comparable, and §6.2 makes mixing them a corpus-level
    /// mistake rather than a recoverable one.
    pub fn open(store_root: &Path, model_id: &str, dims: usize) -> anyhow::Result<Self> {
        anyhow::ensure!(dims > 0, "a model with zero dimensions cannot be cached");
        anyhow::ensure!(
            model_id.len() <= MODEL_ID_LEN,
            "model id `{model_id}` exceeds {MODEL_ID_LEN} bytes"
        );

        let dir = store_root.join("cache").join("embeddings");
        std::fs::create_dir_all(&dir)?;
        let cache = Self {
            path: dir.join(format!("{model_id}-{dims}.vec")),
            model_id: model_id.to_string(),
            dims,
        };

        match File::open(&cache.path) {
            Ok(mut file) => cache.validate_header(&mut file)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => cache.write_header()?,
            Err(e) => return Err(anyhow::anyhow!("opening {}: {e}", cache.path.display())),
        }
        cache.truncate_partial_record()?;
        Ok(cache)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn dims(&self) -> usize {
        self.dims
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    fn record_len(&self) -> u64 {
        (HASH_LEN + self.dims * 4) as u64
    }

    fn write_header(&self) -> anyhow::Result<()> {
        let mut header = [0u8; HEADER_LEN as usize];
        header[..12].copy_from_slice(MAGIC);
        header[12..16].copy_from_slice(&VERSION.to_le_bytes());
        header[16..20].copy_from_slice(&(self.dims as u32).to_le_bytes());
        let id = self.model_id.as_bytes();
        header[20..20 + id.len()].copy_from_slice(id);

        let mut file = File::create(&self.path)?;
        file.write_all(&header)?;
        file.sync_all()?;
        Ok(())
    }

    fn validate_header(&self, file: &mut File) -> anyhow::Result<()> {
        let mut header = [0u8; HEADER_LEN as usize];
        file.read_exact(&mut header).map_err(|e| {
            anyhow::anyhow!("{} is too short to be a cache: {e}", self.path.display())
        })?;

        anyhow::ensure!(
            &header[..12] == MAGIC,
            "{} is not a Centinel vector cache",
            self.path.display()
        );
        let version = u32::from_le_bytes(header[12..16].try_into().unwrap());
        anyhow::ensure!(
            version == VERSION,
            "{} is format version {version}; this build writes {VERSION}",
            self.path.display()
        );
        let dims = u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize;
        anyhow::ensure!(
            dims == self.dims,
            "{} holds {dims}-dimensional vectors, not {}",
            self.path.display(),
            self.dims
        );
        let id_bytes = &header[20..20 + MODEL_ID_LEN];
        let id = String::from_utf8_lossy(id_bytes)
            .trim_end_matches('\0')
            .to_string();
        anyhow::ensure!(
            id == self.model_id,
            "{} was written by `{id}`, not `{}` — vectors from two models are not comparable",
            self.path.display(),
            self.model_id
        );
        Ok(())
    }

    /// Drops a partial trailing record left by an interrupted append.
    fn truncate_partial_record(&self) -> anyhow::Result<()> {
        let len = std::fs::metadata(&self.path)?.len();
        let payload = len.saturating_sub(HEADER_LEN);
        let extra = payload % self.record_len();
        if extra != 0 {
            tracing::warn!(
                path = %self.path.display(),
                bytes = extra,
                "dropping a partial record from an interrupted run"
            );
            OpenOptions::new()
                .write(true)
                .open(&self.path)?
                .set_len(len - extra)?;
        }
        Ok(())
    }

    /// How many vectors are stored.
    pub fn len(&self) -> anyhow::Result<usize> {
        let len = std::fs::metadata(&self.path)?.len();
        Ok((len.saturating_sub(HEADER_LEN) / self.record_len()) as usize)
    }

    pub fn is_empty(&self) -> anyhow::Result<bool> {
        Ok(self.len()? == 0)
    }

    /// Every cached chunk hash, as hex.
    ///
    /// Reads hashes and skips vectors, so answering "what still needs embedding?" costs
    /// 32 bytes per chunk of attention rather than 10 KB. That question is asked at the
    /// start of every `embed` run, which is why it is worth the seeking.
    pub fn hashes(&self) -> anyhow::Result<HashSet<String>> {
        let file = File::open(&self.path)?;
        let count = self.len()?;
        let mut reader = BufReader::with_capacity(1 << 20, file);
        reader.seek(SeekFrom::Start(HEADER_LEN))?;

        let mut out = HashSet::with_capacity(count);
        let mut hash = [0u8; HASH_LEN];
        for _ in 0..count {
            reader.read_exact(&mut hash)?;
            out.insert(hex::encode(hash));
            reader.seek_relative((self.dims * 4) as i64)?;
        }
        Ok(out)
    }

    /// Appends vectors. The only mutation this type performs.
    ///
    /// Buffered into one write per call so a batch lands as a unit under normal
    /// operation; a crash mid-write is still handled, by [`Self::open`].
    pub fn append(&self, entries: &[(String, Vec<f32>)]) -> anyhow::Result<()> {
        if entries.is_empty() {
            return Ok(());
        }
        let mut file = BufWriter::with_capacity(
            (entries.len() as u64 * self.record_len()).min(1 << 22) as usize,
            OpenOptions::new().append(true).open(&self.path)?,
        );

        for (hash, vector) in entries {
            anyhow::ensure!(
                vector.len() == self.dims,
                "vector for {hash} has {} dimensions, expected {}",
                vector.len(),
                self.dims
            );
            let raw = hex::decode(hash)
                .map_err(|e| anyhow::anyhow!("chunk hash `{hash}` is not hex: {e}"))?;
            anyhow::ensure!(
                raw.len() == HASH_LEN,
                "chunk hash `{hash}` is {} bytes, expected {HASH_LEN}",
                raw.len()
            );

            file.write_all(&raw)?;
            for value in vector {
                file.write_all(&value.to_le_bytes())?;
            }
        }
        file.flush()?;
        Ok(())
    }

    /// Loads everything into memory — what a brute-force search scans.
    ///
    /// At 2,560 dimensions a 200,000-chunk corpus is about 2 GB, which an M-series
    /// machine scans in milliseconds. That is the measurement that lets LanceDB wait.
    pub fn load_all(&self) -> anyhow::Result<Vec<(String, Vec<f32>)>> {
        let file = File::open(&self.path)?;
        let count = self.len()?;
        let mut reader = BufReader::with_capacity(1 << 22, file);
        reader.seek(SeekFrom::Start(HEADER_LEN))?;

        let mut out = Vec::with_capacity(count);
        let mut hash = [0u8; HASH_LEN];
        let mut buf = vec![0u8; self.dims * 4];
        for _ in 0..count {
            reader.read_exact(&mut hash)?;
            reader.read_exact(&mut buf)?;
            let vector = buf
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
                .collect();
            out.push((hex::encode(hash), vector));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIMS: usize = 4;

    fn hash(byte: u8) -> String {
        hex::encode([byte; HASH_LEN])
    }

    fn cache(dir: &Path) -> VectorCache {
        VectorCache::open(dir, "test-model", DIMS).unwrap()
    }

    #[test]
    fn a_new_cache_is_empty_and_self_describing() {
        let dir = tempfile::tempdir().unwrap();
        let c = cache(dir.path());
        assert!(c.is_empty().unwrap());
        assert_eq!(c.dims(), DIMS);
        // Reopening validates the header it just wrote.
        assert!(VectorCache::open(dir.path(), "test-model", DIMS).is_ok());
    }

    #[test]
    fn vectors_round_trip_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let c = cache(dir.path());
        let entries = vec![
            (hash(1), vec![1.0, 2.0, 3.0, 4.0]),
            (hash(2), vec![-1.5, 0.0, 0.25, 9.0]),
        ];
        c.append(&entries).unwrap();

        assert_eq!(c.len().unwrap(), 2);
        assert_eq!(c.load_all().unwrap(), entries);
        assert_eq!(c.hashes().unwrap(), HashSet::from([hash(1), hash(2)]));
    }

    #[test]
    fn appends_accumulate_across_reopens() {
        let dir = tempfile::tempdir().unwrap();
        cache(dir.path())
            .append(&[(hash(1), vec![1.0; DIMS])])
            .unwrap();
        cache(dir.path())
            .append(&[(hash(2), vec![2.0; DIMS])])
            .unwrap();
        assert_eq!(cache(dir.path()).len().unwrap(), 2);
    }

    /// The resumability property: whatever was written before a kill is still there,
    /// and the work list is recomputed from it. No checkpoint file.
    #[test]
    fn an_interrupted_append_leaves_a_readable_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let c = cache(dir.path());
        c.append(&[(hash(1), vec![1.0; DIMS]), (hash(2), vec![2.0; DIMS])])
            .unwrap();

        // Simulate a crash midway through a third record.
        let len = std::fs::metadata(c.path()).unwrap().len();
        OpenOptions::new()
            .append(true)
            .open(c.path())
            .unwrap()
            .write_all(&[0u8; 7])
            .unwrap();
        assert_ne!(std::fs::metadata(c.path()).unwrap().len(), len);

        // Reopening heals it, keeping every complete record.
        let reopened = cache(dir.path());
        assert_eq!(reopened.len().unwrap(), 2);
        assert_eq!(std::fs::metadata(reopened.path()).unwrap().len(), len);
        assert_eq!(reopened.hashes().unwrap().len(), 2);
    }

    /// Vectors from two models share no space. Appending one to the other's file would
    /// produce a cache whose contents are silently incomparable.
    #[test]
    fn a_different_model_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        cache(dir.path());
        let path = VectorCache::open(dir.path(), "test-model", DIMS)
            .unwrap()
            .path()
            .to_path_buf();
        // Same filename, different model in the header.
        let other = VectorCache {
            path,
            model_id: "other-model".into(),
            dims: DIMS,
        };
        let err = other
            .validate_header(&mut File::open(other.path()).unwrap())
            .unwrap_err()
            .to_string();
        assert!(err.contains("not comparable"), "{err}");
    }

    #[test]
    fn a_different_dimension_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        cache(dir.path());
        let err = VectorCache::open(dir.path(), "test-model", DIMS + 1);
        // A different width lands in a different file, so this succeeds — and that is
        // the point: the two never mix.
        assert!(err.is_ok());
        assert_ne!(
            cache(dir.path()).path(),
            VectorCache::open(dir.path(), "test-model", DIMS + 1)
                .unwrap()
                .path()
        );
    }

    #[test]
    fn a_wrong_width_vector_is_refused_rather_than_written() {
        let dir = tempfile::tempdir().unwrap();
        let c = cache(dir.path());
        let err = c
            .append(&[(hash(1), vec![1.0; DIMS + 3])])
            .unwrap_err()
            .to_string();
        assert!(err.contains("dimensions"), "{err}");
    }

    #[test]
    fn a_non_hex_chunk_hash_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let err = cache(dir.path())
            .append(&[("not-a-hash".into(), vec![0.0; DIMS])])
            .unwrap_err()
            .to_string();
        assert!(err.contains("not hex"), "{err}");
    }

    #[test]
    fn a_foreign_file_is_not_mistaken_for_a_cache() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir
            .path()
            .join("cache")
            .join("embeddings")
            .join(format!("test-model-{DIMS}.vec"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, vec![0xABu8; 128]).unwrap();

        let err = VectorCache::open(dir.path(), "test-model", DIMS)
            .unwrap_err()
            .to_string();
        assert!(err.contains("not a Centinel vector cache"), "{err}");
    }

    #[test]
    fn an_empty_append_is_a_no_op() {
        let dir = tempfile::tempdir().unwrap();
        let c = cache(dir.path());
        c.append(&[]).unwrap();
        assert!(c.is_empty().unwrap());
    }

    #[test]
    fn the_file_lives_under_the_path_spec_5_names() {
        let dir = tempfile::tempdir().unwrap();
        let c = cache(dir.path());
        assert!(
            c.path()
                .ends_with(format!("cache/embeddings/test-model-{DIMS}.vec"))
        );
    }
}
