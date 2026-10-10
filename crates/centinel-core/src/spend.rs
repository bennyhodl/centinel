//! Spend — every model call, its tokens, and what it cost.
//!
//! One line per call in `spend/YYYY-MM.jsonl`: a document Jev scored (every level of its
//! chain), an OpenRouter embedding batch, a local embedding batch, a query vector. The page
//! that reads it shows one series per model, so the ledger keeps the model as it was
//! called and nothing coarser.
//!
//! ## Called "spend", not "usage"
//!
//! Usage already means a corpus decision here — whether a document is included — and a
//! second meaning on the same page would make every filter ambiguous.
//!
//! ## Priced when written
//!
//! A line carries its cost, worked out from [`PRICES`] at the moment of the call (or the
//! provider's own figure, when it sends one). A price change later re-prices nothing: the
//! ledger says what a call cost on the day it was made, which is what a bill says too.
//!
//! ## Local models are priced as the cloud
//!
//! A local model is its own model on the page — `qwen3-embedding-4b` is never filed under
//! OpenRouter — but it is priced at the cloud model with the same weights ([`PRICED_AS`]),
//! so the page can say what running it here would have cost there. The token counts are
//! exact: the weights are the same, and so is the tokenizer. A local model with no cloud
//! twin is counted and left unpriced, not priced at a guess.
//!
//! ## A failed write does not stop a run
//!
//! The money is spent by the time the line is written. Failing the run there would throw
//! away the answer that was paid for, so a write that fails is logged and the run goes on.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::sync::Mutex;

use anyhow::Context;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::store::Store;

/// Who ran the model. Read off the id, so a line cannot claim one and name another.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    /// TypeSafe's Jev, through its API.
    Jev,
    /// An `openrouter/…` model.
    OpenRouter,
    /// Weights on this machine.
    Local,
}

impl Provider {
    pub fn of(model: &str) -> Self {
        if model.starts_with(JEV_PREFIX) {
            Self::Jev
        } else if model.starts_with(crate::remote::PREFIX) {
            Self::OpenRouter
        } else {
            Self::Local
        }
    }
}

/// What the call was for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// A document scored by a classifier.
    Classify,
    /// Corpus chunks embedded.
    Embed,
    /// A search query embedded.
    Query,
}

/// One request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Spend {
    pub at: Timestamp,
    pub provider: Provider,
    pub stage: Stage,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// `None` when the model has no price — its tokens still count.
    pub cost_usd: Option<f64>,
    /// The cost is the provider's own figure rather than [`PRICES`] times the tokens.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reported: bool,
    /// The classifier run the call belonged to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
}

impl Spend {
    /// A call made now, priced from [`PRICES`].
    pub fn new(stage: Stage, model: &str, input_tokens: u64, output_tokens: u64) -> Self {
        Self {
            at: Timestamp::now(),
            provider: Provider::of(model),
            stage,
            model: model.to_string(),
            input_tokens,
            output_tokens,
            cost_usd: cost(model, input_tokens, output_tokens),
            reported: false,
            run_id: None,
        }
    }

    /// The provider said what the call cost; that figure wins over the list price.
    pub fn reported(mut self, cost_usd: Option<f64>) -> Self {
        if let Some(cost) = cost_usd.filter(|c| c.is_finite()) {
            self.cost_usd = Some(cost);
            self.reported = true;
        }
        self
    }

    pub fn run(mut self, run_id: &str) -> Self {
        self.run_id = Some(run_id.to_string());
        self
    }
}

// ---- prices ----------------------------------------------------------------------------

/// Every Jev version is one price.
const JEV_PREFIX: &str = "jev-";

const OPENROUTER: &str = "OpenRouter list price, 2026-10-10";

