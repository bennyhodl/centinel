//! The `centinel` binary.
//!
//! Three surfaces, one registry. Nothing in this crate names an individual op — the
//! CLI's subcommands, the MCP tool list and the HTTP routes are all built by iterating
//! [`centinel_core::op::all`]. Adding an op in the library makes it appear in all three
//! without touching this file, which is the property ticket #9 was about.

mod http;
mod logging;
mod mcp;
mod progress;
mod promote;
mod schedule;
mod wizard;

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use centinel_core::config::{self, Config};
use centinel_core::op::{self, Ctx, Group, Progress};
use centinel_core::render::{DEFAULT_WIDTH, Painter};
use centinel_core::store::Store;
use clap::{Arg, ArgAction, Command};

/// Commands that are not ops, listed under their own heading.
///
/// They take no [`op::OpDef`] because they are not one definition serving three surfaces
/// — they *are* two of the surfaces.
const SERVER_COMMANDS: [(&str, &str); 2] = [
    (
        "serve",
        "Run the HTTP server (ops as routes, plus MCP over HTTP)",
    ),
    ("mcp", "Run an MCP server over stdio"),
];

fn build_cli() -> Command {
    let mut cmd = Command::new("centinel")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Data collection for .gov web surfaces and YouTube channels")
        .long_about(
            "Collects, versions and (eventually) searches government web content.\n\
             Files on disk are the source of truth; every index is derived and rebuildable.",
        )
        .arg(
            // No `default_value`: absent has to be distinguishable from typed, because
            // the config file answers in between. See [`resolve_root`].
            Arg::new("root")
                .long("root")
                .global(true)
                .env("CENTINEL_ROOT")
                .value_name("DIR")
                .help("Store root [default: `root` in centinel.toml, else ~/.centinel]"),
        )
        .arg(
            Arg::new("verbose")
                .long("verbose")
                .short('v')
                .global(true)
                .action(ArgAction::SetTrue)
                .help("Log debug detail to stderr (`serve` and `mcp` log at info without it)"),
        )
        .arg(
            Arg::new("json")
                .long("json")
                .global(true)
                .action(ArgAction::SetTrue)
                .conflicts_with("pretty")
                .help("Emit the raw report as JSON (the default when stdout is not a terminal)"),
        )
        .arg(
            Arg::new("pretty")
                .long("pretty")
                .global(true)
                .action(ArgAction::SetTrue)
                .help("Render the report for a human (the default when stdout is a terminal)"),
        )
        .arg(
            // The answer to a question, not an instruction to an op: nothing downstream of
            // here sees it, and every prompt it answers is drawn by this crate. See
            // [`promote`] and [`wizard`], which is where the rule that ops never ask lives.
            Arg::new("yes")
                .long("yes")
                .short('y')
                .global(true)
                .action(ArgAction::SetTrue)
                .help("Answer yes to any confirmation instead of asking (e.g. `investigate` offering to add a source)"),
        )
        .arg(
            Arg::new("color")
                .long("color")
                .global(true)
                .value_name("WHEN")
                .value_parser(["auto", "always", "never"])
                .default_value("auto")
                .help("When to colourise: auto, always, never"),
        )
        .arg(
            // Honoured for its presence, per the no-color.org convention: any non-empty
            // value means no colour. Kept as a separate hidden arg rather than as `--color`'s
            // env source, because an env var must not beat an explicit `--color always`.
            Arg::new("no-color-env")
                .long("no-color")
                .global(true)
                .env("NO_COLOR")
                .action(ArgAction::SetTrue)
                .hide(true)
                .help("Render without colour"),
        )
        .subcommand_required(true)
        .arg_required_else_help(true)
        // Subcommands are hidden from clap's own flat list and re-listed by
        // [`command_overview`] under headings. `hide` affects only *this* command's
        // help: `centinel run --help` is unchanged, and so is parsing.
        //
        // Two consequences have to be undone by hand. clap drops `<COMMAND>` from the
        // usage line when every subcommand is hidden, which would tell a new reader the
        // binary takes no command at all; and the default template puts `{after-help}`
        // below the options, burying the command list under six flags nobody needs
        // before they have chosen one.
        .override_usage("centinel [OPTIONS] <COMMAND>")
        // `{usage}` brings its own blank line after it, so the command list follows it
        // directly. This template applies to the root only — clap does not propagate it,
        // so `centinel run --help` keeps the standard layout.
        .help_template(
            "{about-with-newline}\n{usage-heading} {usage}{after-help}\nOptions:\n{options}",
        )
        .after_help(command_overview());

    // Every registered op becomes a subcommand. No list to maintain.
    for def in op::all() {
        let sub = Command::new(def.name).about(def.about).hide(true);
        cmd = cmd.subcommand((def.augment_clap)(sub));
    }

    cmd.subcommand(
        Command::new("serve")
            .about(SERVER_COMMANDS[0].1)
            .hide(true)
            .arg(
                Arg::new("bind")
                    .long("bind")
                    .default_value("127.0.0.1:8787")
                    .value_name("ADDR"),
            )
            .arg(
                // For a machine that serves a corpus somebody else collects.
                Arg::new("no-schedule")
                    .long("no-schedule")
                    .action(ArgAction::SetTrue)
                    .help("Serve the read API without firing any [[schedule]]"),
            )
            .arg(
                Arg::new("config")
                    .long("config")
                    .value_name("FILE")
                    .help("Config file the schedules are read from"),
            ),
    )
    .subcommand(Command::new("mcp").about(SERVER_COMMANDS[1].1).hide(true))
}

