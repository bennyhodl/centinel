//! Running an external program.
//!
//! SPEC §3 accepts that Centinel shells out — to `yt-dlp`, `ffmpeg`, the whisper worker,
//! and whichever application opens a PDF. It does not accept that each caller should
//! invent its own way of doing it, and until this module existed each one did: seven call
//! sites, three error conventions, two different `Command` types, and between them **no
//! timeout and no `kill_on_drop` anywhere**.
//!
//! That combination has two failure modes, and both were reachable:
//!
//! - **A child that hangs blocks its caller forever.** `yt-dlp` waiting on a prompt or a
//!   stalled socket, a whisper worker wedged on a corrupt model — nothing was watching.
//! - **A cancelled caller orphans its children.** Ctrl-C during transcription left an
//!   `ffmpeg` and a whisper worker running, the second of them holding a multi-gigabyte
//!   model, with nothing left to reap them.
//!
//! ## What this interface guarantees
//!
//! Every child started here:
//!
//! 1. **dies with its caller** — `kill_on_drop`, so a dropped future takes the process
//!    with it rather than leaving it to the init system;
//! 2. **has a deadline** — and exceeding it kills the child rather than reporting a
//!    stalled read;
//! 3. **never reads our stdin** — an inherited terminal lets a child swallow keystrokes
//!    or block on a prompt nobody can see;
//! 4. **fails with one error type**, which names the program and separates "not installed"
//!    from "timed out" from "ran and refused".
//!
//! [`Tool::interactive`] is the deliberate exception, and it says so.
//!
//! ## Deadlines are per call, not global
//!
//! A `--version` probe should answer in a second; a three-hour meeting's audio download
//! should not be cut off at two minutes. So there is no single right number here and no
//! attempt to invent one — [`DEFAULT_TIMEOUT`] suits a probe, and anything longer states
//! its own.
//!
//! For a *stream*, the useful guard is inactivity rather than total time: a transcription
//! that is still emitting progress after four hours is working, not stuck. That is the
//! same distinction [`crate::models::download`] makes about HTTP reads, and [`Tool::spawn`]
//! leaves it to the caller because only the caller knows what a heartbeat looks like.

use std::ffi::{OsStr, OsString};
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use tokio::process::{Child, Command};

/// The deadline a probe gets when nothing says otherwise.
///
/// Sized for "ask a binary its version", which is what most callers do. Anything that
/// touches the network or a model states its own and is expected to.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

/// Why an external program did not produce an answer.
///
/// Carries the program's name in every variant, because "No such file or directory" with
/// nothing attached is the least useful error this codebase can emit.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("cannot run `{program}`: {source} — it is not installed, or not on PATH")]
    NotFound {
        program: String,
        source: std::io::Error,
    },

    /// The child exceeded its deadline and was killed.
    #[error("`{program}` did not finish within {}s and was stopped", after.as_secs())]
    TimedOut { program: String, after: Duration },

    /// The child ran and exited non-zero. Only [`Tool::success`] produces this;
    /// [`Tool::output`] hands the status back as data.
    #[error("`{program}` exited with {status}: {stderr}")]
    Failed {
        program: String,
        status: ExitStatus,
        stderr: String,
    },

    #[error("`{program}` failed: {source}")]
    Io {
        program: String,
        source: std::io::Error,
    },
}

impl ToolError {
    /// The program this is about.
    pub fn program(&self) -> &str {
        match self {
            Self::NotFound { program, .. }
            | Self::TimedOut { program, .. }
            | Self::Failed { program, .. }
            | Self::Io { program, .. } => program,
        }
    }

    /// Whatever the program said on stderr, or an empty string.
    pub fn stderr(&self) -> &str {
        match self {
            Self::Failed { stderr, .. } => stderr,
            _ => "",
        }
    }
}

