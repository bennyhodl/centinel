//! `centinel serve start|stop|restart|status` — `serve` as a service of the user's login.
//!
//! The service manager the OS already has does the supervising: a launchd agent on macOS,
//! a systemd user unit on Linux. The unit runs this binary's own `serve`, so it is the
//! same process `centinel serve` is in a terminal — no second daemon (SPEC §2.3) — and
//! everything that process does on the way out (the scheduler's `interrupted` record,
//! taking its tailnet mapping down) happens on `stop` too, because both managers stop a
//! service with `SIGTERM`.
//!
//! - **start** writes the unit with the flags it was given and starts it, replacing any
//!   service already installed. It returns once the server is answering, and prints where.
//! - **stop** stops it and removes the unit, so it does not come back at login.
//! - **restart** stops and starts the installed unit as it was written.
//! - **status** says whether it is running and where.
//!
//! One service per user. The store it serves is written into the unit as `CENTINEL_ROOT`
//! and read back from there, so `status` and `stop` need no `--root`.
//!
//! ## Where the server says where it is
//!
//! [`Running`] is `serve.json` in the store: written by `serve` itself once it is
//! reachable, removed when it stops. `start` waits for it, `status` prints it. The server
//! writes it in a terminal too, which is what lets `start` refuse to start a second server
//! on a store somebody is already serving by hand.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use centinel_core::store::Store;
use centinel_core::tool::Tool;
use serde::{Deserialize, Serialize};

const LABEL: &str = "io.github.bennyhodl.centinel";
const UNIT: &str = "centinel.service";

/// What the unit carries from the shell that ran `start`. A service inherits nothing
/// from it: without `PATH` it cannot find `tailscale`, `ffmpeg` or `yt-dlp`, and without
/// the keys it cannot embed or classify. The unit file is written `0600` for the keys'
/// sake. `TYPESAFE_API_KEY` can live in `<root>/.env` instead, which the service reads
/// because it runs in the store root.
const FORWARDED_ENV: [&str; 6] = [
    "PATH",
    "OPENROUTER_API_KEY",
    "TYPESAFE_API_KEY",
    "CENTINEL_PUBLIC_URL",
    "CENTINEL_CONFIG",
    "RUST_LOG",
];

/// How long a stopping server gets before the manager kills it. Long enough for a
/// scheduled run to reach an item boundary and write its `interrupted` record.
const EXIT_TIMEOUT_SECS: u64 = 90;

/// Every control command waits out a full stop, and a little more.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(EXIT_TIMEOUT_SECS + 30);

/// Opening a large store is not instant; a server that has not answered in this long
/// is reported with the end of its log.
const READY_TIMEOUT: Duration = Duration::from_secs(60);

/// The `centinel serve` running on a store, as it describes itself in `serve.json`.
#[derive(Debug, Serialize, Deserialize)]
pub struct Running {
    pub pid: u32,
    /// `http://127.0.0.1:8787` — an address a browser on this machine can open.
    pub local: String,
    /// The tailnet origin, when `--tailscale` published one.
    pub tailnet: Option<String>,
}

impl Running {
    pub fn new(mut local: SocketAddr, tailnet: Option<String>) -> Self {
        // A wildcard bind is reachable on loopback; `http://0.0.0.0:8787` is not a URL
        // anyone can open.
        if local.ip().is_unspecified() {
            local.set_ip(match local {
                SocketAddr::V4(_) => std::net::Ipv4Addr::LOCALHOST.into(),
                SocketAddr::V6(_) => std::net::Ipv6Addr::LOCALHOST.into(),
            });
        }
        Self {
            pid: std::process::id(),
            local: format!("http://{local}"),
            tailnet,
        }
    }

    pub fn write(&self, store: &Store) -> Result<()> {
        let path = store.serve_record_path();
        std::fs::write(&path, serde_json::to_vec_pretty(self)?)
            .with_context(|| format!("writing {}", path.display()))
    }