/// USD per million tokens, for every model that has a price. Adding a model is one line.
///
/// Pinned rather than fetched: a price list pulled from the network at read time would
/// make the page depend on a request that §2.1 never asked this machine to make, and
/// these prices change a few times a year. When one does, change the line and the date.
pub static PRICES: &[Listing] = &[
    Listing {
        model: "jev-*",
        input: 0.042,
        output: 0.0,
        source: "TypeSafe published price, 2026-09-14",
    },
    Listing {
        model: "openrouter/qwen/qwen3-embedding-8b",
        input: 0.01,
        output: 0.0,
        source: OPENROUTER,
    },
    Listing {
        model: "openrouter/qwen/qwen3-embedding-4b",
        input: 0.02,
        output: 0.0,
        source: OPENROUTER,
    },
    Listing {
        model: "openrouter/openai/text-embedding-3-large",
        input: 0.13,
        output: 0.0,
        source: OPENROUTER,
    },
    Listing {
        model: "openrouter/openai/text-embedding-3-small",
        input: 0.02,
        output: 0.0,
        source: OPENROUTER,
    },
];

/// Local models and the cloud model with the same weights, whose price they take.
pub static PRICED_AS: &[(&str, &str)] =
    &[("qwen3-embedding-4b", "openrouter/qwen/qwen3-embedding-4b")];

/// One line of [`PRICES`].
#[derive(Clone, Copy, Debug)]
pub struct Listing {
    /// An id, or a family when it ends in `*`.
    pub model: &'static str,
    /// USD per million input tokens.
    pub input: f64,
    /// USD per million output tokens.
    pub output: f64,
    /// Where the number came from, and when.
    pub source: &'static str,
}

impl Listing {
    fn covers(&self, model: &str) -> bool {
        match self.model.strip_suffix('*') {
            Some(family) => model.starts_with(family),
            None => self.model == model,
        }
    }
}

/// A model's price, as the page shows it.
#[derive(Clone, Debug, Serialize)]
pub struct Price {
    pub model: String,
    pub provider: Provider,
    pub input: f64,
    pub output: f64,
    pub source: String,
    /// For a local model: the cloud model whose price it takes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priced_as: Option<String>,
}

/// The price a model is charged at, or `None` when it has none.
pub fn price(model: &str) -> Option<Price> {
    let priced_as = PRICED_AS
        .iter()
        .find(|(local, _)| *local == model)
        .map(|(_, cloud)| *cloud);
    let listing = PRICES
        .iter()
        .find(|l| l.covers(priced_as.unwrap_or(model)))?;
    Some(Price {
        model: model.to_string(),
        provider: Provider::of(model),
        input: listing.input,
        output: listing.output,
        source: listing.source.to_string(),
        priced_as: priced_as.map(str::to_string),
    })
}

/// What `input` and `output` tokens of `model` cost, or `None` when it has no price.
pub fn cost(model: &str, input_tokens: u64, output_tokens: u64) -> Option<f64> {
    let price = price(model)?;
    Some((input_tokens as f64 * price.input + output_tokens as f64 * price.output) / 1_000_000.0)
}

/// Every price, local models included, for the spend page's model detail and the classify
/// estimate.
pub fn prices() -> Vec<Price> {
    PRICES
        .iter()
        .map(|l| l.model)
        .chain(PRICED_AS.iter().map(|(local, _)| *local))
        .filter_map(price)
        .collect()
}

// ---- the ledger ------------------------------------------------------------------------

/// Appends from every thread in this process go through one lock, so two lines never
/// interleave.
static WRITE: Mutex<()> = Mutex::new(());

/// Where calls are written: `spend/` in one store. Cheap to clone.
#[derive(Clone, Debug)]
pub struct Ledger {
    store: Store,
}

impl Ledger {
    pub fn new(store: &Store) -> Self {
        Self {
            store: store.clone(),
        }
    }

    /// Writes one line. A failure is logged, not returned — see the module docs.
    pub fn record(&self, spend: Spend) {
        if let Err(error) = self.append(&spend) {
            tracing::warn!(model = %spend.model, error = %format!("{error:#}"), "spend not recorded");
        }
    }

