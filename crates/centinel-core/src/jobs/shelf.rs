//! The board on disk: one file per job, holding the job as it stands.
//!
//! `<store>/jobs/<job>.json` is a [`Shelved`]: which process and board wrote it, and the
//! job's [`JobState`] — the fold of every event and the bounded tail of the readable ones.
//! The writer replaces the whole file ([`write_atomically`]), so a reader sees the last
//! version or the one before and never half of one; a reader takes the file whole, so a
//! look that fails changes nothing and the next one reads the same thing again. A file is
//! as big as one job's state, whatever the job has done.
//!
//! Beside it, `<job>.stop` is a request from another process that the job stop, which
//! only the process running it can act on. Dotted names are writes in flight.
//!
//! The job id is the file's name, escaped to a safe one; nothing parses it back, because
//! the id is in the file.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use super::{FINISHED, JobState};
use crate::journal::pid_is_alive;
use crate::store::{in_flight_writer, write_atomically};

const STATE: &str = "json";
const STOP: &str = "stop";

/// How long a finished job's file outlives its place among the newest [`FINISHED`].
///
/// Every board looks twice a second, so any reader that is running has read a job's
/// ending long before this; the margin is for one that has stalled. A file removed anyway
/// — by hand, or under a reader stalled past it — still ends the job on that reader's
/// board, as one whose record was removed.
const GRACE: Duration = Duration::from_secs(600);

/// One job's file.
#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Shelved {
    /// The process running the job, for asking whether it is still alive.
    pub(super) pid: u32,
    /// The board that runs it. A board skips its own files by this, not by pid, so two
    /// boards in one process see each other's jobs like any other process's.
    pub(super) board: String,
    pub(super) state: JobState,
}

/// What a reader knows of one file, from the last time it read it.
#[derive(Debug)]
pub(super) struct Seen {
    /// Inode, length and modification time: the file has changed when this has. The
    /// inode is what catches two writes inside one tick of a coarse clock, because every
    /// rename puts a new one in place.
    key: (u64, u64, SystemTime),
    id: String,
    pid: u32,
    /// When the job finished, in the writer's milliseconds; `None` while it runs.
    finished_at: Option<i64>,
}

/// The store's `jobs/`, as one process reads it.
#[derive(Debug)]
pub(super) struct Shelf {
    dir: PathBuf,
}

/// One look at the directory.
pub(super) struct Look {
    /// Files that changed since the last look, read whole.
    pub(super) changed: Vec<Shelved>,
    /// Unfinished jobs whose writer is gone and whose file has not changed since that
    /// was known — what the file says is the last thing it will say — with when the file
    /// was last written, which is when the job last moved. A writer that finished and
    /// exited during the look changed its file, so it is read again rather than mistaken
    /// for one that died.
    pub(super) orphaned: Vec<(String, i64)>,
    /// Every job with a file.
    pub(super) present: BTreeSet<String>,
    /// The writers already gone before the listing. Anything decided about a writer's
    /// files is decided from this rather than from asking again later: a process alive at
    /// the listing may have written its ending since, and only one gone before it cannot.
    pub(super) gone: BTreeSet<u32>,
    /// Every job file the listing found.
    listed: BTreeSet<PathBuf>,
    /// Job files that changed and could not be read: what is known of them is older than
    /// what they hold, so nothing is decided about them until a look reads them.
    unsettled: BTreeSet<PathBuf>,
}

impl Shelf {
    pub(super) fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn path(&self, id: &str, extension: &str) -> PathBuf {
        self.dir.join(format!("{}.{extension}", file_safe(id)))
    }