    pub fn read(store: &Store) -> Option<Self> {
        let bytes = std::fs::read(store.serve_record_path()).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Removes the record if this process wrote it. A second server on the same store
    /// would otherwise have its record removed by the first one stopping.
    pub fn clear(store: &Store) {
        if Self::read(store).is_some_and(|r| r.pid == std::process::id()) {
            let _ = std::fs::remove_file(store.serve_record_path());
        }
    }

    /// The addresses, one per line, tailnet first when there is one.
    fn render(&self, log: &Path) -> String {
        let base = self.tailnet.as_deref().unwrap_or(&self.local);
        let mut out = format!("web    {base}/web\nmcp    {base}/mcp\n");
        if self.tailnet.is_some() {
            out.push_str(&format!("local  {}\n", self.local));
        }
        out.push_str(&format!("log    {}\n", log.display()));
        out
    }
}

/// Installs `serve <serve_args>` for `root` as the user's service and starts it.
pub async fn start(root: &Path, serve_args: Vec<String>) -> Result<()> {
    let manager = Manager::detect()?;
    std::fs::create_dir_all(root).with_context(|| format!("creating {}", root.display()))?;
    let root = std::fs::canonicalize(root)?;
    let store = Store::at(&root);

    if let Some(previous) = manager.installed_root() {
        if previous != root {
            tracing::info!(previous = %previous.display(), "replacing the service for another store");
        }
        // Stopped before anything is checked, so its record and its tailnet mapping are
        // gone by the time the new one starts.
        manager.deactivate().await?;
    }
    if let Some(running) = Running::read(&store) {
        if alive(running.pid) {
            bail!(
                "a `centinel serve` (pid {}) is already running on {}; stop it, then run \
                 `centinel serve start` again",
                running.pid,
                root.display()
            );
        }
        // Left by a server that was killed rather than stopped.
        let _ = std::fs::remove_file(store.serve_record_path());
    }

    let exe = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .context("locating this centinel binary")?;
    let mut program = vec![exe.display().to_string(), "serve".to_string()];
    program.extend(serve_args);
    let mut env = vec![("CENTINEL_ROOT".to_string(), root.display().to_string())];
    env.extend(FORWARDED_ENV.iter().filter_map(|name| {
        let value = std::env::var(name).ok().filter(|v| !v.is_empty())?;
        Some((name.to_string(), value))
    }));
    let spec = Spec {
        program,
        env,
        log: store.serve_log_path(),
        root: root.clone(),
    };

    manager.install(&spec)?;
    manager.activate().await?;
    tracing::info!(unit = %manager.path().display(), "service installed");
    manager.warn_if_not_kept().await;
    ready(&manager, &store).await
}

/// Stops the service and removes its unit, so it does not start at the next login.
pub async fn stop() -> Result<()> {
    let manager = Manager::detect()?;
    if manager.installed_root().is_none() {
        println!("centinel serve is not installed as a service");
        return Ok(());
    }
    manager.deactivate().await?;
    manager.uninstall().await?;
    println!("stopped; it will not start at login. `centinel serve start` installs it again.");
    Ok(())
}

/// Stops and starts the installed service, as it was written.
pub async fn restart() -> Result<()> {
    let manager = Manager::detect()?;
    let Some(root) = manager.installed_root() else {
        bail!("centinel serve is not installed as a service; `centinel serve start` installs it");
    };
    manager.deactivate().await?;
    manager.activate().await?;
    ready(&manager, &Store::at(root)).await
}

pub async fn status() -> Result<()> {
    let manager = Manager::detect()?;
    let Some(root) = manager.installed_root() else {
        println!(
            "centinel serve is not installed as a service; `centinel serve start` installs it"
        );
        return Ok(());
    };
    let store = Store::at(&root);
    let log = store.serve_log_path();
    match manager.state().await {
        State::Running { pid } => {
            println!("running  pid {pid}, {}", manager.describe());
            match Running::read(&store).filter(|r| r.pid == pid) {
                Some(running) => print!("{}", running.render(&log)),
                None => println!("starting — not answering yet\nlog    {}", log.display()),
            }
        }
        State::Stopped(detail) => {
            println!("stopped  {detail}, {}", manager.describe());
            println!("log    {}\n{}", log.display(), tail(&log, 10));
        }
    }
    println!("store  {}", root.display());
    manager.warn_if_not_kept().await;
    Ok(())
}

/// Waits until the service's server has written its record, then prints it. A service
/// that stops instead is reported with the end of its log.
async fn ready(manager: &Manager, store: &Store) -> Result<()> {
    let log = store.serve_log_path();
    let started = Instant::now();
    loop {
        tokio::time::sleep(Duration::from_millis(250)).await;
        let state = manager.state().await;
        if let State::Running { pid } = state
            && let Some(running) = Running::read(store).filter(|r| r.pid == pid)
        {
            print!("{}", running.render(&log));
            return Ok(());
        }
        // A moment's grace: a manager can report the job as not yet running just after
        // it was asked to start it.
        let gave_up = match &state {
            State::Stopped(_) => started.elapsed() > Duration::from_secs(2),
            State::Running { .. } => started.elapsed() > READY_TIMEOUT,
        };
        if gave_up {
            let why = match state {
                State::Stopped(detail) => format!("the server stopped ({detail})"),
                State::Running { .. } => format!(
                    "the server did not answer within {}s",
                    READY_TIMEOUT.as_secs()
                ),
            };
            bail!(
                "{why}. It is installed and the service manager will keep retrying; \
                 `centinel serve stop` removes it. The end of {}:\n{}",
                log.display(),
                tail(&log, 20)
            );
        }
    }
}

/// The last `n` lines of a log, or a note that there is none.
fn tail(path: &Path, n: usize) -> String {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let lines: Vec<&str> = text.lines().collect();
            lines[lines.len().saturating_sub(n)..].join("\n")
        }
        Err(_) => "(no log yet)".to_string(),
    }
}