    fn append(&self, spend: &Spend) -> anyhow::Result<()> {
        let path = self.store.spend_path(spend.at);
        let mut line = serde_json::to_vec(spend)?;
        line.push(b'\n');
        let _lock = WRITE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
        file.write_all(&line)?;
        file.sync_data()?;
        Ok(())
    }

    /// Every call from `since` on, oldest month first.
    pub fn read(&self, since: Timestamp) -> anyhow::Result<Vec<Spend>> {
        let dir = self.store.spend_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }
        // Months are named `YYYY-MM.jsonl`, so the names sort as the months do and a file
        // before `since`'s month holds nothing to read.
        let first = self.store.spend_path(since);
        let mut months: Vec<_> = fs::read_dir(&dir)?
            .map(|entry| entry.map(|e| e.path()))
            .collect::<Result<_, _>>()?;
        months.retain(|p| p.extension().is_some_and(|e| e == "jsonl") && *p >= first);
        months.sort();

        let mut out = Vec::new();
        for path in months {
            let file = fs::File::open(&path)?;
            for (i, line) in BufReader::new(file).lines().enumerate() {
                let line = line?;
                if line.trim().is_empty() {
                    continue;
                }
                let spend: Spend = serde_json::from_str(&line)
                    .with_context(|| format!("malformed {} line {}", path.display(), i + 1))?;
                if spend.at >= since {
                    out.push(spend);
                }
            }
        }
        Ok(out)
    }
}

// ---- the summary -----------------------------------------------------------------------

/// One model's calls for one stage in one hour.
///
/// Hours rather than days because a day is a question about the reader's time zone and
/// the server does not know it. The page folds hours into its own days.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Bucket {
    /// The start of the hour, UTC.
    pub hour: Timestamp,
    pub model: String,
    pub provider: Provider,
    pub stage: Stage,
    pub requests: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Summed over the priced requests.
    pub cost_usd: f64,
    /// Requests whose model had no price.
    pub unpriced: u64,
}

/// What the spend page reads.
#[derive(Clone, Debug, Serialize)]
pub struct Summary {
    pub since: Timestamp,
    pub read_at: Timestamp,
    pub buckets: Vec<Bucket>,
}

