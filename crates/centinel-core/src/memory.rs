//! The memory budget: how much this process may hold at once, and the one place that
//! decides it.
//!
//! Readers scale with cores and their cost scales with the document, so a fixed number of
//! them dies on a small machine and idles on a large one (docs/FIELD-NOTES.md, Fault 9).
//! Four 19 MB hurricane guides read at once took a 3.3 GB VM past 2.2 GB, and the same four
//! pass unnoticed on a 16 GB laptop. A knob per stage would push that arithmetic onto the
//! operator and still die on a wrong guess. So there is one figure, the [`Budget`]. Every
//! stage that holds memory **reserves** against it before a unit of work and waits while it
//! is full. A small machine runs slower instead of being killed. A large one never waits,
//! because its budget is more than its workers can ask for.
//!
//! ## Where the figure comes from
//!
//! Nearest answer first: `--memory` for one run, `[resources] memory` in the config, else
//! [`SHARE`] of what this machine has, the smaller of its memory and its cgroup's limit
//! ([`Budget::auto`]). It is read once, when the process starts ([`install`]), and every op
//! the process runs shares it: `serve`'s schedules and the web's actions run on one machine.
//! Two processes on one machine each have their own.
//!
//! ## What reserves
//!
//! - **extract**, per document: [`read_cost`] of its size, from before the blob is read
//!   until its readers are done.
//! - **collect**, per body: its declared length, or the size ceiling (at most the default
//!   one) while the server has not declared one, until the body is read. See
//!   [`crate::fetch`].
//!
//! Embed and classify wait on a server, not on memory, and keep their `concurrency`. The
//! Lance caches are sized from the budget ([`Budget::metadata_cache`],
//! [`Budget::index_cache`]) and taken out of it when it is made, because they are held for
//! the life of a connection and not for a unit of work. What is left is the **room** that
//! reservations share ([`Budget::room`]).
//!
//! ## One reservation at a time
//!
//! A unit of work holds one reservation and gives it back before it asks for another.
//! Holding one while waiting for a second is how two hosts, each holding half the budget,
//! would wait for each other forever. A request larger than the whole room is cut to the
//! whole room: it waits for everything else to finish, then runs alone.
//!
//! ## An estimate, not a bound
//!
//! A reservation is what a unit of work is expected to hold, and nothing measures what it
//! does hold. The estimate for a document is deliberately high because its size is a poor
//! predictor of its cost. Where one is wrong anyway, the contained readers still offer
//! themselves to the OOM killer first (`extract::contained`), so a document the estimate
//! missed costs one read and not the run.

#[cfg(any(target_os = "linux", test))]
use std::path::Path;
use std::sync::OnceLock;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::sync::{Semaphore, SemaphorePermit};

use crate::config::ResourcesConfig;
use crate::render;

/// The share of the machine a budget takes when nothing names one: three quarters.
///
/// Generous, so a large machine is never held below what it did before there was a budget.
/// Sixteen readers of the heaviest documents measured, 52 MB scans, reserve ~16 GB of
/// [`read_cost`]. A 32 GB machine's budget is 24 GiB, 17.6 GiB of it room once the Lance
/// caches are taken out, so its sixteen cores all read at once; the smallest machine that
/// holds sixteen of them is about 28 GB. A 16 GB laptop has 8.8 GiB of room: nine of those
/// scans at once, twelve 19 MB vector-art guides, and its cores' worth of anything under a
/// few MB.
///
/// The quarter left over is for what the budget does not count: this process at rest
/// (100–270 MB under jemalloc), the kernel, and whatever else the machine runs. On the field VM that quarter is ~850 MB, and `serve` beside a run held ~580 MB of
/// it. A machine shared like that is better named by hand, in `[resources] memory`.
pub const SHARE: (u64, u64) = (3, 4);

/// The budget when this machine's memory cannot be read, which happens on neither Linux
/// nor macOS. Four GiB, the size of a small machine, so a guess errs towards waiting.
const ASSUMED: u64 = 4 << 30;