/// A program that never produced an answer is a **transport fault**, not evidence about
/// the resource.
///
/// Distinct from a refusal the program *reported*: `yt-dlp` saying "video unavailable" is
/// a fact about the video and is classified from its stderr. A missing binary or a killed
/// hang says nothing about the video at all, and recording it as `Gone` would mark a live
/// recording deleted — the same mistake [`crate::domain::Liveness::Blocked`] exists to
/// prevent one level up.
impl From<ToolError> for crate::domain::Refusal {
    fn from(e: ToolError) -> Self {
        Self {
            state: crate::domain::Liveness::Error,
            detail: e.to_string(),
        }
    }
}

/// What a finished program produced. A non-zero `status` is data, not an error.
#[derive(Clone, Debug)]
pub struct Output {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl Output {
    /// The first non-empty line of whichever stream spoke.
    ///
    /// Programs disagree about where a version string belongs — poppler's tools print
    /// theirs to stderr, most others to stdout — so asking "what did it say" is more
    /// useful than picking a stream and hoping.
    pub fn first_line(&self) -> Option<String> {
        let merged = if self.stdout.is_empty() {
            &self.stderr
        } else {
            &self.stdout
        };
        String::from_utf8_lossy(merged)
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .map(str::to_string)
    }
}

/// Which of a child's streams the caller wants to hold.
///
/// A closed set rather than a `Command` handed back, so the guarantees above cannot be
/// undone by a caller reaching past them.
#[derive(Clone, Copy, Debug, Default)]
pub struct Pipes {
    /// Give the caller the child's stdin. Off means the child gets nothing, ever.
    pub stdin: bool,
    pub stdout: bool,
    pub stderr: bool,
}

impl Pipes {
    /// Read both output streams; write nothing.
    pub fn read() -> Self {
        Self {
            stdin: false,
            stdout: true,
            stderr: true,
        }
    }