/// Every call since `since`, by hour, model and stage.
pub fn summary(store: &Store, since: Timestamp) -> anyhow::Result<Summary> {
    let mut buckets: BTreeMap<(Timestamp, String, Stage), Bucket> = BTreeMap::new();
    for spend in Ledger::new(store).read(since)? {
        let second = spend.at.as_second();
        let hour = Timestamp::from_second(second - second.rem_euclid(3600))?;
        let bucket = buckets
            .entry((hour, spend.model.clone(), spend.stage))
            .or_insert_with(|| Bucket {
                hour,
                model: spend.model.clone(),
                provider: spend.provider,
                stage: spend.stage,
                requests: 0,
                input_tokens: 0,
                output_tokens: 0,
                cost_usd: 0.0,
                unpriced: 0,
            });
        bucket.requests += 1;
        bucket.input_tokens += spend.input_tokens;
        bucket.output_tokens += spend.output_tokens;
        match spend.cost_usd {
            Some(cost) => bucket.cost_usd += cost,
            None => bucket.unpriced += 1,
        }
    }
    Ok(Summary {
        since,
        read_at: Timestamp::now(),
        buckets: buckets.into_values().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_remote_embedding_model_has_a_price() {
        for spec in crate::remote::REMOTE_REGISTRY {
            assert!(price(spec.id).is_some(), "{} has no price", spec.id);
        }
    }

    /// Every cloud model a local one is priced as must itself have a price, or the local
    /// model would silently read as unpriced.
    #[test]
    fn every_local_model_is_priced_as_a_listed_cloud_model() {
        for (local, cloud) in PRICED_AS {
            assert_eq!(Provider::of(local), Provider::Local, "{local}");
            assert!(
                PRICES.iter().any(|l| l.covers(cloud)),
                "{local} → {cloud} is not listed"
            );
        }
    }

    #[test]
    fn every_jev_version_is_priced_as_jev() {
        let cost = cost("jev-1.13.0", 1_000_000, 100).unwrap();
        assert!((cost - 0.042).abs() < 1e-12, "{cost}");
        assert_eq!(Provider::of("jev-1.13.0"), Provider::Jev);
    }

    /// A local model is its own model, at the cloud model's price.
    #[test]
    fn a_local_model_keeps_its_name_and_takes_the_cloud_price() {
        let local = Spend::new(Stage::Embed, "qwen3-embedding-4b", 1_000_000, 0);
        assert_eq!(local.model, "qwen3-embedding-4b");
        assert_eq!(local.provider, Provider::Local);
        assert_eq!(
            local.cost_usd,
            cost("openrouter/qwen/qwen3-embedding-4b", 1_000_000, 0)
        );
        assert_eq!(
            price("qwen3-embedding-4b").unwrap().priced_as.as_deref(),
            Some("openrouter/qwen/qwen3-embedding-4b")
        );
    }

    #[test]
    fn a_model_without_a_price_still_counts_its_tokens() {
        let spend = Spend::new(Stage::Embed, "qwen3-embedding-0.6b", 500, 0);
        assert_eq!(spend.cost_usd, None);
        assert_eq!(spend.input_tokens, 500);
    }

    #[test]
    fn a_reported_cost_wins_over_the_list_price() {
        let spend = Spend::new(Stage::Embed, "openrouter/qwen/qwen3-embedding-8b", 1_000, 0)
            .reported(Some(0.5));
        assert_eq!(spend.cost_usd, Some(0.5));
        assert!(spend.reported);
        let absent =
            Spend::new(Stage::Embed, "openrouter/qwen/qwen3-embedding-8b", 1_000, 0).reported(None);
        assert!(!absent.reported);
    }

    /// Written, read back across a month boundary, and folded into hours per model.
    #[test]
    fn the_summary_folds_calls_into_hours_per_model_and_stage() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::at(dir.path());
        let ledger = Ledger::new(&store);
        let at = |s: &str| s.parse::<Timestamp>().unwrap();
        let call = |when: &str, stage, model: &str, tokens| Spend {
            at: at(when),
            ..Spend::new(stage, model, tokens, 0)
        };
        ledger.record(call(
            "2026-09-30T23:10:00Z",
            Stage::Embed,
            "qwen3-embedding-4b",
            100,
        ));
        ledger.record(call(
            "2026-10-01T09:05:00Z",
            Stage::Classify,
            "jev-1.13.0",
            1_000_000,
        ));
        ledger.record(call(
            "2026-10-01T09:55:00Z",
            Stage::Classify,
            "jev-1.13.0",
            1_000_000,
        ));
        ledger.record(call(
            "2026-10-01T09:30:00Z",
            Stage::Query,
            "qwen3-embedding-0.6b",
            7,
        ));

        let all = summary(&store, at("2026-09-01T00:00:00Z")).unwrap();
        assert_eq!(all.buckets.len(), 3, "{:#?}", all.buckets);
        let jev = all
            .buckets
            .iter()
            .find(|b| b.model == "jev-1.13.0")
            .unwrap();
        assert_eq!(jev.hour, at("2026-10-01T09:00:00Z"));
        assert_eq!((jev.requests, jev.input_tokens), (2, 2_000_000));
        assert!((jev.cost_usd - 0.084).abs() < 1e-12);
        let small = all
            .buckets
            .iter()
            .find(|b| b.model == "qwen3-embedding-0.6b")
            .unwrap();
        assert_eq!((small.unpriced, small.cost_usd), (1, 0.0));

        let october = summary(&store, at("2026-10-01T00:00:00Z")).unwrap();
        assert!(
            october
                .buckets
                .iter()
                .all(|b| b.model != "qwen3-embedding-4b")
        );
    }
}
