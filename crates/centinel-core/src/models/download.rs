//! Resumable, verified downloads.
//!
//! A model pull is 0.6–1.7 GB over a link that may be a hotel wifi. Three properties
//! follow, and each one is a decision rather than a nicety:
//!
//! 1. **Interruption is normal.** Bytes land in `<name>.part`; an interrupted transfer
//!    is resumed with `Range: bytes=<len>-` instead of restarted. This mirrors the
//!    argument in [`crate::ops::collect`]: at this scale, resumability is not a feature
//!    bolted on afterwards, it is what makes the operation runnable at all.
//! 2. **Nothing is trusted.** Every file is re-hashed against the digest pinned in
//!    [`super::REGISTRY`] before it is renamed into place, so a truncated or corrupted
//!    transfer can never be mistaken for a working model.
//! 3. **A `.part` is only ever discarded deliberately.** A network error leaves it
//!    exactly where it was — that is the resume point. Only a *digest mismatch* deletes
//!    it, because resuming from bytes known to be wrong would loop forever.
//!
//! ## Why the final hash reads from disk
//!
//! It would be cheaper to hash the stream as it arrives, but that cannot work across a
//! resume: the prefix was written by an earlier process. Re-reading the completed
//! `.part` is uniform for both cases and verifies **what actually landed on disk**,
//! which is strictly stronger than verifying what came off the socket.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use super::ModelFile;
use crate::op::{Progress, TOTAL_TRACK, Unit};

/// Emit a progress event at least this often, so a slow link still looks alive.
const EMIT_INTERVAL: Duration = Duration::from_millis(100);

/// …and at most this often, so a fast link does not flood the channel. 613 MB at this
/// granularity is about 1,200 events rather than 75,000.
const EMIT_BYTES: u64 = 512 * 1024;

/// Where interrupted bytes accumulate. Sibling of the target, so the rename is atomic.
pub fn part_path(dest: &Path) -> PathBuf {
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    dest.with_file_name(name)
}

/// One file to fetch.
pub struct FileJob {
    pub url: String,
    pub dest: PathBuf,
    pub file: &'static ModelFile,
    /// Groups progress events into one bar. Namespaced by model, because two models
    /// both have a `tokenizer.json`.
    pub bar_id: String,
    /// What a human should see on that bar.
    pub label: String,
}

/// What happened to one file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Already on disk at the pinned size. Nothing was fetched.
    Present,
    /// Fetched. `resumed_from` is 0 for a fresh download.
    Downloaded { bytes: u64, resumed_from: u64 },
}

/// Aggregate progress across every file in a pull.
///
/// Tracked separately from the per-file bar so the operator sees both "this file" and
/// "this pull" — the difference between a 4-second tokenizer and the 1.2 GB behind it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Overall {
    pub done: u64,
    pub total: u64,
}

impl Overall {
    pub fn new(total: u64) -> Self {
        Self { done: 0, total }
    }

    fn emit(&self, progress: &Progress) {
        progress.track(TOTAL_TRACK, "total", self.done, self.total, Unit::Bytes);
    }
}

pub struct Downloader {
    client: reqwest::Client,
    /// Re-fetch even when the file is already present at the right size.
    force: bool,
}

