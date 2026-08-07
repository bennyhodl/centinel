//! `centinel schedule set` with nothing to type.
//!
//! `docs/SCHEDULING.md` §3.4.
//!
//! ## The op never prompts
//!
//! This is a **CLI-side layer above the op**, not a mode inside it. The op receives a
//! complete argument set from every surface, exactly as it does today.
//!
//! *Why it matters:* an op that prompts blocks an MCP call until the client times out, and
//! hangs a script forever with no output explaining why. It is the same rule `tool.rs`
//! already enforces one level down — every child process Centinel starts is denied our
//! stdin, because a tool that reads it is a tool that can wedge the run.
//!
//! So prompting happens only when stdin **and** stderr are a terminal. With arguments
//! missing and no terminal, the op fails naming what is absent. It never waits.
//!
//! ## What it puts on screen
//!
//! Three things an operator cannot hold in their head:
//!
//! - **the source ids that exist**, with kind and when each was last collected, so a
//!   schedule cannot name a source that is not there and the one you forgot is visible
//!   rather than remembered;
//! - **the cadence in words**, beside the expression that produces it;
//! - **the next three fire times**, in the chosen zone, with jitter applied. This is the
//!   one that earns the feature: `0 3 * * 1` is Mondays and `0 3 1 * *` is the 1st, and
//!   three dates turn that from a guess into a decision.

use std::io::IsTerminal;

use anyhow::{Context, Result};
use centinel_core::config::{Config, STAGE_NAMES};
use centinel_core::op::Ctx;
use centinel_core::schedule::{Cron, jitter_offset};
use centinel_core::store::Store;
use dialoguer::theme::ColorfulTheme;
use dialoguer::{Confirm, Input, MultiSelect, Select};
use serde_json::{Value, json};

/// The cadences offered before "something else".
///
/// Early morning, because a `.gov` site is a business-hours thing and the point of a
/// scheduled crawl is to be finished before anyone is using it.
const PRESETS: [(&str, &str); 4] = [
    ("Daily, early morning", "0 3 * * *"),
    ("Weekly, Sunday early morning", "0 3 * * 0"),
    ("Monthly, the 1st", "0 2 1 * *"),
    ("Twice a day", "0 3,15 * * *"),
];