#[cfg(unix)]
fn alive(pid: u32) -> bool {
    // Signal 0 checks the process exists without touching it.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[cfg(not(unix))]
fn alive(_pid: u32) -> bool {
    false
}

/// What the unit runs, and with what.
struct Spec {
    /// The absolute binary, `serve`, and its flags.
    program: Vec<String>,
    env: Vec<(String, String)>,
    log: PathBuf,
    root: PathBuf,
}

enum State {
    Running { pid: u32 },
    Stopped(String),
}

/// Each platform constructs one of these.
#[allow(dead_code)]
enum Manager {
    Launchd { plist: PathBuf, domain: String },
    Systemd { unit: PathBuf },
}

impl Manager {
    fn detect() -> Result<Self> {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .context("HOME is not set")?;
        #[cfg(target_os = "macos")]
        {
            let uid = unsafe { libc::getuid() };
            Ok(Self::Launchd {
                plist: home.join(format!("Library/LaunchAgents/{LABEL}.plist")),
                domain: format!("gui/{uid}"),
            })
        }
        #[cfg(target_os = "linux")]
        {
            let config = std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".config"));
            Ok(Self::Systemd {
                unit: config.join("systemd/user").join(UNIT),
            })
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            let _ = home;
            bail!(
                "running centinel as a service needs launchd or systemd; run `centinel serve` \
                 under your own supervisor instead"
            )
        }
    }

    fn path(&self) -> &Path {
        match self {
            Self::Launchd { plist, .. } => plist,
            Self::Systemd { unit } => unit,
        }
    }

    fn describe(&self) -> String {
        match self {
            Self::Launchd { .. } => format!("launchd {LABEL}"),
            Self::Systemd { .. } => format!("systemd --user {UNIT}"),
        }
    }

    /// The store the installed unit serves, or `None` when nothing is installed.
    fn installed_root(&self) -> Option<PathBuf> {
        let text = std::fs::read_to_string(self.path()).ok()?;
        match self {
            Self::Launchd { .. } => plist_root(&text),
            Self::Systemd { .. } => unit_root(&text),
        }
    }

    fn install(&self, spec: &Spec) -> Result<()> {
        let path = self.path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = match self {
            Self::Launchd { .. } => render_plist(spec),
            Self::Systemd { .. } => render_unit(spec),
        };
        std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    async fn uninstall(&self) -> Result<()> {
        let path = self.path();
        std::fs::remove_file(path).with_context(|| format!("removing {}", path.display()))?;
        if let Self::Systemd { .. } = self {
            run("systemctl", &["--user", "daemon-reload"]).await?;
        }
        Ok(())
    }

    /// Starts the installed unit, and marks it to start at login.
    async fn activate(&self) -> Result<()> {
        match self {
            Self::Launchd { plist, domain } => {
                let target = format!("{domain}/{LABEL}");
                // A persisted `launchctl disable` refuses the bootstrap; clear it.
                let _ = run("launchctl", &["enable", &target]).await;
                // Loading a `RunAtLoad` agent is what starts it, and what loads it at
                // every later login while the plist is in place.
                run(
                    "launchctl",
                    &["bootstrap", domain, &plist.display().to_string()],
                )
                .await?;
            }
            Self::Systemd { .. } => {
                run("systemctl", &["--user", "daemon-reload"]).await?;
                run("systemctl", &["--user", "enable", UNIT]).await?;
                run("systemctl", &["--user", "restart", UNIT]).await?;
            }
        }
        Ok(())
    }

    /// Stops the unit and waits for the server to exit. Not loaded is not an error.
    async fn deactivate(&self) -> Result<()> {
        match self {
            Self::Launchd { domain, .. } => {
                // `--wait` blocks until the job is gone; without it a bootstrap straight
                // after fails while the old server is still draining.
                let target = format!("{domain}/{LABEL}");
                if let Err(e) = run("launchctl", &["bootout", "--wait", &target]).await
                    && matches!(self.state().await, State::Running { .. })
                {
                    return Err(e);
                }
            }
            Self::Systemd { .. } => {
                if self.path().exists() {
                    run("systemctl", &["--user", "disable", "--now", UNIT]).await?;
                }
            }
        }
        Ok(())
    }

    async fn state(&self) -> State {
        match self {
            Self::Launchd { domain, .. } => {
                let target = format!("{domain}/{LABEL}");
                let Ok(out) = run("launchctl", &["print", &target]).await else {
                    return State::Stopped("not loaded".into());
                };
                let field = |name: &str| {
                    out.lines()
                        .find_map(|l| l.trim().strip_prefix(name)?.strip_prefix(" = "))
                        .map(str::to_string)
                };
                match (
                    field("state").as_deref(),
                    field("pid").and_then(|pid| pid.parse().ok()),
                ) {
                    (Some("running"), Some(pid)) => State::Running { pid },
                    (state, _) => State::Stopped(format!(
                        "{}, last exit {}",
                        state.unwrap_or("unknown"),
                        field("last exit code").unwrap_or_else(|| "unknown".into())
                    )),
                }
            }
            Self::Systemd { .. } => {
                let property = "--property=ActiveState,SubState,MainPID,ExecMainStatus";
                let Ok(out) = run("systemctl", &["--user", "show", UNIT, property]).await else {
                    return State::Stopped("systemd did not answer".into());
                };
                let field = |name: &str| {
                    out.lines()
                        .find_map(|l| l.strip_prefix(name)?.strip_prefix('='))
                        .unwrap_or("")
                        .to_string()
                };
                let pid = field("MainPID").parse::<u32>().unwrap_or(0);
                if field("ActiveState") == "active" && pid > 0 {
                    State::Running { pid }
                } else {
                    State::Stopped(format!(
                        "{}/{}, last exit {}",
                        field("ActiveState"),
                        field("SubState"),
                        field("ExecMainStatus")
                    ))
                }
            }
        }
    }

    /// A systemd user service stops at logout unless the user lingers; say so where it
    /// would otherwise be found out the next morning.
    async fn warn_if_not_kept(&self) {
        if let Self::Systemd { .. } = self {
            let user = std::env::var("USER").unwrap_or_default();
            // Unknown is not off: a host without logind has nothing to warn about.
            let lingers = run("loginctl", &["show-user", &user, "--property=Linger"])
                .await
                .map(|out| out.trim() == "Linger=yes")
                .unwrap_or(true);
            if !lingers {
                tracing::warn!(
                    "lingering is off, so the service stops when you log out and does not start \
                     at boot; `sudo loginctl enable-linger \"$(id -un)\"` keeps it running"
                );
            }
        }
    }
}