impl Downloader {
    pub fn new(user_agent: &str, force: bool) -> anyhow::Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .user_agent(user_agent)
                // Deliberately **no** total-request timeout. `Fetcher` sets one because a
                // web page that takes 30s is broken; a 1.2 GB download that takes 20
                // minutes is not. What we want is an inactivity timeout, which is what
                // `read_timeout` is — it fires when no bytes arrive, not when many do.
                .connect_timeout(Duration::from_secs(30))
                .read_timeout(Duration::from_secs(60))
                .build()?,
            force,
        })
    }

    /// Fetches one file, resuming if a `.part` is present.
    pub async fn fetch(
        &self,
        job: &FileJob,
        progress: &Progress,
        overall: &mut Overall,
    ) -> anyhow::Result<Outcome> {
        let expected = job.file.size;

        if !self.force
            && let Ok(meta) = tokio::fs::metadata(&job.dest).await
            && meta.len() == expected
        {
            overall.done += expected;
            overall.emit(progress);
            progress.say(format!("{} — already present", job.label));
            return Ok(Outcome::Present);
        }

        if let Some(parent) = job.dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let part = part_path(&job.dest);
        if self.force {
            // A forced pull starts clean; resuming into bytes we were told to distrust
            // would defeat the flag.
            let _ = tokio::fs::remove_file(&part).await;
        }

        let mut resume_from = match tokio::fs::metadata(&part).await {
            Ok(m) => m.len(),
            Err(_) => 0,
        };
        // A `.part` at or past the full size cannot be resumed — the server would answer
        // 416. Discard it and start over rather than hand the user a range error.
        if resume_from >= expected {
            let _ = tokio::fs::remove_file(&part).await;
            resume_from = 0;
        }

        let mut request = self.client.get(&job.url);
        if resume_from > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={resume_from}-"));
            progress.say(format!(
                "{} — resuming at {resume_from} of {expected} bytes",
                job.label
            ));
        }

        let response = request
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("GET {}: {e}", job.url))?;
        let status = response.status();

        // 206 honours the range; 200 means the server ignored it and is sending the whole
        // file, in which case the `.part` must be thrown away or the result is corrupt.
        let append = match status.as_u16() {
            206 => true,
            200 => {
                if resume_from > 0 {
                    tracing::debug!(url = %job.url, "server ignored Range; restarting");
                    resume_from = 0;
                }
                false
            }
            416 => anyhow::bail!(
                "{}: server rejected the resume range. Delete {} and pull again.",
                job.label,
                part.display()
            ),
            _ => anyhow::bail!("{}: HTTP {status} from {}", job.label, job.url),
        };

        // Guard against the pin having gone stale. Better to fail here, naming the
        // cause, than to download a gigabyte and fail the digest check at the end.
        let claimed = response.content_length().map(|len| len + resume_from);
        if let Some(total) = claimed
            && total != expected
        {
            anyhow::bail!(
                "{}: upstream reports {total} bytes, the registry pins {expected}. \
                 The pinned revision may have been rewritten.",
                job.label
            );
        }

        let mut sink = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append)
            .open(&part)
            .await
            .map_err(|e| anyhow::anyhow!("opening {}: {e}", part.display()))?;

        // The floor for this file's bar and for the aggregate, so a resumed download
        // starts where it left off instead of snapping back to zero.
        let overall_base = overall.done;
        let mut written = resume_from;
        progress.track(&job.bar_id, &job.label, written, expected, Unit::Bytes);
        overall.done = overall_base + written;
        overall.emit(progress);

        let mut stream = response.bytes_stream();
        let mut since_emit = 0u64;
        let mut last_emit = Instant::now();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| {
                anyhow::anyhow!("{}: transfer failed after {written} bytes: {e}", job.label)
            })?;
            sink.write_all(&chunk).await?;

            written += chunk.len() as u64;
            since_emit += chunk.len() as u64;
            overall.done = overall_base + written;

            if since_emit >= EMIT_BYTES || last_emit.elapsed() >= EMIT_INTERVAL {
                progress.track(&job.bar_id, &job.label, written, expected, Unit::Bytes);
                overall.emit(progress);
                since_emit = 0;
                last_emit = Instant::now();
            }
        }
        // Flushed and closed before hashing, so the digest sees every byte.
        sink.flush().await?;
        drop(sink);

        if written != expected {
            // The `.part` survives: a short read is precisely the case resuming exists
            // for, and the next run picks up here.
            anyhow::bail!(
                "{}: transfer ended at {written} of {expected} bytes. Run the pull again to resume.",
                job.label
            );
        }

        progress.say(format!("{} — verifying", job.label));
        let actual = sha256_file(&part).await?;
        if actual != job.file.sha256 {
            // The only case that discards a `.part`. These bytes are known-bad, so
            // resuming from them would fail identically forever.
            let _ = tokio::fs::remove_file(&part).await;
            anyhow::bail!(
                "{}: digest mismatch.\n  expected {}\n  actual   {actual}\n\
                 The partial download has been discarded; pull again.",
                job.label,
                job.file.sha256
            );
        }

        tokio::fs::rename(&part, &job.dest)
            .await
            .map_err(|e| anyhow::anyhow!("installing {}: {e}", job.dest.display()))?;

        progress.track(&job.bar_id, &job.label, expected, expected, Unit::Bytes);
        overall.done = overall_base + expected;
        overall.emit(progress);

        Ok(Outcome::Downloaded {
            bytes: written - resume_from,
            resumed_from: resume_from,
        })
    }
}

/// SHA-256 of a file, streamed so a 1.2 GB model does not become 1.2 GB of RAM.
pub async fn sha256_file(path: &Path) -> anyhow::Result<String> {
    use tokio::io::AsyncReadExt;

    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];

    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_files_sit_beside_their_target() {
        let dest = Path::new("/models/repo/rev/onnx/model_int8.onnx");
        let part = part_path(dest);
        assert_eq!(
            part.parent(),
            dest.parent(),
            "rename must stay on one filesystem"
        );
        assert_eq!(part.file_name().unwrap(), "model_int8.onnx.part");
    }

    /// External-data files keep a `.onnx_data` suffix; the `.part` must not eat it.
    #[test]
    fn part_naming_preserves_unusual_extensions() {
        let part = part_path(Path::new("/m/onnx/model_fp16.onnx_data"));
        assert_eq!(part.file_name().unwrap(), "model_fp16.onnx_data.part");
    }

    #[tokio::test]
    async fn hashing_matches_a_known_digest() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f");
        tokio::fs::write(&path, b"centinel").await.unwrap();
        assert_eq!(
            sha256_file(&path).await.unwrap(),
            "7f9e7349108363deafcb3ba6b4d4ef994b0f1b30899cbbf3330acadf42fc1735"
        );
    }

    #[tokio::test]
    async fn streamed_hashing_matches_whole_file_hashing_across_the_buffer_boundary() {
        // The read loop is chunked; a file larger than one buffer is the case that
        // would expose a mistake in it.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big");
        let bytes: Vec<u8> = (0..(3 << 20) + 7).map(|i| (i % 251) as u8).collect();
        tokio::fs::write(&path, &bytes).await.unwrap();

        assert_eq!(
            sha256_file(&path).await.unwrap(),
            hex::encode(Sha256::digest(&bytes))
        );
    }

    #[tokio::test]
    async fn hashing_an_empty_file_is_the_empty_digest() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty");
        tokio::fs::write(&path, b"").await.unwrap();
        assert_eq!(
            sha256_file(&path).await.unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