    /// Feed the child and read both output streams.
    pub fn duplex() -> Self {
        Self {
            stdin: true,
            stdout: true,
            stderr: true,
        }
    }
}

/// One external program, and how to run it.
#[derive(Clone, Debug)]
pub struct Tool {
    program: OsString,
    args: Vec<OsString>,
    timeout: Duration,
}

impl Tool {
    pub fn new(program: impl AsRef<OsStr>) -> Self {
        Self {
            program: program.as_ref().to_os_string(),
            args: Vec::new(),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    pub fn arg(mut self, arg: impl AsRef<OsStr>) -> Self {
        self.args.push(arg.as_ref().to_os_string());
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.args
            .extend(args.into_iter().map(|a| a.as_ref().to_os_string()));
        self
    }

    /// Sets this call's deadline. Exceeding it kills the child.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// The name this program is reported under.
    pub fn name(&self) -> String {
        self.program.to_string_lossy().to_string()
    }

    /// The command line, for a diagnostic. Not shell-quoted — it is for reading.
    pub fn display(&self) -> String {
        std::iter::once(self.program.to_string_lossy().to_string())
            .chain(self.args.iter().map(|a| a.to_string_lossy().to_string()))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// A `Command` carrying the guarantees this module exists to make.
    fn command(&self) -> Command {
        let mut cmd = Command::new(&self.program);
        cmd.args(&self.args)
            // The one that turns a cancelled future into a dead process rather than an
            // orphan holding a multi-gigabyte model.
            .kill_on_drop(true)
            // Never inherit a terminal. A child that can read our stdin can swallow
            // keystrokes or block on a prompt nobody can see — measured with ffmpeg.
            .stdin(Stdio::null());
        cmd
    }

    /// Runs to completion and captures both streams.
    ///
    /// A non-zero exit is **data**, not an error: `yt-dlp` says why it refused on stderr
    /// and poppler prints its version there while exiting non-zero. Callers that want a
    /// failure to be an error want [`Self::success`].
    pub async fn output(&self) -> Result<Output, ToolError> {
        let child = self
            .command()
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| self.spawn_error(e))?;

        // Dropping this future on timeout drops the `Child`, and `kill_on_drop` does the
        // rest. That is the entire reason the flag is set above.
        let out = tokio::time::timeout(self.timeout, child.wait_with_output())
            .await
            .map_err(|_| ToolError::TimedOut {
                program: self.name(),
                after: self.timeout,
            })?
            .map_err(|e| ToolError::Io {
                program: self.name(),
                source: e,
            })?;

        Ok(Output {
            status: out.status,
            stdout: out.stdout,
            stderr: out.stderr,
        })
    }

    /// Runs to completion and treats a non-zero exit as an error carrying stderr.
    pub async fn success(&self) -> Result<Vec<u8>, ToolError> {
        let out = self.output().await?;
        if !out.status.success() {
            return Err(ToolError::Failed {
                program: self.name(),
                status: out.status,
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
            });
        }
        Ok(out.stdout)
    }

    /// Starts the program and hands back the running child.
    ///
    /// For the streaming case, where the output is too large to buffer — a three-hour
    /// meeting is ~691 MB of PCM. The child is already `kill_on_drop`, so dropping it is
    /// enough to stop it; **the deadline is the caller's**, because a stream's useful
    /// guard is inactivity rather than total time and only the caller knows what a
    /// heartbeat from this program looks like.
    pub fn spawn(&self, pipes: Pipes) -> Result<Child, ToolError> {
        let mut cmd = self.command();
        if pipes.stdin {
            cmd.stdin(Stdio::piped());
        }
        cmd.stdout(if pipes.stdout {
            Stdio::piped()
        } else {
            Stdio::null()
        });
        cmd.stderr(if pipes.stderr {
            Stdio::piped()
        } else {
            Stdio::null()
        });
        cmd.spawn().map_err(|e| self.spawn_error(e))
    }

    /// Runs the program **in the foreground**, attached to this process's terminal.
    ///
    /// The deliberate exception to everything above, and the interface says so: no
    /// deadline, no `kill_on_drop`, and every stream inherited. It exists for `open`,
    /// whose "program" may be a person's editor — `nvim {path}` needs the terminal, and
    /// a deadline would close a document somebody was still reading.
    ///
    /// Still `tokio::process` and still awaited, because the previous version used
    /// `std::process::Command::status()` and blocked a runtime thread for as long as the
    /// application stayed open.
    pub async fn interactive(&self) -> Result<ExitStatus, ToolError> {
        Command::new(&self.program)
            .args(&self.args)
            .status()
            .await
            .map_err(|e| self.spawn_error(e))
    }

    /// Distinguishes "not installed" from every other way a spawn can fail, because the
    /// first has an obvious fix and the rest do not.
    fn spawn_error(&self, e: std::io::Error) -> ToolError {
        if e.kind() == std::io::ErrorKind::NotFound {
            ToolError::NotFound {
                program: self.name(),
                source: e,
            }
        } else {
            ToolError::Io {
                program: self.name(),
                source: e,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A program every target of this project has, so the guarantees can be tested
    /// against a real process rather than a mock of one. `doctor` already shells out to
    /// `sh`, so this assumes nothing new.
    fn sh(script: &str) -> Tool {
        Tool::new("sh").arg("-c").arg(script)
    }

    #[tokio::test]
    async fn output_captures_both_streams_and_hands_back_the_status() {
        let out = sh("echo to-stdout; echo to-stderr >&2; exit 3")
            .output()
            .await
            .unwrap();

        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "to-stdout");
        assert_eq!(String::from_utf8_lossy(&out.stderr).trim(), "to-stderr");
        assert_eq!(
            out.status.code(),
            Some(3),
            "a refusal is data, not an error"
        );
    }

    #[tokio::test]
    async fn success_turns_a_non_zero_exit_into_an_error_that_keeps_stderr() {
        let err = sh("echo it-broke >&2; exit 1").success().await.unwrap_err();
        assert!(matches!(err, ToolError::Failed { .. }));
        assert_eq!(err.stderr(), "it-broke");
        assert_eq!(err.program(), "sh");
        assert!(err.to_string().contains("it-broke"), "{err}");
    }

    #[tokio::test]
    async fn a_missing_program_says_it_is_not_installed() {
        let err = Tool::new("centinel-no-such-binary")
            .output()
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::NotFound { .. }));
        assert!(err.to_string().contains("not installed"), "{err}");
        assert!(err.to_string().contains("centinel-no-such-binary"), "{err}");
    }

    /// The hazard this module was written for: nothing was watching a child that hung.
    #[tokio::test]
    async fn a_child_that_hangs_is_stopped_at_its_deadline() {
        let started = std::time::Instant::now();
        let err = sh("sleep 30")
            .timeout(Duration::from_millis(300))
            .output()
            .await
            .unwrap_err();

        assert!(matches!(err, ToolError::TimedOut { .. }), "{err}");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the deadline did not fire: {:?}",
            started.elapsed()
        );
        assert!(err.to_string().contains("was stopped"), "{err}");
    }

    /// The other hazard: a cancelled caller used to leave its children running.
    ///
    /// The child writes to a file after a delay. If dropping the future really kills it,
    /// the file never appears.
    #[tokio::test]
    async fn dropping_the_caller_kills_the_child() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("survived");
        let script = format!("sleep 1; touch {}", marker.display());

        {
            let tool = sh(&script);
            let call = tool.output();
            // Give the child time to exist, then abandon it — exactly what Ctrl-C or a
            // `select!` losing a race does to the caller.
            let _ = tokio::time::timeout(Duration::from_millis(200), call).await;
        }

        tokio::time::sleep(Duration::from_millis(1_800)).await;
        assert!(
            !marker.exists(),
            "the child outlived its caller and finished its work"
        );
    }

