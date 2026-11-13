//! The process's allocator: jemalloc, set to hand freed memory back to the system.
//!
//! glibc's malloc keeps what a thread frees for that thread's next request. Extract parses
//! a document into hundreds of megabytes of short-lived heap, so under glibc RSS rose to
//! each heavy document's peak and stayed there: a one-reader extract on a 3.3 GB VM was
//! OOM-killed at 2 GB while parsing 200 KB agendas. Under jemalloc with the settings below,
//! the same work fell back to 100–270 MB after each heavy document and finished at a 999 MB
//! peak. docs/FIELD-NOTES.md has the run, Fault 9 and the log at 07:22Z.
//!
//! tikv-jemalloc-sys builds jemalloc with its symbols prefixed (`_rjem_malloc` and so on),
//! and `unprefixed_malloc_on_supported_platforms` stays off, so only Rust's heap moves to
//! it. C code linked in — SQLite, zstd, lz4 — keeps the system malloc. The heap that
//! ratcheted was Rust's: lopdf's operands, in Fault 9.
//!
//! Unix only. jemalloc's build runs `sh ./configure` and `make`, which an MSVC host has
//! neither of; a Windows build, should one ever exist, keeps the system allocator.

use std::ffi::{CStr, c_char};

#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

/// Whether jemalloc can run its background threads here: glibc Linux only. It has none on
/// macOS, where asking for them fails its initialisation and with it every allocation, and
/// tikv-jemalloc-sys lists musl as a target they do not work on.
const BACKGROUND_THREAD: bool = cfg!(all(target_os = "linux", target_env = "gnu"));

/// The settings the field run measured:
///
/// - `dirty_decay_ms:1000` — a freed page goes back to the system about a second after it
///   was last used, not jemalloc's default ten.
/// - `muzzy_decay_ms:0` — already jemalloc 5.3's default, so it changes nothing; it stays
///   so the string is the one the run measured.
/// - `background_thread:true` — a jemalloc thread does the returning, so it happens while
///   extract sits in one long parse, not only when something next allocates. Only where
///   [`BACKGROUND_THREAD`] says it can.
const CONF: &CStr = if BACKGROUND_THREAD {
    c"background_thread:true,dirty_decay_ms:1000,muzzy_decay_ms:0"
} else {
    c"dirty_decay_ms:1000,muzzy_decay_ms:0"
};

/// jemalloc reads this symbol before its first allocation, and reads `_RJEM_MALLOC_CONF`
/// from the environment after it, so a host can retune without a rebuild — the
/// environment wins. The name carries the same `_rjem_` prefix as the build; a symbol under
/// any other name links fine and is never read, which is what the test below is for.
#[unsafe(export_name = "_rjem_malloc_conf")]
static MALLOC_CONF: Option<&c_char> = Some(unsafe { &*CONF.as_ptr() });

#[cfg(test)]
mod tests {
    use std::ffi::{CStr, c_void};

    /// Reads one of jemalloc's `opt.*` values, which is what it settled on at start.
    fn opt<T: Default>(name: &CStr) -> T {
        let mut value = T::default();
        let mut len = size_of::<T>();
        let status = unsafe {
            tikv_jemalloc_sys::mallctl(
                name.as_ptr(),
                (&raw mut value).cast::<c_void>(),
                &raw mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        assert_eq!(status, 0, "mallctl {name:?}");
        value
    }

    /// The settings reach jemalloc. A misnamed symbol is not a link error; jemalloc would
    /// quietly run on its defaults, and the first sign would be the OOM killer.
    #[test]
    fn jemalloc_runs_with_the_measured_settings() {
        // `_RJEM_MALLOC_CONF` overrides these by design; a test environment that sets it
        // is asking for something else.
        if std::env::var_os("_RJEM_MALLOC_CONF").is_some() {
            return;
        }
        assert_eq!(opt::<isize>(c"opt.dirty_decay_ms"), 1000);
        assert_eq!(
            opt::<bool>(c"opt.background_thread"),
            super::BACKGROUND_THREAD
        );
    }
}