/// Whether this invocation should be filled in by asking.
///
/// Both streams, and both must be terminals: prompts are drawn on stderr and read from
/// stdin, so `centinel schedule set < /dev/null` and `… | tee log` must both fall through
/// to the op's own error rather than block.
pub fn should_prompt(name: &str, args: &Value) -> bool {
    if name != "schedule" {
        return false;
    }
    let Some(set) = set_args(args) else {
        return false;
    };
    // Anything already typed is an instruction, and a half-answered command line is more
    // likely a mistake than a request to be interviewed about the rest.
    let bare =
        set.get("id").is_none_or(Value::is_null) && set.get("cron").is_none_or(Value::is_null);

    bare && std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

/// The `set` action's own arguments, which sit one level down.
///
/// `ScheduleArgs` wraps a subcommand enum, so the fields are under `action`, beside the
/// tag that names which action it is — not at the top level. Reading them from the top
/// silently found nothing, which is a gate that never opens rather than one that misfires:
/// the wizard simply never ran, and the op's "give one, or run this on a terminal" error
/// was the only thing anyone ever saw.
fn set_args(args: &Value) -> Option<&serde_json::Map<String, Value>> {
    let action = args.get("action")?.as_object()?;
    (action.get("action").and_then(Value::as_str) == Some("set")).then_some(action)
}

/// Asks, and returns the argument set the op would have been given.
pub async fn schedule_set(ctx: &Ctx, mut args: Value) -> Result<Value> {
    let theme = ColorfulTheme::default();

    let config_path = set_args(&args)
        .and_then(|a| a.get("config"))
        .and_then(Value::as_str)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(Config::write_path);
    let config = if config_path.exists() {
        Config::from_file(&config_path)?
    } else {
        Config::default()
    };

    eprintln!();
    eprintln!("Writing a schedule to {}", config_path.display());
    eprintln!();

    // ── id ────────────────────────────────────────────────────────────────────
    let taken: Vec<String> = config.schedules.iter().map(|s| s.id.clone()).collect();
    let id: String = Input::with_theme(&theme)
        .with_prompt("Schedule id")
        .validate_with(|input: &String| -> Result<(), String> {
            let value = input.trim();
            if value.is_empty() {
                return Err("an id names this schedule in every report".into());
            }
            if taken.iter().any(|t| t == value) {
                return Err(format!("`{value}` is already in this file"));
            }
            Ok(())
        })
        .interact_text()?;
    let id = id.trim().to_string();

    // ── sources ───────────────────────────────────────────────────────────────
    let sources = choose_sources(&theme, &config, &ctx.store).await?;

    // ── cadence ───────────────────────────────────────────────────────────────
    let cron = choose_cadence(&theme)?;

    // ── zone ──────────────────────────────────────────────────────────────────
    let host = jiff::tz::TimeZone::system()
        .iana_name()
        .unwrap_or("UTC")
        .to_string();
    let tz: String = Input::with_theme(&theme)
        .with_prompt("Time zone")
        .default(host)
        .validate_with(|input: &String| -> Result<(), String> {
            jiff::tz::TimeZone::get(input.trim())
                .map(|_| ())
                .map_err(|_| format!("`{}` is not an IANA zone name", input.trim()))
        })
        .interact_text()?;
    let tz = tz.trim().to_string();

    // ── stages ────────────────────────────────────────────────────────────────
    let skipped = MultiSelect::with_theme(&theme)
        .with_prompt("Skip any stages? (space to select, enter for none)")
        .items(STAGE_NAMES)
        .interact()?;
    let skip: Vec<String> = skipped
        .into_iter()
        .map(|i| STAGE_NAMES[i].to_string())
        .collect();

    // ── the preview that earns the feature ────────────────────────────────────
    preview(ctx, &id, &cron, &tz)?;

    if !Confirm::with_theme(&theme)
        .with_prompt(format!("Write this to {}?", config_path.display()))
        .default(true)
        .interact()?
    {
        anyhow::bail!("nothing was written");
    }

    // Written back into the action, where the op will look for them.
    let action = args
        .get_mut("action")
        .and_then(Value::as_object_mut)
        .context("`schedule set` arguments changed shape")?;
    action.insert("id".into(), json!(id));
    action.insert("cron".into(), json!(cron));
    action.insert("tz".into(), json!(tz));
    action.insert("sources".into(), json!(sources));
    action.insert("skip".into(), json!(skip));
    action.insert("config".into(), json!(config_path.display().to_string()));
    Ok(args)
}

/// The source picker, showing what the store already holds for each.
///
/// The counts come from the log rather than the index, so this is right on a store whose
/// `centinel.db` has been deleted — and "never collected" is a real and useful answer for
/// a source added five minutes ago.
async fn choose_sources(
    theme: &ColorfulTheme,
    config: &Config,
    store: &Store,
) -> Result<Vec<String>> {
    if config.sources.is_empty() {
        eprintln!("  no [[source]] blocks yet — this schedule will run whatever you add");
        return Ok(Vec::new());
    }

    let mut labels = Vec::with_capacity(config.sources.len());
    for source in &config.sources {
        let kind = match source.acquisition() {
            Ok(a) => a.kind().to_string(),
            Err(_) => "?".to_string(),
        };
        let held = describe_holdings(store, &source.id).await;
        labels.push(format!("{:<20} {:<9} {held}", source.id, kind));
    }

    let chosen = MultiSelect::with_theme(theme)
        .with_prompt("Which sources? (space to select, enter for all)")
        .items(&labels)
        .interact()?;

    Ok(chosen
        .into_iter()
        .map(|i| config.sources[i].id.clone())
        .collect())
}

/// "1,005 addresses · collected 2026-08-05", or why not.
async fn describe_holdings(store: &Store, id: &str) -> String {
    let Ok(source_id) = centinel_core::domain::SourceId::new(id.to_string()) else {
        return "not a legal id".into();
    };
    let Ok(replay) = store.replay(&source_id).await else {
        return "never collected".into();
    };

    let statuses = replay.statuses();
    if statuses.is_empty() {
        return "never collected".into();
    }
    let latest = replay
        .latest_observations()
        .values()
        .map(|obs| obs.at)
        .max();

    match latest {
        Some(at) => format!(
            "{} addresses · collected {}",
            statuses.len(),
            at.to_zoned(jiff::tz::TimeZone::system())
                .strftime("%Y-%m-%d")
        ),
        None => format!("{} addresses · never collected", statuses.len()),
    }
}

/// The preset list, then a free-text expression validated as it is typed.
fn choose_cadence(theme: &ColorfulTheme) -> Result<String> {
    let mut items: Vec<String> = PRESETS
        .iter()
        .map(|(label, expr)| format!("{label:<32} {expr}"))
        .collect();
    items.push("Custom cron expression …".into());

    let chosen = Select::with_theme(theme)
        .with_prompt("How often?")
        .items(&items)
        .default(0)
        .interact()?;

    if chosen < PRESETS.len() {
        return Ok(PRESETS[chosen].1.to_string());
    }

    let typed: String = Input::with_theme(theme)
        .with_prompt("Cron expression (minute hour day-of-month month day-of-week)")
        .validate_with(|input: &String| -> Result<(), String> {
            Cron::parse(input.trim()).map(|_| ()).map_err(|e| e.reason)
        })
        .interact_text()?;
    Ok(typed.trim().to_string())
}

/// The next three fire times, in the chosen zone, with jitter applied.
///
/// The whole reason the wizard earns its place. `0 3 * * 1` is Mondays and `0 3 1 * *` is
/// the 1st — a mistake the config file invites silently and that three dates settle before
/// anything is written. It is also where the DST rules stop being theoretical.
fn preview(ctx: &Ctx, id: &str, cron: &str, tz: &str) -> Result<()> {
    let parsed = Cron::parse(cron).map_err(|e| anyhow::anyhow!("{e}"))?;
    let zone = jiff::tz::TimeZone::get(tz).context("resolving the time zone")?;

    let jitter_secs = centinel_core::config::DEFAULT_JITTER_SECS;
    let offset = jiff::Span::new().seconds(jitter_offset(
        &ctx.store.root().display().to_string(),
        id,
        jitter_secs,
    ) as i64);

    let fires: Vec<String> = parsed
        .next_n(jiff::Timestamp::now(), &zone, 3)
        .into_iter()
        .map(|at| {
            (at + offset)
                .to_zoned(zone.clone())
                .strftime("%a %e %b %H:%M %Z")
                .to_string()
        })
        .collect();

    eprintln!();
    eprintln!("  {id} — {cron} {tz}, ±{}s jitter", jitter_secs);
    if fires.is_empty() {
        eprintln!("  next three:  never — this expression does not occur");
    } else {
        eprintln!("  next three:  {}", fires.join(" · "));
    }
    eprintln!();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The gate is about *which* invocation, not about the terminal — that half cannot be
    /// tested without one. What matters here is that a fully-specified command line, and
    /// every op that is not this one, fall straight through.
    #[test]
    fn only_a_bare_schedule_set_is_a_candidate_for_prompting() {
        // Not this op.
        assert!(!should_prompt("run", &json!({})));
        assert!(!should_prompt(
            "source",
            &json!({"action": {"action": "add"}})
        ));

        // The other action on this op.
        assert!(!should_prompt(
            "schedule",
            &json!({"action": {"action": "rm", "id": "x"}})
        ));

        // Already answered: a half-typed command line is more likely a mistake than a
        // request to be interviewed about the rest.
        assert!(!should_prompt(
            "schedule",
            &json!({"action": {"action": "set", "id": "daily", "cron": "@daily"}})
        ));
        assert!(!should_prompt(
            "schedule",
            &json!({"action": {"action": "set", "id": "daily", "cron": null}})
        ));
    }

    /// The shape the gate reads, pinned against the type it reads from.
    ///
    /// This is the bug that shipped: `ScheduleArgs` wraps a subcommand enum, so the fields
    /// sit under `action`, and looking for them at the top level found nothing. The gate
    /// never opened, the wizard never ran, and the only symptom was the op's own "run this
    /// on a terminal" error — the one message that makes it look like the terminal check
    /// is what failed.
    #[test]
    fn the_gate_reads_the_shape_the_op_actually_produces() {
        let args = serde_json::to_value(centinel_core::ops::ScheduleArgs {
            action: centinel_core::ops::ScheduleAction::Set(Default::default()),
        })
        .unwrap();

        let set = set_args(&args).expect("the `set` action was not found where it lives");
        assert!(set.contains_key("id"), "{args}");
        assert!(set.contains_key("cron"), "{args}");
        assert!(
            args.get("id").is_none(),
            "the fields are not at the top level; the gate must not look there"
        );
    }

    /// Every preset has to parse, or the wizard offers a choice that will not write.
    #[test]
    fn every_preset_is_a_real_expression() {
        for (label, expr) in PRESETS {
            let cron = Cron::parse(expr)
                .unwrap_or_else(|e| panic!("preset `{label}` does not parse: {e}"));
            let fires = cron.next_n(jiff::Timestamp::now(), &jiff::tz::TimeZone::UTC, 2);
            assert_eq!(fires.len(), 2, "preset `{label}` never occurs");
        }
    }

    /// The stage list the wizard offers must be the stage list the config accepts, or it
    /// will happily write a `skip` entry that fails validation.
    #[test]
    fn the_offered_stages_are_the_ones_a_schedule_accepts() {
        let block = format!(
            "[[schedule]]\nid = \"x\"\ncron = \"@daily\"\nskip = [{}]\n",
            STAGE_NAMES
                .iter()
                .map(|s| format!("\"{s}\""))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let config = Config::parse(&block).expect("the wizard's stage names are not accepted");
        config.schedules[0]
            .run_args()
            .expect("a stage name the wizard offers was refused");
    }
}