async fn run(program: &str, args: &[&str]) -> Result<String> {
    let out = Tool::new(program)
        .args(args)
        .timeout(COMMAND_TIMEOUT)
        .success()
        .await?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn render_plist(spec: &Spec) -> String {
    let string = |v: &str| format!("<string>{}</string>", xml(v));
    let program: String = spec
        .program
        .iter()
        .map(|arg| format!("    {}\n", string(arg)))
        .collect();
    let env: String = spec
        .env
        .iter()
        .map(|(k, v)| format!("    <key>{}</key>\n    {}\n", xml(k), string(v)))
        .collect();
    let log = string(&spec.log.display().to_string());
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>
{program}  </array>
  <key>EnvironmentVariables</key>
  <dict>
{env}  </dict>
  <key>WorkingDirectory</key>
  {root}
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <dict>
    <key>SuccessfulExit</key>
    <false/>
  </dict>
  <key>ThrottleInterval</key>
  <integer>10</integer>
  <key>ExitTimeOut</key>
  <integer>{EXIT_TIMEOUT_SECS}</integer>
  <key>StandardOutPath</key>
  {log}
  <key>StandardErrorPath</key>
  {log}
</dict>
</plist>
"#,
        root = string(&spec.root.display().to_string()),
    )
}

fn plist_root(text: &str) -> Option<PathBuf> {
    let after = text.split_once("<key>CENTINEL_ROOT</key>")?.1;
    let value = after.split_once("<string>")?.1.split_once("</string>")?.0;
    let value = value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&");
    Some(PathBuf::from(value))
}