/// The command list, grouped by [`Group`].
///
/// Sixteen verbs in one alphabetical column make `collect`, `embed` and `doctor` look
/// like peer choices, when the first two are stages of what `run` does for you and the
/// third is a health check. The headings say which is which; the opening two lines say
/// what to type when none of it means anything yet.
///
/// Built by iterating the registry, so a new op appears here for the same reason it
/// appears in `tools/list` — because it exists, not because someone remembered.
fn command_overview() -> String {
    let mut out = String::from(
        "Getting started:\n  \
         centinel source add tampa --site https://www.tampa.gov\n  \
         centinel run\n",
    );

    // One column width across every group, so the descriptions line up down the whole
    // list rather than stepping in and out per section.
    let width = op::all()
        .iter()
        .map(|d| d.name.len())
        .chain(SERVER_COMMANDS.iter().map(|(n, _)| n.len()))
        .max()
        .unwrap_or(8)
        .max(8);

    for group in Group::ORDER {
        let ops = op::in_group(group);
        if ops.is_empty() {
            continue;
        }
        out.push_str(&format!("\n{}:\n", group.heading()));
        for def in ops {
            out.push_str(&format!("  {:width$}  {}\n", def.name, def.about));
        }
    }

    out.push_str("\nServer:\n");
    for (name, about) in SERVER_COMMANDS {
        out.push_str(&format!("  {name:width$}  {about}\n"));
    }
    out
}

#[tokio::main]
async fn main() -> Result<()> {
    let matches = build_cli().get_matches();

    let (name, sub) = matches
        .subcommand()
        .expect("subcommand_required guarantees one");

    // Installed before the store opens, so that opening it is inside the log rather than
    // the first thing missing from it. Which levels reach stderr is [`logging`]'s
    // decision — it is the one that knows a server has no other way to speak.
    logging::install(
        name,
        matches.get_flag("verbose"),
        matches.get_flag("no-color-env"),
    );

    let root = resolve_root(&matches)?;
    let store = Store::open(&root)
        .await
        .with_context(|| format!("opening store at {}", root.display()))?;
    let ctx = Arc::new(Ctx::new(store));

    match name {
        "serve" => {
            let bind = sub
                .get_one::<String>("bind")
                .expect("bind has a default")
                .clone();
            serve(ctx, &bind, sub).await
        }
        "mcp" => mcp::serve(ctx).await,
        op_name => run_op(ctx, op_name, sub, Output::detect(sub)).await,
    }
}

/// Serves the read API, and — unless told not to — fires the configured schedules.
///
/// The two are deliberately separate concerns sharing one process: the server *reports* on
/// the record and never causes it to grow, while the scheduler executes instructions the
/// operator wrote into `centinel.toml`. Nothing arriving on the socket can reach the
/// second (`op::Reach`), which is the whole of `docs/SCHEDULING.md` §1.1.
///
/// **A broken schedule refuses to start the whole command.** A server that came up happily
/// and collected nothing would say so nowhere, and the operator would find out weeks later
/// from an empty search result. This is loud at the one moment it is cheap.
async fn serve(ctx: Arc<Ctx>, bind: &str, matches: &clap::ArgMatches) -> Result<()> {
    if matches.get_flag("no-schedule") {
        tracing::info!("scheduler disabled by --no-schedule");
        return http::serve(ctx, bind).await;
    }

    let config = matches.get_one::<String>("config").map(String::as_str);
    let scheduler = schedule::Scheduler::new(Arc::clone(&ctx), config)?;
    let count = scheduler.schedules().len();

    let (reload_tx, reload_rx) = schedule::ReloadSignal::channel();
    let (canceller, thread) = schedule::spawn(scheduler, reload_rx)?;

    if count == 0 {
        eprintln!("  no schedules configured — centinel schedule set");
    } else {
        eprintln!("  {count} schedule(s) armed — centinel schedules");
    }
    install_reload_handler(reload_tx);

    let served = http::serve_until(ctx, bind, terminate()).await;

    // The socket is closed, so the in-flight run is asked to stop at its next item
    // boundary and the scheduler is given time to write its `interrupted` record. Nothing
    // is lost by stopping there: every stage computes its work list as a subtraction, so
    // the next fire resumes from what the log says.
    tracing::info!("stopping the scheduler");
    canceller.cancel();
    if let Err(e) = thread.join() {
        tracing::warn!("the scheduler thread panicked: {e:?}");
    }
    served
}