/// What one read of a document is reserved at, before anything is known about it but its
/// size: [`READ_BASE`], plus [`READ_PER_BYTE`] for each byte up to [`READ_KNEE`], plus
/// [`READ_PER_BYTE_PAST_KNEE`] for each byte past it.
///
/// Peak RSS of one read, measured on the field corpus (docs/FIELD-NOTES.md, Fault 9, and
/// the running log at 06:33–06:36Z):
///
/// | document | size | peak | × size | reserved |
/// |---|---|---|---|---|
/// | a one-page form | 0.2 MB | 49 MB | — | 75 MB |
/// | a utility guide | 6.9 MB | 64 MB | 9× | 343 MB |
/// | damage prevention guide | 15.8 MB | 671 MB | 44× | 699 MB |
/// | hurricane guide | 19.3 MB | 561 MB | 29× | 758 MB |
/// | ACFR, 17,162 font subsets | 32 MB | 669 MB | 21× | 860 MB |
/// | THEDC report, scans | 52 MB | 867 MB | 17× | 1.02 GB |
///
/// Size does not predict cost: 8–10× for most PDFs, 30–44× for the ones carrying vector
/// artwork, and nothing about the bytes says which a document is. So up to the knee the
/// estimate takes the worst ratio seen. It over-reserves most documents in order to never
/// under-reserve one of those, and that has the effect wanted: on a small machine a
/// document of a few MB takes a large share and reads nearly alone, while small ones read as
/// wide as there are cores.
///
/// Past the knee the worst ratio falls, from 44× at 15.8 MB to 17× at 52 MB: what a parse
/// holds grows with the artwork in a page, not with the scans and fonts that make a document
/// large. A flat 40× would reserve 2.15 GB for the 52 MB scan that took 867 MB, and hold a
/// 32 GB machine to twelve of them where it has sixteen cores. The second slope still covers
/// every document measured.
///
/// One formula for every kind. Only PDFs were measured, and a kind that was not gets the
/// worst one that was.
pub fn read_cost(bytes: u64) -> u64 {
    let below = bytes.min(READ_KNEE);
    let past = bytes.saturating_sub(READ_KNEE);
    READ_BASE
        .saturating_add(below * READ_PER_BYTE)
        .saturating_add(past.saturating_mul(READ_PER_BYTE_PAST_KNEE))
}

/// A reader's child at rest, with its parser loaded: 49 MB measured on a one-page form.
const READ_BASE: u64 = 64 << 20;

/// The worst peak measured per byte of document, 44× on a 15.8 MB vector-art PDF, less the
/// part [`READ_BASE`] already covers. Charged on a document's first [`READ_KNEE`] bytes.
const READ_PER_BYTE: u64 = 40;

/// Where the worst ratio measured starts to fall: just past the 15.8 MB guide that set
/// [`READ_PER_BYTE`].
const READ_KNEE: u64 = 16 << 20;

/// What each byte past [`READ_KNEE`] adds. Eight, the least that keeps the reservation over
/// every document measured past the knee: 19.3 MB at 561 MB, 32 MB at 669 MB, 52 MB at
/// 867 MB.
const READ_PER_BYTE_PAST_KNEE: u64 = 8;

/// The metadata cache's floor, which is what it was before it scaled: Lance caches every
/// committed manifest there, and 64 MiB holds every version a run makes with fragments
/// kept to tens (see `vectors`).
const METADATA_CACHE_FLOOR: u64 = 64 << 20;
/// Lance's own default for the metadata cache, which a large machine gets back.
const METADATA_CACHE_CEILING: u64 = 1 << 30;
/// The index cache's floor: room for a few partitions of an IVF index.
const INDEX_CACHE_FLOOR: u64 = 64 << 20;
/// Lance's own default for the index cache, which a large machine gets back.
const INDEX_CACHE_CEILING: u64 = 6 << 30;

/// What a permit stands for. Tokio counts a reservation in `u32` permits, and a KiB each
/// reaches 4 TiB.
const UNIT: u64 = 1024;

/// Where a budget's figure came from, so `doctor` can say which answer was taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// `--memory`, for this run.
    Flag,
    /// `[resources] memory` in the config.
    Config,
    /// [`SHARE`] of the machine's memory.
    Machine,
    /// [`SHARE`] of this process's cgroup limit, which is below the machine's memory.
    Cgroup,
    /// Nothing could be read about the machine.
    Assumed,
}

impl std::fmt::Display for Origin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Flag => "--memory",
            Self::Config => "[resources] memory",
            Self::Machine => "three quarters of the machine",
            Self::Cgroup => "three quarters of the cgroup's limit",
            Self::Assumed => "assumed; this machine's memory could not be read",
        })
    }
}

