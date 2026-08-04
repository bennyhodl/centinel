//! The `centinel` binary.
//!
//! Three surfaces, one registry. Nothing in this crate names an individual op — the
//! CLI's subcommands, the MCP tool list and the HTTP routes are all built by iterating
//! [`centinel_core::op::all`]. Adding an op in the library makes it appear in all three
//! without touching this file, which is the property ticket #9 was about.

mod http;
mod mcp;
mod progress;

use std::io::{IsTerminal, Write};
use std::sync::Arc;

use anyhow::{Context, Result};
use centinel_core::op::{self, Ctx, Group, Progress};
use centinel_core::render::{DEFAULT_WIDTH, Painter};
use centinel_core::store::Store;
use clap::{Arg, ArgAction, Command};

/// Commands that are not ops, listed under their own heading.
///
/// They take no [`op::OpDef`] because they are not one definition serving three surfaces
/// — they *are* two of the surfaces.
const SERVER_COMMANDS: [(&str, &str); 2] = [
    ("serve", "Run the HTTP server (ops as routes, plus MCP over HTTP)"),
    ("mcp", "Run an MCP server over stdio"),
];

/// Store root, relative to the working directory unless `--root`/`CENTINEL_ROOT` says
/// otherwise. A local default keeps the corpus next to the work, matching SPEC §5.4's
/// "`rsync`-able and complete on its own".
const DEFAULT_ROOT: &str = ".centinel";

fn build_cli() -> Command {
    let mut cmd = Command::new("centinel")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Data collection for .gov web surfaces and YouTube channels")
        .long_about(
            "Collects, versions and (eventually) searches government web content.\n\
             Files on disk are the source of truth; every index is derived and rebuildable.",
        )
        .arg(
            Arg::new("root")
                .long("root")
                .global(true)
                .env("CENTINEL_ROOT")
                .default_value(DEFAULT_ROOT)
                .value_name("DIR")
                .help("Store root"),
        )
        .arg(
            Arg::new("verbose")
                .long("verbose")
                .short('v')
                .global(true)
                .action(ArgAction::SetTrue)
                .help("Log to stderr"),
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

    let root = matches
        .get_one::<String>("root")
        .expect("root has a default")
        .clone();
    let verbose = matches.get_flag("verbose");

    // Always stderr. Under `centinel mcp`, stdout carries JSON-RPC frames and a stray
    // log line would corrupt the protocol stream.
    if verbose {
        tracing_subscriber::fmt()
            .with_writer(std::io::stderr)
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "centinel=debug,centinel_core=debug".into()),
            )
            .init();
    }

    let store = Store::open(&root)
        .await
        .with_context(|| format!("opening store at {root}"))?;
    let ctx = Arc::new(Ctx::new(store));

    let (name, sub) = matches
        .subcommand()
        .expect("subcommand_required guarantees one");

    match name {
        "serve" => {
            let bind = sub
                .get_one::<String>("bind")
                .expect("bind has a default")
                .clone();
            http::serve(ctx, &bind).await
        }
        "mcp" => mcp::serve(ctx).await,
        op_name => run_op(ctx, op_name, sub, Output::detect(sub)).await,
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
    let args = (def.args_from_matches)(matches)?;

    // Progress goes to stderr so stdout stays a clean JSON stream for piping. Which
    // renderer draws it — bars or lines — is [`progress`]'s decision, not this one's.
    let (progress, rx) = if def.long_running {
        let (p, rx) = Progress::channel();
        (p, Some(rx))
    } else {
        (Progress::none(), None)
    };

    let printer = rx.map(progress::spawn);

    let result = (def.invoke)(ctx, args, progress).await;

    if let Some(handle) = printer {
        // The sink was dropped with `progress`, so the printer terminates on its own.
        let _ = handle.await;
    }

    let value = result?;

    if output.json {
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
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
    Ok(())
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