/// Resolves on `SIGTERM` or `SIGINT`.
///
/// Both, because they arrive from different places and mean the same thing here: a
/// container stopping and somebody pressing ctrl-C both want the run journal to end with a
/// record rather than with a stale lock.
#[cfg(unix)]
async fn terminate() {
    use tokio::signal::unix::{SignalKind, signal};
    let mut term = match signal(SignalKind::terminate()) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, "no SIGTERM handler; shutdown will not be graceful");
            return std::future::pending().await;
        }
    };
    tokio::select! {
        _ = term.recv() => tracing::info!("SIGTERM"),
        _ = tokio::signal::ctrl_c() => tracing::info!("interrupted"),
    }
}

#[cfg(not(unix))]
async fn terminate() {
    let _ = tokio::signal::ctrl_c().await;
}

/// Re-reads the config on `SIGHUP`, so `schedule set` against a live server is not a
/// restart.
///
/// A reload that does not validate keeps the running schedule (see `Scheduler::reload`): a
/// restart is always correct and always sufficient, and a typo must not disarm a server
/// that was collecting correctly.
#[cfg(unix)]
fn install_reload_handler(tx: tokio::sync::mpsc::Sender<()>) {
    tokio::spawn(async move {
        let mut hup = match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup()) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "SIGHUP reload unavailable; restart to pick up edits");
                return;
            }
        };
        while hup.recv().await.is_some() {
            tracing::info!("SIGHUP: re-reading the config");
            // A full queue means a reload is already pending, which is the same outcome.
            let _ = tx.try_send(());
        }
    });
}

#[cfg(not(unix))]
fn install_reload_handler(_tx: tokio::sync::mpsc::Sender<()>) {}

/// The store root in effect, nearest answer first.
///
/// 1. `--root`, or `$CENTINEL_ROOT` — clap reads the variable into the same argument, so
///    both arrive here as a path somebody typed;
/// 2. `root` in the config file — the standing preference;
/// 3. `~/.centinel` — see [`config::default_root`].
///
/// The config consulted is whichever one this invocation named, so `centinel run --config
/// other.toml` collects that file's sources into that file's store. Reading the sources
/// from one config and the root from another would put a corpus somewhere nothing else
/// would look for it.
///
/// A config that does not parse is an error here rather than a fall back to the default:
/// the alternative is collecting into the wrong store and saying nothing about why.
fn resolve_root(matches: &clap::ArgMatches) -> Result<PathBuf> {
    if let Some(typed) = matches.get_one::<String>("root") {
        return Ok(config::expand_tilde(typed));
    }
    let config = match explicit_config(matches) {
        Some(path) => Config::from_file(Path::new(path))?,
        None => Config::load()?,
    };
    Ok(config.store_root())
}

/// A `--config` typed anywhere in the subcommand chain.
///
/// Walked rather than read off one level, because `--config` is an op's own argument and
/// an op can nest: it sits on `run`, but on `source `**`list`** — one deeper. Reading only
/// the first level found it for `run` and silently missed it for every `source` action,
/// which is the whole of the bug this walk exists for.
fn explicit_config(matches: &clap::ArgMatches) -> Option<&String> {
    let mut level = matches;
    loop {
        if let Ok(found @ Some(_)) = level.try_get_one::<String>("config") {
            return found;
        }
        level = level.subcommand()?.1;
    }
}

/// What stdout should receive.
///
/// The default is decided by the destination, not by a flag: a person gets prose, a pipe
/// gets JSON. That keeps `centinel list | jq` working exactly as it did — the guarantee
/// this binary has always made — while ending the practice of printing a serialization
/// format at a human who asked a question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Output {
    json: bool,
    color: bool,
    width: usize,
}