/// How much this process may hold at once, and the room left in it.
#[derive(Debug)]
pub struct Budget {
    bytes: u64,
    origin: Origin,
    /// One permit per [`UNIT`] of the room: the budget less the Lance caches.
    room: Semaphore,
    units: u32,
}

impl Budget {
    /// A budget of `bytes`, with the Lance caches it sizes taken out of the room
    /// reservations share.
    ///
    /// Taken out because they are sized from this figure and held beside the work, not
    /// inside it: a budget that handed reads the whole of itself and caches a quarter more
    /// would let the process reach a quarter over what it was given. A cache does not fill
    /// until a search or a write uses it, so this is room set aside rather than room in use.
    ///
    /// A budget the caches' floors already fill leaves no room, and then every reservation
    /// is larger than the room and runs alone: on a machine that small, one at a time is the
    /// right answer, and the floors are what Lance needs to work at all.
    pub fn new(bytes: u64, origin: Origin) -> Self {
        let caches = (cache_size(bytes, METADATA) + cache_size(bytes, INDEX)) as u64;
        let units = units(bytes.saturating_sub(caches)).max(1);
        Self {
            bytes,
            origin,
            room: Semaphore::new(units as usize),
            units,
        }
    }

    /// The budget this process runs under: `--memory`, else `[resources] memory`, else
    /// [`Budget::auto`] of what the machine says.
    pub fn resolve(flag: Option<u64>, config: &ResourcesConfig) -> anyhow::Result<Self> {
        if let Some(bytes) = flag {
            return Ok(Self::new(bytes, Origin::Flag));
        }
        if let Some(bytes) = config.memory()? {
            return Ok(Self::new(bytes, Origin::Config));
        }
        Ok(Self::auto(&Machine::read()))
    }

    /// [`SHARE`] of the smaller of the machine's memory and its cgroup's limit.
    ///
    /// `MemTotal`, not `MemAvailable`. Available memory is a reading of one moment: a run
    /// started while a build held memory would be held to that for hours after the build
    /// ended, and a `serve` started at boot would size every scheduled run by a machine
    /// with nothing else on it. Total is what the kernel's own arithmetic is done against,
    /// and the quarter left is the room for what else the machine runs. On a ballooned VM
    /// total moves too, which `doctor` warns about: the budget is read once, at start.
    pub fn auto(machine: &Machine) -> Self {
        let (limit, origin) = match (machine.total, machine.cgroup) {
            (Some(total), Some(cgroup)) if cgroup < total => (cgroup, Origin::Cgroup),
            (Some(total), _) => (total, Origin::Machine),
            (None, Some(cgroup)) => (cgroup, Origin::Cgroup),
            (None, None) => return Self::new(ASSUMED, Origin::Assumed),
        };
        Self::new(limit / SHARE.1 * SHARE.0, origin)
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    pub fn origin(&self) -> Origin {
        self.origin
    }

    /// What reservations share: the budget less the Lance caches, at least one [`UNIT`].
    pub fn room(&self) -> u64 {
        u64::from(self.units) * UNIT
    }

    /// Takes `bytes` of room, waiting until there is that much. More than the whole
    /// [`Budget::room`] is taken as all of it, so it waits for everything else and then runs
    /// alone.
    ///
    /// Waiters are served in the order they asked: a large request at the head of the
    /// queue holds back the small ones behind it, which is what makes it run alone rather
    /// than wait for a gap that small requests keep filling.
    pub async fn reserve(&self, bytes: u64) -> Reservation<'_> {
        // Capped at the room, so a request larger than all of it waits for everything else
        // and then runs alone, rather than waiting for room that can never exist.
        let asked = units(bytes).min(self.units);
        let permit = match self.room.try_acquire_many(asked) {
            Ok(permit) => permit,
            Err(_) => {
                tracing::debug!(
                    asked = %render::bytes(bytes),
                    room = %render::bytes(self.room()),
                    "waiting for room in the memory budget"
                );
                self.room
                    .acquire_many(asked)
                    .await
                    .expect("the budget's semaphore is never closed")
            }
        };
        Reservation { permit }
    }