/// A double-quoted systemd word. `%` is a specifier everywhere; `$` expands only in
/// `ExecStart`, so `dollar` says whether to escape it.
fn quoted(value: &str, dollar: bool) -> String {
    let mut out = String::from('"');
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '%' => out.push_str("%%"),
            '$' if dollar => out.push_str("$$"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn render_unit(spec: &Spec) -> String {
    let exec: Vec<String> = spec.program.iter().map(|a| quoted(a, true)).collect();
    let env: String = spec
        .env
        .iter()
        .map(|(k, v)| format!("Environment={}\n", quoted(&format!("{k}={v}"), false)))
        .collect();
    let path = |p: &Path| p.display().to_string().replace('%', "%%");
    format!(
        "[Unit]\n\
         Description=Centinel server\n\
         # Retry for as long as it takes: at boot, Tailscale may come up after this does.\n\
         StartLimitIntervalSec=0\n\
         \n\
         [Service]\n\
         Type=simple\n\
         WorkingDirectory={root}\n\
         {env}\
         ExecStart={exec}\n\
         Restart=on-failure\n\
         RestartSec=10\n\
         TimeoutStopSec={EXIT_TIMEOUT_SECS}\n\
         StandardOutput=append:{log}\n\
         StandardError=append:{log}\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        root = path(&spec.root),
        exec = exec.join(" "),
        log = path(&spec.log),
    )
}

fn unit_root(text: &str) -> Option<PathBuf> {
    let line = text
        .lines()
        .find_map(|l| l.strip_prefix("Environment=\"CENTINEL_ROOT="))?;
    let value = line.strip_suffix('"')?;
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.extend(chars.next()),
            '%' => {
                chars.next();
                out.push('%');
            }
            c => out.push(c),
        }
    }
    Some(PathBuf::from(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `status`, `stop` and `restart` find the store by reading back the unit `start`
    /// wrote, so a root that does not survive the round trip would point them at the
    /// wrong corpus — or at none.
    #[test]
    fn the_installed_root_reads_back_from_either_unit() {
        let root = PathBuf::from("/srv/my corpus/100% \"gov\" & <more>\\x");
        let spec = Spec {
            program: vec!["/bin/centinel".into(), "serve".into(), "--tailscale".into()],
            env: vec![
                ("CENTINEL_ROOT".into(), root.display().to_string()),
                ("PATH".into(), "/usr/bin:/bin".into()),
            ],
            log: root.join("serve.log"),
            root: root.clone(),
        };
        assert_eq!(plist_root(&render_plist(&spec)), Some(root.clone()));
        assert_eq!(unit_root(&render_unit(&spec)), Some(root));
    }

    #[test]
    fn the_record_offers_the_tailnet_first() {
        let running = Running::new(
            "0.0.0.0:8787".parse().unwrap(),
            Some("https://box.ts.net".into()),
        );
        let text = running.render(Path::new("/srv/serve.log"));
        assert!(text.starts_with("web    https://box.ts.net/web\nmcp    https://box.ts.net/mcp\n"));
        assert!(text.contains("local  http://127.0.0.1:8787\n"), "{text}");
    }
}