    /// A child must not be able to read the terminal out from under the operator.
    #[tokio::test]
    async fn a_child_never_receives_our_stdin() {
        // `cat` with an inherited stdin would block here forever; with /dev/null it ends.
        let out = tokio::time::timeout(Duration::from_secs(5), Tool::new("cat").output())
            .await
            .expect("stdin was not denied — the child is waiting on a terminal")
            .unwrap();
        assert!(out.status.success());
        assert!(out.stdout.is_empty());
    }

    #[tokio::test]
    async fn a_spawned_child_streams_rather_than_buffers() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let mut child = sh("cat").spawn(Pipes::duplex()).unwrap();
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(b"through the pipe").await.unwrap();
        drop(stdin);

        let mut buf = String::new();
        child
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut buf)
            .await
            .unwrap();
        assert_eq!(buf, "through the pipe");
        assert!(child.wait().await.unwrap().success());
    }

    #[tokio::test]
    async fn a_spawned_child_is_also_killed_when_it_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("survived");
        let child = sh(&format!("sleep 1; touch {}", marker.display()))
            .spawn(Pipes::read())
            .unwrap();
        drop(child);

        tokio::time::sleep(Duration::from_millis(1_800)).await;
        assert!(!marker.exists(), "a dropped child kept running");
    }

    #[test]
    fn a_version_string_is_read_from_whichever_stream_carried_it() {
        let on_stdout = Output {
            status: std::process::Command::new("true").status().unwrap(),
            stdout: b"yt-dlp 2026.07.04\n".to_vec(),
            stderr: Vec::new(),
        };
        assert_eq!(on_stdout.first_line().as_deref(), Some("yt-dlp 2026.07.04"));

        // poppler prints its version to stderr.
        let on_stderr = Output {
            stdout: Vec::new(),
            stderr: b"pdftoppm version 24.08.0\n".to_vec(),
            ..on_stdout.clone()
        };
        assert_eq!(
            on_stderr.first_line().as_deref(),
            Some("pdftoppm version 24.08.0")
        );

        let silent = Output {
            stdout: Vec::new(),
            stderr: b"  \n\n".to_vec(),
            ..on_stdout
        };
        assert_eq!(silent.first_line(), None);
    }

    #[test]
    fn a_tool_can_say_what_it_would_run() {
        let t = Tool::new("yt-dlp")
            .args(["--flat-playlist", "-J"])
            .arg("url");
        assert_eq!(t.name(), "yt-dlp");
        assert_eq!(t.display(), "yt-dlp --flat-playlist -J url");
    }
}