    /// The Lance session's metadata cache: a sixty-fourth of the budget, from the 64 MiB it
    /// was before it scaled up to Lance's default of 1 GiB.
    ///
    /// Lance puts every committed manifest here, and a manifest lists every fragment, so a
    /// small cache costs re-reads of small files from local disk and a large one costs
    /// memory a small machine does not have (docs/FIELD-NOTES.md, Fault 12). The field VM
    /// is at the floor; a 64 GB machine has Lance's default.
    pub fn metadata_cache(&self) -> usize {
        cache_size(self.bytes, METADATA)
    }

    /// The Lance session's index cache: a quarter of the budget, up to Lance's default of
    /// 6 GiB.
    ///
    /// The index cache is where a search keeps the IVF partitions it has read, so it fills
    /// in `serve` and holds what it filled. Lance's 6 GiB default is more than the field VM
    /// has in all; this gives it ~640 MB there. A machine with a 24 GiB budget, 32 GB at
    /// the default share, keeps the full default.
    pub fn index_cache(&self) -> usize {
        cache_size(self.bytes, INDEX)
    }
}

/// A Lance cache: its share of the budget (a divisor), its floor and its ceiling.
struct Cache {
    divisor: u64,
    floor: u64,
    ceiling: u64,
}

const METADATA: Cache = Cache {
    divisor: 64,
    floor: METADATA_CACHE_FLOOR,
    ceiling: METADATA_CACHE_CEILING,
};

const INDEX: Cache = Cache {
    divisor: 4,
    floor: INDEX_CACHE_FLOOR,
    ceiling: INDEX_CACHE_CEILING,
};

/// What a budget of `bytes` gives `cache`, which [`Budget::new`] takes out of the room as
/// well as the session being sized by it.
fn cache_size(bytes: u64, cache: Cache) -> usize {
    (bytes / cache.divisor).clamp(cache.floor, cache.ceiling) as usize
}

/// Permits for `bytes`, rounded up, at most what a `u32` counts.
fn units(bytes: u64) -> u32 {
    bytes.div_ceil(UNIT).min(u64::from(u32::MAX)) as u32
}

/// Room taken from a [`Budget`], given back when it is dropped.
#[derive(Debug)]
pub struct Reservation<'a> {
    permit: SemaphorePermit<'a>,
}

impl Reservation<'_> {
    /// Gives back all but `bytes`, once the work knows it needs less than it asked for.
    pub fn shrink_to(&mut self, bytes: u64) {
        let keep = units(bytes) as usize;
        let held = self.permit.num_permits();
        if keep < held {
            drop(self.permit.split(held - keep));
        }
    }
}

static BUDGET: OnceLock<Budget> = OnceLock::new();

/// The budget this process runs under.
///
/// The one [`install`] put in place, else [`Budget::auto`] of this machine, which is what a
/// program that never installs one (a test, a library caller) runs under.
pub fn budget() -> &'static Budget {
    BUDGET.get_or_init(|| Budget::auto(&Machine::read()))
}

/// Puts `budget` in place for the life of this process. Once, before anything reserves:
/// the binary does it before it opens the store.
pub fn install(budget: Budget) -> anyhow::Result<&'static Budget> {
    BUDGET.set(budget).map_err(|_| {
        anyhow::anyhow!("the memory budget is set once, when the process starts, and it was")
    })?;
    let budget = self::budget();
    tracing::debug!(
        budget = %render::bytes(budget.bytes),
        origin = %budget.origin,
        "memory budget"
    );
    Ok(budget)
}

/// Reads a size as a person writes it: `"1.5 GiB"`, `"1536 MiB"`, `"2G"`, or bytes.
///
/// `KB`, `MB`, `GB`, `TB` are powers of 1000 and `KiB`, `MiB`, `GiB`, `TiB` powers of 1024,
/// as they are written. A bare `K`, `M`, `G` or `T` is 1024's, as in systemd's
/// `MemoryMax=1400M`, which is where a number for this is often copied from.
pub fn parse_size(text: &str) -> Result<u64, String> {
    let text = text.trim();
    let split = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let number: f64 = number
        .parse()
        .map_err(|_| format!("`{text}` is not a size; write it as \"1.5 GiB\" or \"1536 MiB\""))?;
    let scale: u64 = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1,
        "kb" => 1_000,
        "mb" => 1_000_000,
        "gb" => 1_000_000_000,
        "tb" => 1_000_000_000_000,
        "k" | "kib" => 1 << 10,
        "m" | "mib" => 1 << 20,
        "g" | "gib" => 1 << 30,
        "t" | "tib" => 1 << 40,
        other => {
            return Err(format!(
                "`{other}` is not a unit; use B, KB, MB, GB, TB, or KiB, MiB, GiB, TiB"
            ));
        }
    };
    let bytes = number * scale as f64;
    if bytes < 1.0 {
        return Err(format!("`{text}` is no memory at all"));
    }
    Ok(bytes as u64)
}

