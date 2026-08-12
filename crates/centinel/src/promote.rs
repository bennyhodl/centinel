//! The question `centinel investigate` leaves you with, asked instead of printed.
//!
//! An investigation ends with `centinel source add agartha --site https://www.agartha.gov/
//! --strategy=sitemap`, and the operator's next act is to select that line and run it. This
//! module offers to run it for them: one `y`, or `-y` for somebody who decided in advance.
//!
//! ## Above the op, never inside it
//!
//! The second member of the interactive layer [`crate::wizard`] describes, and held to the
//! same rule for the same reason: **the op never prompts**. One that did would block an MCP
//! call until the client timed out and hang a cron job forever with no output explaining
//! why. So `investigate` still writes nothing and still reads no config — it hands back a
//! [`Promote`], which is what `source add` takes — and everything interactive happens out
//! here, where there is a terminal to be sure of.
//!
//! The rule that follows: no terminal, no offer. `centinel investigate … | tee log` and a
//! scheduled investigation both fall through to the printed command line, which is why that
//! line is still printed.
//!
//! ## It runs the op, not a shell
//!
//! Saying yes invokes the `source` op through the registry, with the JSON every other
//! surface sends. Nothing here shells out to the string the report printed, and nothing here
//! writes TOML: the two mistakes that file invites — a duplicate id and an id that is not a
//! legal directory name — are already refused in one place, and this is not a second one.

use std::io::{IsTerminal, Write};
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use centinel_core::config::Config;
use centinel_core::op::{self, Ctx};
use centinel_core::ops::Promote;
use centinel_core::render::Painter;
use dialoguer::Confirm;
use dialoguer::theme::ColorfulTheme;
use serde_json::{Value, json};

use crate::{Output, logging};

/// Offers to add what an investigation found, and adds it if told to.
///
/// A no-op for every other op, for an investigation that found nothing worth adding, and
/// for a source the config already names.
pub async fn offer(
    ctx: &Arc<Ctx>,
    name: &str,
    report: &Value,
    output: Output,
    assume_yes: bool,
) -> Result<()> {
    if name != "investigate" {
        return Ok(());
    }
    // Read off the serialized report, the same value `render` is handed. Reading the typed
    // struct instead would let this act on a field the other surfaces never see.
    let Some(found) = report.get("promote").filter(|v| !v.is_null()) else {
        return Ok(());
    };
    let promote: Promote = serde_json::from_value(found.clone())
        .context("`investigate` changed the shape of what it says would be added")?;

    // The file `source add` would edit, asked the same way it asks.
    let path = Config::write_path();
    let config = match path.exists() {
        true => Config::from_file(&path)?,
        false => Config::default(),
    };
    if config.source(&promote.id).is_some() {
        // Said rather than asked. The suggested id comes from the host, so investigating a
        // site a second time reaches this every time, and a prompt whose default answer
        // produces an error is worse than no prompt.
        //
        // Said only to somebody who was expecting an offer, though: a piped investigation
        // never had one coming, and a line about an add that is not happening is noise on
        // a stream something else is reading. It says why there was no offer and not that
        // the site is collected — the id could belong to another block entirely, and the
        // command printed above is still there to edit.
        if assume_yes || can_ask(output) {
            eprintln!(
                "  `{}` is already in {} — not offering to add it again",
                promote.id,
                path.display()
            );
        }
        return Ok(());
    }

    match assume_yes || ask(&promote, &path, output) {
        true => add(ctx, &promote, output).await,
        false => Ok(()),
    }
}