    /// Writes one of this board's jobs as it stands; false when the store refused it. The
    /// op the job watches goes on either way.
    pub(super) fn write(&self, shelved: &Shelved) -> bool {
        let path = self.path(&shelved.state.id, STATE);
        let bytes = serde_json::to_vec(shelved).expect("a job always serializes");
        match write_atomically(&path, bytes) {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(%error, "could not record a job on disk; other processes will not see it move until it can");
                false
            }
        }
    }

    /// Asks the process running a job to stop it. True when the request is down.
    pub(super) fn ask_to_stop(&self, id: &str) -> bool {
        let path = self.path(id, STOP);
        match std::fs::File::create(&path) {
            Ok(_) => true,
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "could not ask another process to stop a job");
                false
            }
        }
    }

    /// Whether another process has asked for this job to stop, taking the request.
    pub(super) fn take_stop_request(&self, id: &str) -> bool {
        std::fs::remove_file(self.path(id, STOP)).is_ok()
    }

    /// Reads every file that changed since the last look. `None` when the directory could
    /// not be listed: a look that cannot see the shelf decides nothing about it, and
    /// `seen` is kept for the next one.
    pub(super) fn look(&self, seen: &mut BTreeMap<PathBuf, Seen>) -> Option<Look> {
        // Asked before the listing, so a writer counted gone cannot still be writing.
        let gone: BTreeSet<u32> = seen
            .values()
            .map(|s| s.pid)
            .filter(|pid| !pid_is_alive(*pid))
            .collect();
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            // No job has been written to this store yet.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                seen.clear();
                return Some(Look {
                    changed: Vec::new(),
                    orphaned: Vec::new(),
                    present: BTreeSet::new(),
                    gone,
                    listed: BTreeSet::new(),
                    unsettled: BTreeSet::new(),
                });
            }
            Err(error) => {
                tracing::debug!(%error, "could not list the jobs on disk this time");
                return None;
            }
        };
        let mut listed = BTreeMap::new();
        for entry in entries {
            let Ok(entry) = entry else { return None };
            let path = entry.path();
            if path.extension().is_some_and(|e| e == STATE) && !is_in_flight(&path) {
                let Ok(meta) = entry.metadata() else {
                    return None;
                };
                listed.insert(path, key(&meta));
            }
        }
        seen.retain(|path, _| listed.contains_key(path));

        let (mut changed, mut orphaned, mut unsettled) = (Vec::new(), Vec::new(), BTreeSet::new());
        let paths = listed.keys().cloned().collect();
        for (path, key) in listed {
            if let Some(s) = seen.get(&path).filter(|s| s.key == key) {
                if gone.contains(&s.pid) && s.finished_at.is_none() {
                    orphaned.push((s.id.clone(), millis(key.2)));
                }
                continue;
            }
            // A file replaced between the listing and this read is read as it now is; its
            // key is taken again on the next look and it is read once more, which is
            // harmless.
            let Some(shelved) = std::fs::read(&path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Shelved>(&bytes).ok())
            else {
                unsettled.insert(path);
                continue;
            };
            // Read for the first time, unfinished, from a writer that is gone now: if the
            // file is still the one the listing found, nothing wrote it after the read, so
            // its writer died with it unfinished — a board's first look need not wait for
            // a second to say so.
            if shelved.state.finished_at.is_none()
                && !pid_is_alive(shelved.pid)
                && std::fs::metadata(&path).is_ok_and(|meta| self::key(&meta) == key)
            {
                orphaned.push((shelved.state.id.clone(), millis(key.2)));
            }
            seen.insert(
                path,
                Seen {
                    key,
                    id: shelved.state.id.clone(),
                    pid: shelved.pid,
                    finished_at: shelved.state.finished_at,
                },
            );
            changed.push(shelved);
        }
        let present = seen.values().map(|s| s.id.clone()).collect();
        Some(Look {
            changed,
            orphaned,
            present,
            gone,
            listed: paths,
            unsettled,
        })
    }

    /// Removes what no reader needs any more, by one rule every process applies the same
    /// way: a job still running is kept; of the rest — finished, or left by a writer
    /// `gone` before the look — the newest [`FINISHED`] are kept, and the others once they
    /// are older than [`GRACE`]. A stop request goes once its job has ended, or once it is
    /// older than [`GRACE`] with no job beside it; a write in flight goes once its writer
    /// is gone.
    ///
    /// Every judgement is made from what `look` read and who was gone before it. A writer
    /// alive at the listing may have replaced its file since, a file that changed and could
    /// not be read may hold an ending nobody has seen, and a request may have been made for
    /// a job no look has read yet; none of them is touched.
    pub(super) fn prune(&self, seen: &BTreeMap<PathBuf, Seen>, look: &Look) {
        let ended_at = |s: &Seen| match s.finished_at {
            Some(at) => Some(at),
            None if look.gone.contains(&s.pid) => Some(millis(s.key.2)),
            None => None,
        };
        let mut ended: Vec<(i64, &str, &Path)> = seen
            .iter()
            .filter(|(path, _)| !look.unsettled.contains(*path))
            .filter_map(|(path, s)| Some((ended_at(s)?, s.id.as_str(), path.as_path())))
            .collect();
        // Newest first, then by id: every process must agree on the cut, and the order a
        // board heard of two endings is that board's alone.
        ended.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
        let cutoff = millis(SystemTime::now()) - GRACE.as_millis() as i64;
        for (at, _, path) in ended.iter().skip(FINISHED) {
            if *at < cutoff {
                let _ = std::fs::remove_file(path);
            }
        }

        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            let stale = if path.extension().is_some_and(|e| e == STOP) {
                let job = path.with_extension(STATE);
                match seen.get(&job) {
                    Some(seen) if !look.unsettled.contains(&job) => ended_at(seen).is_some(),
                    _ if look.listed.contains(&job) => false,
                    _ => entry
                        .metadata()
                        .and_then(|m| m.modified())
                        .is_ok_and(|at| millis(at) < cutoff),
                }
            } else {
                in_flight_writer(&path).is_some_and(|pid| !pid_is_alive(pid))
            };
            if stale {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
}

fn key(meta: &std::fs::Metadata) -> (u64, u64, SystemTime) {
    #[cfg(unix)]
    let ino = std::os::unix::fs::MetadataExt::ino(meta);
    #[cfg(not(unix))]
    let ino = 0;
    (
        ino,
        meta.len(),
        meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
    )
}

fn millis(at: SystemTime) -> i64 {
    at.duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// A dotted name is a [`write_atomically`] temp file.
fn is_in_flight(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with('.'))
}

/// A job id as a file name: letters, digits, `-` and `_` as they are, every other byte as
/// `%XX`. A run's id is an instant, and `:` is not a file name everywhere.
fn file_safe(id: &str) -> String {
    let mut out = String::with_capacity(id.len());
    for byte in id.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' => out.push(byte as char),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}