/// What this machine says about its memory.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct Machine {
    /// `MemTotal` on Linux, `hw.memsize` on macOS.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    /// `MemAvailable`: what could be had now without swapping. Linux only, and not used
    /// for the budget; see [`Budget::auto`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available: Option<u64>,
    /// The tightest cgroup `memory.max` (or v1 `memory.limit_in_bytes`) on the way from
    /// this process's cgroup up to the root. Absent when none is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cgroup: Option<u64>,
    /// `virtio_balloon` is loaded: the host can take this VM's memory back while a run
    /// holds it, and `total` falls with it.
    #[serde(default)]
    pub ballooned: bool,
    /// What the VM's memory blocks add up to, read when it is ballooned: the size it was
    /// given, which `total` is below while the balloon holds some of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub given: Option<u64>,
}

impl Machine {
    pub fn read() -> Self {
        read_machine()
    }
}

#[cfg(target_os = "linux")]
fn read_machine() -> Machine {
    let meminfo = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let cgroup = std::fs::read_to_string("/proc/self/cgroup")
        .ok()
        .and_then(|own| {
            cgroup_limit(
                &own,
                Path::new("/sys/fs/cgroup"),
                Path::new("/sys/fs/cgroup/memory"),
            )
        });
    let ballooned = Path::new("/sys/module/virtio_balloon").exists();
    Machine {
        total: meminfo_field(&meminfo, "MemTotal"),
        available: meminfo_field(&meminfo, "MemAvailable"),
        cgroup,
        ballooned,
        given: ballooned
            .then(|| memory_blocks(Path::new("/sys/devices/system/memory")))
            .flatten(),
    }
}

#[cfg(target_os = "macos")]
fn read_machine() -> Machine {
    let mut size: u64 = 0;
    let mut len = std::mem::size_of::<u64>();
    // SAFETY: `hw.memsize` is a 64-bit integer, and `size` is one, with `len` saying so.
    let read = unsafe {
        libc::sysctlbyname(
            c"hw.memsize".as_ptr(),
            (&raw mut size).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    Machine {
        total: (read == 0 && size > 0).then_some(size),
        ..Machine::default()
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn read_machine() -> Machine {
    Machine::default()
}

/// One `/proc/meminfo` line, in bytes: `MemTotal:  3423456 kB`.
#[cfg(any(target_os = "linux", test))]
fn meminfo_field(meminfo: &str, name: &str) -> Option<u64> {
    let line = meminfo
        .lines()
        .find(|line| line.split(':').next() == Some(name))?;
    let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kib * 1024)
}

/// The tightest memory limit on this process's cgroups, v2 and v1, from `/proc/self/cgroup`
/// (`own`).
///
/// Every level up to the root, not only this process's own: `systemd-run --scope -p
/// MemoryMax=1400M` sets it on the scope, and a slice above can set a tighter one that the
/// scope's own `max` says nothing about. A level that reads `max`, or v1's
/// near-`i64::MAX` for unlimited, is no limit; the second is left for [`Budget::auto`] to
/// lose against the machine's memory.
#[cfg(any(target_os = "linux", test))]
fn cgroup_limit(own: &str, v2: &Path, v1: &Path) -> Option<u64> {
    own.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, ':');
            let (_, controllers, path) = (parts.next()?, parts.next()?, parts.next()?);
            match controllers {
                "" => tightest(v2, path, "memory.max"),
                c if c.split(',').any(|c| c == "memory") => {
                    tightest(v1, path, "memory.limit_in_bytes")
                }
                _ => None,
            }
        })
        .min()
}

#[cfg(any(target_os = "linux", test))]
fn tightest(root: &Path, path: &str, file: &str) -> Option<u64> {
    let mut dir = root.join(path.trim_start_matches('/'));
    let mut tightest: Option<u64> = None;
    loop {
        let limit = std::fs::read_to_string(dir.join(file))
            .ok()
            .and_then(|text| text.trim().parse::<u64>().ok());
        tightest = match (tightest, limit) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        if dir == root || !dir.pop() || !dir.starts_with(root) {
            return tightest;
        }
    }
}