impl Output {
    fn detect(matches: &clap::ArgMatches) -> Self {
        let tty = std::io::stdout().is_terminal();

        // Format and colour are decided separately, because `--pretty | less -R` is a
        // real thing to want and bundling them would make it unreachable.
        let json = match (matches.get_flag("json"), matches.get_flag("pretty")) {
            (true, _) => true,
            (_, true) => false,
            _ => !tty,
        };

        let color = match matches
            .get_one::<String>("color")
            .map(String::as_str)
            .unwrap_or("auto")
        {
            "always" => true,
            "never" => false,
            // `NO_COLOR` loses to an explicit `--color always` and wins over everything else.
            _ => tty && !matches.get_flag("no-color-env"),
        };

        Self {
            json,
            color,
            width: terminal_width(),
        }
    }
}

/// The usable width, or [`DEFAULT_WIDTH`] when there is no terminal to ask.
fn terminal_width() -> usize {
    console::Term::stdout()
        .size_checked()
        .map(|(_, cols)| cols as usize)
        .unwrap_or(DEFAULT_WIDTH as u16 as usize)
}

/// Runs one op from the CLI.
///
/// CLI arguments are converted to the same JSON the HTTP and MCP surfaces send, rather
/// than being passed as a struct. That keeps the three paths genuinely identical — a
/// divergence surfaces as a deserialize failure instead of as quietly different behaviour.
///
/// The *result* takes the same route: rendering reads the erased JSON value the other two
/// surfaces receive, so a terminal can never be shown a field HTTP would not return.
async fn run_op(
    ctx: Arc<Ctx>,
    name: &str,
    matches: &clap::ArgMatches,
    output: Output,
) -> Result<()> {
    let def = op::find(name).with_context(|| format!("unknown op `{name}`"))?;
    let mut args = (def.args_from_matches)(matches)?;
    let assume_yes = matches.get_flag("yes");

    // The interactive layer sits *above* the op and fills in what was not typed. The op
    // itself never prompts: one that did would block an MCP call until the client timed
    // out and hang a script forever. See [`wizard`].
    if wizard::should_prompt(name, &args) {
        args = wizard::schedule_set(&ctx, args, assume_yes).await?;
    }

    // Progress goes to stderr so stdout stays a clean JSON stream for piping. Which
    // renderer draws it — bars or lines — is [`progress`]'s decision, not this one's.
    let (progress, rx) = if def.long_running {
        let (p, rx) = Progress::channel();
        (p, Some(rx))
    } else {
        (Progress::none(), None)
    };

    let printer = rx.map(progress::spawn);

    let result = logging::invoke("cli", def, Arc::clone(&ctx), args, Some(progress)).await;

    if let Some(handle) = printer {
        // The sink was dropped with `progress`, so the printer terminates on its own.
        let _ = handle.await;
    }

    let value = result?;

    if output.json {
        println!("{}", serde_json::to_string_pretty(&value)?);
        return promote::offer(&ctx, name, &value, output, assume_yes).await;
    }

    // Rendered through a lock and flushed once: a report is one screen of output and
    // should not interleave with anything the progress renderer is still finishing.
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    writeln!(handle)?;
    {
        let mut painter = Painter::new(&mut handle, output.color, output.width);
        (def.render)(&value, &mut painter)?;
    }
    writeln!(handle)?;
    handle.flush()?;
    drop(handle);

    // After the evidence, not before it: the one question an investigation leaves you
    // with, asked here rather than printed as a line to retype. A no-op for every other
    // op, and for a terminal there is nobody sitting at. See [`promote`].
    promote::offer(&ctx, name, &value, output, assume_yes).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_builds_and_validates() {
        // clap panics on malformed definitions; this is how a bad `#[op]` argument
        // struct gets caught by `cargo test` rather than by a user.
        build_cli().debug_assert();
    }

    /// `--root` must stay *absent* when nobody typed it. A clap default would satisfy
    /// [`resolve_root`]'s first branch on every invocation, so `root` in the config file
    /// would be read, parsed, and never used.
    #[test]
    fn root_is_absent_unless_something_names_it() {
        // clap folds `$CENTINEL_ROOT` into this argument, so the variable being set in
        // the test environment is indistinguishable from `--root` and would fail here.
        if std::env::var_os("CENTINEL_ROOT").is_some() {
            return;
        }
        let m = build_cli()
            .try_get_matches_from(["centinel", "doctor"])
            .unwrap();
        assert_eq!(m.get_one::<String>("root"), None);

        let m = build_cli()
            .try_get_matches_from(["centinel", "doctor", "--root", "/srv/corpus"])
            .unwrap();
        assert_eq!(
            m.get_one::<String>("root").map(String::as_str),
            Some("/srv/corpus")
        );
    }

    /// `-y` answers a prompt this crate draws *after* the op it belongs to, so it has to
    /// parse on the subcommand rather than only ahead of it — `centinel investigate <url>
    /// -y` is where a hand puts it.
    #[test]
    fn yes_is_global_and_arrives_with_the_subcommand() {
        let flag = |argv: &[&str]| {
            let m = build_cli().try_get_matches_from(argv).unwrap();
            m.subcommand().unwrap().1.get_flag("yes")
        };
        assert!(flag(&["centinel", "investigate", "https://x.gov/", "-y"]));
        assert!(flag(&[
            "centinel",
            "--yes",
            "investigate",
            "https://x.gov/"
        ]));
        assert!(!flag(&["centinel", "investigate", "https://x.gov/"]));
    }

    /// The nesting `--config` actually has: on `run` at one level, on `source list` at
    /// two. Reading a fixed level found one and silently missed the other, which put the
    /// store somewhere the named config never asked for.
    #[test]
    fn a_config_typed_at_any_depth_is_found() {
        let at = |argv: &[&str]| {
            let matches = build_cli().try_get_matches_from(argv).unwrap();
            explicit_config(&matches).cloned()
        };
        assert_eq!(
            at(&["centinel", "run", "--config", "a.toml"]).as_deref(),
            Some("a.toml")
        );
        assert_eq!(
            at(&["centinel", "source", "list", "--config", "b.toml"]).as_deref(),
            Some("b.toml")
        );
        assert_eq!(at(&["centinel", "doctor"]), None);
    }

    #[test]
    fn every_registered_op_is_a_subcommand() {
        let cli = build_cli();
        let names: Vec<_> = cli.get_subcommands().map(|s| s.get_name()).collect();
        for def in op::all() {
            assert!(
                names.contains(&def.name),
                "op `{}` is registered but missing from the CLI",
                def.name
            );
        }
        assert!(names.contains(&"serve"));
        assert!(names.contains(&"mcp"));
    }

    /// `run` and `source` are the two commands someone new should see first, so they
    /// have to actually be registered rather than merely written about in the docs.
    #[test]
    fn the_pipeline_group_holds_run_and_source() {
        let names: Vec<_> = op::in_group(Group::Pipeline)
            .iter()
            .map(|d| d.name)
            .collect();
        assert!(names.contains(&"run"), "{names:?}");
        assert!(names.contains(&"source"), "{names:?}");
    }

    /// Hiding the subcommands from clap's own list means this overview is now the only
    /// place they are named. An op missing from it would be invisible to anyone reading
    /// `--help`, while still parsing perfectly — the exact failure the registry exists
    /// to prevent, reintroduced one layer up.
    #[test]
    fn the_overview_lists_every_op_and_every_server_command() {
        let overview = command_overview();
        for def in op::all() {
            assert!(
                overview.contains(def.name),
                "op `{}` is registered but missing from --help",
                def.name
            );
        }
        for (name, _) in SERVER_COMMANDS {
            assert!(overview.contains(name), "`{name}` missing from --help");
        }
    }

    /// Headings are the whole point; a group that silently emptied would take its ops
    /// with it.
    #[test]
    fn every_group_with_ops_gets_a_heading() {
        let overview = command_overview();
        for group in Group::ORDER {
            if op::in_group(group).is_empty() {
                continue;
            }
            assert!(
                overview.contains(&format!("{}:", group.heading())),
                "group `{}` has ops but no heading",
                group.heading()
            );
        }
        // The two commands that make the flat list confusing must lead it.
        let pipeline = overview.find("Pipeline:").expect("pipeline heading");
        let stages = overview.find("Stages:").expect("stages heading");
        assert!(pipeline < stages, "run must be listed above its own stages");
    }

    #[test]
    fn ops_are_actually_registered() {
        // Guards against the failure mode where `inventory` collects nothing because
        // the ops module was optimised out — which would make every surface silently empty.
        let all = op::all();
        assert!(!all.is_empty(), "no ops registered");
        for expected in ["doctor", "ingest", "list"] {
            assert!(op::find(expected).is_some(), "missing op `{expected}`");
        }
    }
}