/// Whether there is somebody there to ask.
///
/// Both streams have to be terminals — the prompt is drawn on stderr and read from stdin —
/// so a pipe on either end falls through rather than blocking on an answer nobody can give.
/// `--json` never asks either: its caller is a program, and `-y` is how a program says yes.
fn can_ask(output: Output) -> bool {
    !output.json && std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

/// Whether the operator says yes.
///
/// The default is yes. This writes one `[[source]]` block, collects nothing, and is undone
/// by `centinel source remove`; the expensive decision is `centinel run`, which is still a
/// separate command you type yourself.
fn ask(promote: &Promote, path: &Path, output: Output) -> bool {
    if !can_ask(output) {
        return false;
    }

    Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt(format!("Add `{}` to {}?", promote.id, path.display()))
        .default(true)
        // A terminal that stopped answering, or ctrl-C. Not adding is the safe reading of
        // both, and neither is worth an error over a question.
        .interact()
        .unwrap_or(false)
}

/// Runs `source add` with what the investigation found.
async fn add(ctx: &Arc<Ctx>, promote: &Promote, output: Output) -> Result<()> {
    let def = op::find("source").context("the `source` op is not registered")?;
    let value = logging::invoke("cli", def, Arc::clone(ctx), add_args(promote), None).await?;

    if output.json {
        // stdout is carrying the investigation as one JSON document and must stay one, so
        // the outcome goes where progress goes.
        eprintln!(
            "  added `{}` to {}",
            promote.id,
            value
                .get("config")
                .and_then(Value::as_str)
                .unwrap_or("the config")
        );
        return Ok(());
    }

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

/// The arguments the `source` op is invoked with — the shape every surface sends.
///
/// `run` is deliberately absent: adding is not collecting, and a command that fetched an
/// hour of a city because somebody pressed `y` at a prompt about a config file would be the
/// worst kind of surprise.
fn add_args(promote: &Promote) -> Value {
    json!({
        "action": {
            "action": "add",
            "id": promote.id,
            "site": promote.site,
            "strategy": promote.strategy,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use centinel_core::ops::{SourceAction, SourceArgs};

    /// The shape is a contract between two ops and nothing type-checks it, so it is pinned
    /// here: a rename on either side has to fail a test rather than a keystroke.
    #[test]
    fn the_arguments_are_the_ones_the_source_op_takes() {
        let promote = Promote {
            id: "agartha".into(),
            site: "https://www.agartha.gov/".into(),
            strategy: Some("sitemap".into()),
        };

        let args: SourceArgs = serde_json::from_value(add_args(&promote))
            .expect("`source add` no longer accepts what the offer sends");
        match args.action {
            SourceAction::Add(add) => {
                assert_eq!(add.id, "agartha");
                assert_eq!(add.site.as_deref(), Some("https://www.agartha.gov/"));
                assert_eq!(add.strategy.as_deref(), Some("sitemap"));
                assert!(!add.run, "adding must not collect");
                assert!(!add.disabled);
            }
            _ => panic!("wrong action"),
        }
    }

    /// A fallback walk is a guess, and `source add` must not be told it was evidence.
    #[test]
    fn an_unrecognised_address_is_added_without_a_pinned_strategy() {
        let args: SourceArgs = serde_json::from_value(add_args(&Promote {
            id: "agartha".into(),
            site: "https://www.agartha.gov/".into(),
            strategy: None,
        }))
        .unwrap();
        match args.action {
            SourceAction::Add(add) => assert!(add.strategy.is_none()),
            _ => panic!("wrong action"),
        }
    }

    /// What the offer reads is the serialized report, so the field it looks for has to be
    /// the field `investigate` writes.
    #[test]
    fn the_promotion_is_read_off_the_report_the_other_surfaces_receive() {
        let report = centinel_core::ops::InvestigateReport {
            address: "https://www.agartha.gov/".into(),
            seed: centinel_core::ops::SeedSummary {
                http_status: Some("200 OK".into()),
                bytes: 1,
                kind: "html".into(),
                final_url: None,
                robots_declared: true,
            },
            recognised: Vec::new(),
            probe: None,
            lead: None,
            crumbs: Vec::new(),
            promote: Some(Promote {
                id: "agartha".into(),
                site: "https://www.agartha.gov/".into(),
                strategy: None,
            }),
            warnings: Vec::new(),
            elapsed_secs: 0.1,
        };

        let value = serde_json::to_value(&report).unwrap();
        let found: Promote = serde_json::from_value(value["promote"].clone()).unwrap();
        assert_eq!(found.id, "agartha");
    }
}