/// What the online memory blocks under `dir` add up to.
#[cfg(target_os = "linux")]
fn memory_blocks(dir: &Path) -> Option<u64> {
    let block = std::fs::read_to_string(dir.join("block_size_bytes")).ok()?;
    let block = u64::from_str_radix(block.trim(), 16).ok()?;
    let online = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("memory"))
        .filter(|entry| {
            std::fs::read_to_string(entry.path().join("online")).is_ok_and(|s| s.trim() == "1")
        })
        .count() as u64;
    (online > 0).then_some(block * online)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const MIB: u64 = 1 << 20;

    /// Whether `reserving` is still waiting after a moment.
    async fn waits<F: std::future::Future>(reserving: F) -> bool {
        tokio::time::timeout(Duration::from_millis(50), reserving)
            .await
            .is_err()
    }

    /// A budget whose room is 704 MiB once its caches are out, in quarters.
    fn quartered() -> (Budget, u64) {
        let budget = Budget::new(1 << 30, Origin::Flag);
        let quarter = budget.room() / 4;
        (budget, quarter)
    }

    /// Room is taken and waited for, and dropping a reservation is what gives it back.
    #[tokio::test]
    async fn a_full_budget_waits_until_room_is_given_back() {
        let (budget, quarter) = quartered();
        let first = budget.reserve(3 * quarter).await;
        assert!(waits(budget.reserve(3 * quarter)).await, "3 + 3 is over 4");
        assert!(!waits(budget.reserve(quarter)).await, "3 + 1 fits");
        drop(first);
        assert!(
            !waits(budget.reserve(3 * quarter)).await,
            "given back on drop"
        );
    }

    /// A document heavier than the whole budget is not refused and does not wait forever:
    /// it waits for the rest, then runs alone.
    #[tokio::test]
    async fn a_reservation_over_the_whole_budget_runs_alone() {
        let (budget, quarter) = quartered();
        let small = budget.reserve(quarter).await;
        let mut heavy = Box::pin(budget.reserve(100 * budget.bytes()));
        assert!(
            waits(heavy.as_mut()).await,
            "it waits for what is in flight"
        );
        drop(small);
        let heavy = heavy.await;
        assert!(
            waits(budget.reserve(1)).await,
            "and while it runs nothing else does"
        );
        drop(heavy);
        assert!(!waits(budget.reserve(budget.room())).await);
    }

    /// A body reserved at the ceiling gives back what its declared length does not need.
    #[tokio::test]
    async fn shrinking_a_reservation_gives_back_the_rest() {
        let (budget, quarter) = quartered();
        let mut body = budget.reserve(3 * quarter).await;
        assert!(waits(budget.reserve(2 * quarter)).await);
        body.shrink_to(quarter);
        assert!(!waits(budget.reserve(2 * quarter)).await);
    }

    /// The caches are sized from the budget and held beside the work, so reads share what
    /// they leave. A budget the caches' floors fill still has room for one thing at a time.
    #[tokio::test]
    async fn the_lance_caches_come_out_of_the_room_and_a_tiny_budget_still_runs() {
        let budget = Budget::new(1 << 30, Origin::Flag);
        let caches = (budget.metadata_cache() + budget.index_cache()) as u64;
        assert_eq!(budget.room(), (1 << 30) - caches);

        let tiny = Budget::new(MIB, Origin::Flag);
        assert!(tiny.room() > 0);
        let only = tiny.reserve(read_cost(20 * MIB)).await;
        assert!(waits(tiny.reserve(1)).await, "one at a time");
        drop(only);
        assert!(!waits(tiny.reserve(1)).await);
    }

    /// The estimate covers every read measured (docs/FIELD-NOTES.md, Fault 9), and still
    /// lets a 32 GB, sixteen-core machine read sixteen of the heaviest at once: the budget
    /// is not to hold a large machine below what it did before there was one.
    #[test]
    fn a_read_is_reserved_above_every_peak_measured_and_a_large_machine_reads_wide() {
        let mb = 1_000_000;
        let measured = [
            (200_000, 49 * mb),
            (6_900_000, 64 * mb),
            (15_800_000, 671 * mb),
            (19_300_000, 561 * mb),
            (32_000_000, 669 * mb),
            (52_000_000, 867 * mb),
        ];
        for (size, peak) in measured {
            assert!(
                read_cost(size) > peak,
                "{size} bytes: {} <= {peak}",
                read_cost(size)
            );
        }

        let large = Budget::auto(&Machine {
            total: Some(32 << 30),
            ..Machine::default()
        });
        assert!(large.room() >= 16 * read_cost(52 * mb), "{}", large.room());
    }

    #[test]
    fn sizes_read_as_they_are_written() {
        assert_eq!(parse_size("1.5 GB"), Ok(1_500_000_000));
        assert_eq!(parse_size("1536 MiB"), Ok(1536 * MIB));
        assert_eq!(parse_size("1400M"), Ok(1400 * MIB), "systemd's spelling");
        assert_eq!(parse_size("2gib"), Ok(2 << 30));
        assert_eq!(parse_size("4096"), Ok(4096));
        assert!(parse_size("lots").is_err());
        assert!(parse_size("2 GHz").is_err());
        assert!(parse_size("0 GB").is_err());
    }

    /// The cgroup is the machine a process in one has, so its limit wins when it is the
    /// smaller; a machine with no limit gets its share of its own memory.
    #[test]
    fn the_budget_is_a_share_of_the_tighter_of_machine_and_cgroup() {
        let machine = |total, cgroup| Machine {
            total,
            cgroup,
            ..Machine::default()
        };
        let gib = 1 << 30;

        let alone = Budget::auto(&machine(Some(16 * gib), None));
        assert_eq!((alone.bytes(), alone.origin()), (12 * gib, Origin::Machine));

        let scoped = Budget::auto(&machine(Some(16 * gib), Some(1400 * MIB)));
        assert_eq!(
            (scoped.bytes(), scoped.origin()),
            (1050 * MIB, Origin::Cgroup)
        );

        // v1 says "unlimited" as a number near i64::MAX, which must not win.
        let v1 = Budget::auto(&machine(Some(16 * gib), Some(9_223_372_036_854_771_712)));
        assert_eq!(v1.origin(), Origin::Machine);
    }

    /// The scope `systemd-run -p MemoryMax=1400M` makes has the limit, under a slice and a
    /// root that have none; and a slice above can be tighter than the scope.
    #[test]
    fn the_tightest_cgroup_limit_on_the_way_up_is_found() {
        let root = tempfile::tempdir().unwrap();
        let v2 = root.path().join("v2");
        let scope = v2.join("user.slice/run-1.scope");
        std::fs::create_dir_all(&scope).unwrap();
        std::fs::write(v2.join("user.slice/memory.max"), "max\n").unwrap();
        std::fs::write(scope.join("memory.max"), "1468006400\n").unwrap();
        let own = "0::/user.slice/run-1.scope\n";
        let v1 = root.path().join("v1");

        assert_eq!(cgroup_limit(own, &v2, &v1), Some(1_468_006_400));

        std::fs::write(v2.join("user.slice/memory.max"), "1073741824\n").unwrap();
        assert_eq!(cgroup_limit(own, &v2, &v1), Some(1 << 30));

        assert_eq!(cgroup_limit("0::/\n", &v1, &v1), None, "nothing set");
    }

    #[test]
    fn meminfo_is_read_in_bytes() {
        let meminfo = "MemTotal:        3423456 kB\nMemFree:  10 kB\nMemAvailable:    2005000 kB\n";
        assert_eq!(meminfo_field(meminfo, "MemTotal"), Some(3_423_456 * 1024));
        assert_eq!(
            meminfo_field(meminfo, "MemAvailable"),
            Some(2_005_000 * 1024)
        );
        assert_eq!(meminfo_field(meminfo, "SwapTotal"), None);
    }

    /// The owner's rule: a small machine keeps the small caches it had, and a large one
    /// gets Lance's own defaults back.
    #[test]
    fn the_lance_caches_scale_from_small_to_lances_defaults() {
        let field_vm = Budget::new(2_550_000_000, Origin::Machine);
        assert_eq!(field_vm.metadata_cache() as u64, METADATA_CACHE_FLOOR);
        assert!(field_vm.index_cache() < 1 << 30);

        let large = Budget::new(48 << 30, Origin::Machine);
        assert_eq!(large.index_cache() as u64, INDEX_CACHE_CEILING);
        assert!(large.metadata_cache() as u64 > METADATA_CACHE_FLOOR);
    }
}
