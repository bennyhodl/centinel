//! `classify` — score indexed documents with Jev.
//!
//! The command for running the classifiers over the corpus, one source, or one document,
//! as often as wanted. It is the same run the web workspace starts from its Classify
//! button: the saved questions go to Jev with each document's derived text, the answers
//! land in the workspace, and the current policy decides what they mean — at once. A
//! document whose junk probability clears the gate's threshold is out of search the
//! moment its answer is written, every tag that clears its threshold is on the record for
//! search to filter by, the review band is left for a person, and `restore` undoes any
//! exclusion.
//!
//! ## Everything in scope, every time
//!
//! A run scores every indexed document in its scope, excluded ones included, with no cap:
//! rerunning the corpus, moving a threshold, rewording a question and retagging are the
//! normal uses. A document is sent again whether or not it has an answer. The one
//! subtraction is `--continue`: a run that stopped — Jev refused the account, the process
//! was interrupted — is carried on over the documents in its scope that no run in its
//! chain has answered yet.
//!
//! ## Nothing leaves the machine unasked
//!
//! Text goes to TypeSafe only when this command, or the web's button, runs. Classifying
//! is not a pipeline stage. `--dry-run` prices the work and sends nothing; `--preview`
//! sends and shows, and writes nothing.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::prelude::*;
use crate::workspace::{
    self, Question, QuestionKind, RunDetailQuery, RunRequest, RunResult, RunScope, RunSettings,
    Workspace,
};

#[derive(Clone, Debug, Default, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct ClassifyArgs {
    /// Limit to one source. Omit for the whole corpus.
    #[arg(long)]
    #[serde(default)]
    pub source: Option<String>,

    /// One document: a URL, a substring of one, or a blob hash — the same targets `search`
    /// prints.
    #[arg(long, value_name = "TARGET")]
    #[serde(default)]
    pub document: Option<String>,

    /// Question id to run. Repeatable. Omit for every saved question.
    #[arg(long = "question", value_name = "ID")]
    #[serde(default)]
    pub questions: Vec<String>,

    /// Carry on a run that stopped: the same scope, questions and settings, over the
    /// documents no run in its chain has answered. `--source`, `--document`, `--question`
    /// and `--model` are taken from that run.
    #[arg(long = "continue", value_name = "RUN")]
    #[serde(default)]
    pub continue_from: Option<String>,

    /// Count and price the work. Nothing is sent.
    #[arg(long)]
    #[serde(default)]
    pub dry_run: bool,

    /// Score and report, but record nothing: no run, no exclusions, no tags.
    #[arg(long)]
    #[serde(default)]
    pub preview: bool,

    /// The Jev model. Defaults to `model` under `[classify]` in the config.
    #[arg(long)]
    #[serde(default)]
    pub model: Option<String>,

    /// Documents in flight to Jev at once. Eight unless set; at most thirty-two.
    #[arg(long)]
    #[serde(default)]
    pub concurrency: Option<usize>,

    /// Config file. Defaults to the usual search path.
    #[arg(long, value_name = "FILE")]
    #[serde(default)]
    pub config: Option<String>,
}

/// One document Jev did not answer for, and what it said.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ClassifyFailure {
    pub resource: String,
    pub error: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ClassifyReport {
    pub model: String,
    /// The question ids sent, in the saved order.
    pub questions: Vec<String>,
    /// Documents in scope for this run.
    pub selected: usize,
    /// Documents Jev answered for.
    pub scored: usize,
    pub failed: usize,
    /// Under the current policy: excluded by the gate, held for review, carrying at least
    /// one tag, or none of those.
    pub excluded: usize,
    pub review: usize,
    pub tagged: usize,
    pub kept: usize,
    /// Documents none of the run's questions reached: each was a follow-up whose parent
    /// landed elsewhere. Nothing was sent for them.
    #[serde(default)]
    pub not_asked: usize,
    /// Documents per tag. A yes-or-no question tags under its id; a choice option under
    /// `question:option`.
    pub tags: BTreeMap<String, usize>,
    /// Before sending: the characters that would go, counted at the sampling limit, plus
    /// the questions, at about four characters a token. A follow-up is counted for the
    /// share of documents expected to reach it, and its text sent again.
    pub estimated_tokens: u64,
    /// At the model's published rate, when one is known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_cost_usd: Option<f64>,
    /// What Jev reported it read, summed over the documents that reported usage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    pub elapsed_secs: f64,
    /// The recorded run, so its detail can be read back in the Runs view.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub dry_run: bool,
    pub preview: bool,
    /// The first few failures, with Jev's own words.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failures: Vec<ClassifyFailure>,
}

/// How many failures the report names before it counts the rest.
const FAILURES_SHOWN: usize = 10;

/// Score indexed documents with Jev: exclude junk, tag the rest.
#[op(long_running, reach = "operator", group = "stage")]
pub async fn classify(
    ctx: &Ctx,
    args: ClassifyArgs,
    progress: &Progress,
    cancel: &Cancel,
) -> anyhow::Result<ClassifyReport> {
    let ws = Workspace::new(&ctx.store);
    // A continuation takes its questions, scope and model from the run it continues; the
    // flags that would name them are not read.
    let continuing = args.continue_from.is_some();
    let questions = match continuing {
        true => Vec::new(),
        false => workspace::chosen(&ws.questions()?, &args.questions)?,
    };

    // The config is opened only for what the flags did not say, so a run that names its
    // model works in a directory with no `centinel.toml` at all.
    let (model, concurrency) = match (&args.model, continuing) {
        (Some(model), _) => (model.clone(), args.concurrency),
        (None, true) => (String::new(), args.concurrency),
        (None, false) => {
            let (config, _) = super::load_config(args.config.as_deref())?;
            let block = config.classify.ok_or_else(|| {
                anyhow::anyhow!(
                    "no classifier model: pass --model, or add `[classify]` with `model = \
                     \"jev-1.13.0\"` to centinel.toml"
                )
            })?;
            (block.model, args.concurrency.or(block.concurrency))
        }
    };

    progress.say("reading the index");
    let scope = match (&args.document, continuing) {
        (_, true) => None,
        (Some(target), false) => Some(RunScope {
            source: None,
            document: Some(
                ws.document_by_target(ctx, target, args.source.as_deref())
                    .await?,
            ),
        }),
        (None, false) => Some(RunScope {
            source: args.source.clone(),
            document: None,
        }),
    };
    let plan = ws.plan(
        scope.as_ref(),
        args.continue_from.as_deref(),
        &questions,
        &model,
    )?;

    let mut report = ClassifyReport {
        model: model.clone(),
        questions: questions.iter().map(|q| q.id.clone()).collect(),
        selected: plan.documents,
        scored: 0,
        failed: 0,
        excluded: 0,
        review: 0,
        tagged: 0,
        kept: 0,
        not_asked: 0,
        tags: BTreeMap::new(),
        estimated_tokens: plan.estimated_tokens,
        estimated_cost_usd: plan.estimated_cost_usd,
        input_tokens: None,
        cost_usd: None,
        elapsed_secs: 0.0,
        run_id: None,
        dry_run: args.dry_run,
        preview: args.preview,
        failures: Vec::new(),
    };
    // A continuation with nothing left is refused by `prepare`, naming the run; an empty
    // scope is simply nothing to do.
    if args.dry_run || (plan.documents == 0 && !continuing) {
        return Ok(report);
    }

    let mut settings = RunSettings::default();
    if let Some(concurrency) = concurrency {
        settings
            .values
            .insert("concurrency".into(), json!(concurrency));
    }
    let request = RunRequest {
        questions,
        documents: Vec::new(),
        selection: None,
        repeat: None,
        scope,
        continue_from: args.continue_from.clone(),
        model,
        evaluation_date: today(),
        settings,
        record: !args.preview,
    };

    let prepared = ws.prepare(request)?;
    // Said after `prepare`, which is where a continuation learns its model and questions.
    report.model = prepared.run().model.clone();
    report.questions = prepared
        .run()
        .questions
        .iter()
        .map(|q| q.id.clone())
        .collect();
    progress.say(format!(
        "sending {} to {}",
        render::plural(report.selected, "document", "documents"),
        report.model
    ));
    let run = ws.execute_with(prepared, progress, cancel).await?;

    // The same counting the Runs view does, under the policy the run was asked with. A
    // recorded run's results are read back from the workspace; a preview's travel with it.
    let detail = ws.run_detail(
        &run.id,
        RunDetailQuery {
            outcome: "error".into(),
            page_size: FAILURES_SHOWN,
            ..RunDetailQuery::default()
        },
    )?;
    let view = detail.view.unwrap_or_default();
    report.scored = view.scored - view.documents.errors - view.documents.not_asked;
    report.failed = view.documents.errors;
    report.excluded = view.documents.excluded;
    report.review = view.documents.review;
    report.tagged = view.documents.tagged;
    report.kept = view.documents.kept;
    report.not_asked = view.documents.not_asked;
    report.tags = tag_counts(&run.questions, &view.questions);
    report.input_tokens = run.input_tokens;
    report.cost_usd = run.cost_usd;
    report.elapsed_secs = run.duration_ms.unwrap_or_default() as f64 / 1000.0;
    report.failures = failures(&detail.results);
    if !args.preview {
        report.run_id = Some(run.id);
    }
    Ok(report)
}

/// Documents per tag, keyed the way search filters: a yes-or-no question under its own
/// id, a choice option under `question:option`.
fn tag_counts(
    questions: &[Question],
    totals: &BTreeMap<String, workspace::QuestionTotals>,
) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for (id, totals) in totals {
        let choice = questions
            .iter()
            .any(|q| &q.id == id && q.kind == QuestionKind::Choice);
        for (tag, count) in &totals.tags {
            let key = if choice {
                workspace::option_key(id, tag)
            } else {
                id.clone()
            };
            *out.entry(key).or_insert(0) += count;
        }
    }
    out
}

/// The first page of a run's failed results, with Jev's own words.
fn failures(results: &[RunResult]) -> Vec<ClassifyFailure> {
    results
        .iter()
        .filter_map(|r| {
            Some(ClassifyFailure {
                resource: r.resource.clone(),
                error: r.error.clone()?,
            })
        })
        .take(FAILURES_SHOWN)
        .collect()
}

/// Today, as Jev's `evaluation_date` wants it. The run records it, so a question like
/// "is this hearing upcoming" is answered against the day it was asked.
fn today() -> String {
    jiff::Zoned::now().date().to_string()
}

// ── rendering ─────────────────────────────────────────────────────────────────

/// The decision counts first, then what it cost, then the tags, then what failed.
impl Render for ClassifyReport {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        let aside = format!(
            "{} · {}",
            render::plural(self.questions.len(), "question", "questions"),
            render::duration(self.elapsed_secs)
        );
        p.title(&self.model, &aside)?;
        p.nest(|p| {
            if self.dry_run {
                let text = format!(
                    "dry run — {} would be sent, about {} input tokens{}",
                    render::plural(self.selected, "document", "documents"),
                    render::count(self.estimated_tokens),
                    match self.estimated_cost_usd {
                        Some(cost) => format!(" · about ${cost:.2}"),
                        None => String::new(),
                    }
                );
                p.marked(Mark::Warn, p.paint(&text, Ink::Dim))?;
            } else if self.selected == 0 {
                p.line(p.paint("nothing in scope", Ink::Dim))?;
            } else {
                let mut figures = vec![
                    (self.scored as u64, "scored"),
                    (self.excluded as u64, "excluded"),
                    (self.review as u64, "held for review"),
                    (self.tagged as u64, "tagged"),
                    (self.kept as u64, "kept untagged"),
                ];
                if self.not_asked > 0 {
                    figures.push((self.not_asked as u64, "not asked"));
                }
                if self.failed > 0 {
                    figures.push((self.failed as u64, "failed"));
                }
                p.figures(&figures)?;
                p.blank()?;
                let mut line = match self.input_tokens {
                    Some(tokens) => format!("{} input tokens", render::count(tokens)),
                    None => "usage not reported".to_string(),
                };
                if let Some(cost) = self.cost_usd {
                    line.push_str(&format!(" · ${cost:.2}"));
                }
                if self.preview {
                    line.push_str(" · preview, nothing written");
                } else if let Some(id) = &self.run_id {
                    line.push_str(&format!(" · run {id}"));
                }
                p.line(p.paint(&line, Ink::Dim))?;
            }

            if !self.tags.is_empty() {
                p.section("tags")?;
                let mut table = Table::new(&[("tag", Align::Left), ("documents", Align::Right)]);
                let mut rows: Vec<_> = self.tags.iter().collect();
                rows.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
                for (tag, count) in rows {
                    table.push(vec![
                        Cell::plain(tag),
                        Cell::plain(render::count(*count as u64)),
                    ]);
                }
                p.table(&table)?;
            }

            if !self.failures.is_empty() {
                p.section("failed")?;
                for failure in &self.failures {
                    let text = format!(
                        "{}  {}",
                        render::truncate_start(&failure.resource, 48),
                        render::one_line(&failure.error)
                    );
                    p.marked(Mark::Warn, p.paint(&text, Ink::Dim))?;
                }
                if self.failed > self.failures.len() {
                    let more = format!("and {} more", self.failed - self.failures.len());
                    p.line(p.paint(&more, Ink::Dim))?;
                }
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::{ChunkConfig, chunk_markdown};
    use crate::index::{Index, Placement};
    use crate::store::Store;
    use crate::workspace::fake_jev::fake_jev;
    use jiff::Timestamp;

    /// Two documents the way a run leaves them: observed and derived in the log, the
    /// derived text in the pool, and chunked into the index. A budget record and a
    /// navigation menu.
    async fn corpus() -> (tempfile::TempDir, Ctx) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        std::fs::write(dir.path().join(".env"), "TYPESAFE_API_KEY=test-key\n").unwrap();
        let mut index = Index::open(store.index_path()).unwrap();
        let city = SourceId::new("city".to_string()).unwrap();
        let docs = [
            (
                "https://example.gov/budget",
                "# Budget hearing\n\nResidents may speak about the proposed budget on October 20.",
            ),
            (
                "https://example.gov/menu",
                "# MENU\n\nHome · Quick Links · Contact Us · Site Map",
            ),
        ];
        for (resource, text) in docs {
            let html = format!("<html><body>{text}</body></html>");
            let meta = BTreeMap::from([
                ("content-type".to_string(), "text/html".to_string()),
                ("final_url".to_string(), resource.to_string()),
            ]);
            let observation = store
                .record_observation(
                    &Resource::new(city.clone(), resource),
                    html.as_bytes(),
                    Timestamp::now(),
                    meta,
                )
                .await
                .unwrap();
            let derived = store.put_blob(text.as_bytes()).await.unwrap();
            store
                .append(
                    &city,
                    &LogRecord::Derivation(Derivation {
                        from_sha: observation.blob_sha.clone(),
                        to_sha: derived.clone(),
                        tool: "test".into(),
                        version: "1".into(),
                        model_tier: None,
                        at: Timestamp::now(),
                        anchors: vec![],
                    }),
                )
                .await
                .unwrap();
            let derived = derived.to_string();
            for chunk in chunk_markdown(text, &ChunkConfig::default()) {
                index
                    .insert(
                        &chunk,
                        &Placement {
                            source: "city".into(),
                            resource: resource.into(),
                            blob_sha: observation.blob_sha.to_string(),
                            derived_sha: derived.clone(),
                            ordinal: chunk.ordinal,
                            heading: chunk.heading.clone(),
                            char_start: chunk.char_start,
                            char_end: chunk.char_end,
                            observed_at: "2026-10-07T12:00:00Z".into(),
                            tool: "test 1".into(),
                            title: Some(resource.into()),
                        },
                    )
                    .unwrap();
            }
        }
        drop(index);
        (dir, Ctx::new(store))
    }

    fn args(questions: &[&str]) -> ClassifyArgs {
        ClassifyArgs {
            model: Some("jev-test".into()),
            questions: questions.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    /// The whole loop: a dry run prices and sends nothing, a preview scores and writes
    /// nothing, a run scores everything in scope, the gate's exclusion is live at once,
    /// and the next run scores everything again — only a continuation subtracts.
    #[tokio::test]
    async fn a_run_scores_the_scope_and_the_gate_excludes_live() {
        let endpoint = fake_jev().await;
        // SAFETY: the only reader is a run that holds a key, and the only test with a key
        // is this one, in its own store.
        unsafe { std::env::set_var("CENTINEL_TYPESAFE_ENDPOINT", &endpoint) };
        let (_dir, ctx) = corpus().await;
        let ws = Workspace::new(&ctx.store);

        let dry = classify(
            &ctx,
            ClassifyArgs {
                dry_run: true,
                ..args(&["page_kind", "laws"])
            },
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap();
        assert_eq!(dry.selected, 2);
        assert!(dry.estimated_tokens > 0);
        assert!(dry.estimated_cost_usd.unwrap() > 0.0);
        assert!(dry.run_id.is_none());
        assert_eq!(
            ws.runs(Default::default()).unwrap().total,
            0,
            "a dry run writes nothing"
        );
        assert!(!ctx.store.spend_dir().exists(), "a dry run spends nothing");

        let preview = classify(
            &ctx,
            ClassifyArgs {
                preview: true,
                ..args(&["page_kind", "laws"])
            },
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap();
        assert_eq!(preview.scored, 2);
        assert_eq!(preview.excluded, 1);
        assert!(preview.run_id.is_none());
        assert_eq!(
            ws.runs(Default::default()).unwrap().total,
            0,
            "a preview writes nothing"
        );
        assert_eq!(
            ws.documents(Default::default()).unwrap().facets.usage["excluded"],
            0,
            "and excludes nothing"
        );

        let run = classify(
            &ctx,
            args(&["page_kind", "laws"]),
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap();
        assert_eq!(run.selected, 2);
        assert_eq!(run.scored, 2);
        assert_eq!(run.failed, 0);
        assert_eq!(run.excluded, 1, "the menu is junk");
        assert_eq!(
            run.tags.get("laws"),
            Some(&1),
            "the record is tagged, the junk is not"
        );
        assert_eq!(
            run.tags.get("page_kind:record"),
            None,
            "record is a keep option"
        );
        assert_eq!(run.input_tokens, Some(200));
        assert!(run.run_id.is_some());

        // A line per document Jev answered — the preview's two as well, since a preview is
        // paid for — priced as the run is.
        let spent = crate::spend::Ledger::new(&ctx.store)
            .read(jiff::Timestamp::UNIX_EPOCH)
            .unwrap();
        assert_eq!(spent.len(), 4, "{spent:#?}");
        assert!(
            spent
                .iter()
                .all(|s| s.stage == crate::spend::Stage::Classify
                    && s.provider == crate::spend::Provider::Jev
                    && s.input_tokens == 100)
        );
        let this_run: Vec<_> = spent.iter().filter(|s| s.run_id == run.run_id).collect();
        assert_eq!(this_run.len(), 2);
        let ledger_cost: f64 = this_run.iter().filter_map(|s| s.cost_usd).sum();
        assert!((ledger_cost - run.cost_usd.unwrap()).abs() < 1e-12);
        assert_eq!(run.questions, ["page_kind", "laws"]);

        // The exclusion is live: search no longer cites the menu, and the Corpus says why.
        let index = Index::open(ctx.store.index_path()).unwrap();
        assert!(
            index
                .search("Quick Links", 10, &crate::index::Filter::default())
                .unwrap()
                .is_empty()
        );
        assert!(
            !index
                .search("Budget hearing", 10, &crate::index::Filter::default())
                .unwrap()
                .is_empty()
        );
        drop(index);
        let excluded = ws
            .documents(crate::workspace::DocumentQuery {
                usage: "excluded".into(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(excluded.total, 1);
        assert_eq!(excluded.documents[0].resource, "https://example.gov/menu");
        assert_eq!(
            excluded.documents[0].exclusion_reason.as_deref(),
            Some("page_kind")
        );

        // Run again: everything in scope is scored again, excluded or not.
        let again = classify(
            &ctx,
            args(&["page_kind", "laws"]),
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap();
        assert_eq!(again.selected, 2);
        assert_eq!(again.scored, 2);
        assert!(again.run_id.is_some());

        // A finished run has nothing to continue.
        let error = classify(
            &ctx,
            ClassifyArgs {
                continue_from: again.run_id.clone(),
                ..Default::default()
            },
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(error.contains("every document in scope"), "{error}");

        // One document by its address.
        let one = classify(
            &ctx,
            ClassifyArgs {
                document: Some("example.gov/budget".into()),
                ..args(&["laws"])
            },
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap();
        assert_eq!(one.selected, 1);
        assert_eq!(one.scored, 1);
    }

    #[tokio::test]
    async fn an_unknown_question_id_is_refused_before_anything_is_sent() {
        let (_dir, ctx) = corpus().await;
        let error = classify(
            &ctx,
            args(&["page_kind", "nope"]),
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(error.contains("`nope`"), "{error}");
        assert_eq!(
            Workspace::new(&ctx.store)
                .runs(Default::default())
                .unwrap()
                .total,
            0
        );
    }

    #[tokio::test]
    async fn without_a_model_the_error_names_the_config_block() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        let ctx = Ctx::new(store);
        let config = dir.path().join("centinel.toml");
        std::fs::write(&config, "[defaults]\nrps = 1.0\n").unwrap();
        let error = classify(
            &ctx,
            ClassifyArgs {
                config: Some(config.display().to_string()),
                ..Default::default()
            },
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(error.contains("[classify]"), "{error}");
    }

    #[test]
    fn tags_are_keyed_the_way_search_filters() {
        let questions = workspace::default_questions();
        let mut totals = BTreeMap::new();
        totals.insert(
            "laws".to_string(),
            workspace::QuestionTotals {
                tags: BTreeMap::from([("laws".to_string(), 3)]),
                ..Default::default()
            },
        );
        totals.insert(
            "record_type".to_string(),
            workspace::QuestionTotals {
                tags: BTreeMap::from([("minutes".to_string(), 2), ("agenda".to_string(), 1)]),
                ..Default::default()
            },
        );
        let counts = tag_counts(&questions, &totals);
        assert_eq!(counts["laws"], 3);
        assert_eq!(counts["record_type:minutes"], 2);
        assert_eq!(counts["record_type:agenda"], 1);
    }

    #[test]
    fn the_report_reads_as_decisions_then_cost_then_tags() {
        let report = ClassifyReport {
            model: "jev-1.13.0".into(),
            questions: vec!["page_kind".into(), "laws".into()],
            selected: 120,
            scored: 118,
            failed: 2,
            excluded: 40,
            review: 10,
            tagged: 50,
            kept: 18,
            not_asked: 0,
            tags: BTreeMap::from([
                ("laws".to_string(), 50),
                ("record_type:minutes".to_string(), 7),
            ]),
            estimated_tokens: 500_000,
            estimated_cost_usd: Some(0.021),
            input_tokens: Some(480_000),
            cost_usd: Some(0.02),
            elapsed_secs: 61.0,
            run_id: Some("run-1".into()),
            dry_run: false,
            preview: false,
            failures: vec![ClassifyFailure {
                resource: "https://example.gov/x".into(),
                error: "Jev answered 400: too large".into(),
            }],
        };
        let mut buf = Vec::new();
        {
            let mut p = Painter::new(&mut buf, false, 100);
            report.render(&mut p).unwrap();
        }
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains("40") && out.contains("excluded"), "{out}");
        assert!(out.contains("run run-1"), "{out}");
        assert!(out.contains("record_type:minutes"), "{out}");
        assert!(out.contains("too large"), "{out}");
        assert!(out.contains("and 1 more"), "{out}");

        let dry = ClassifyReport {
            dry_run: true,
            run_id: None,
            ..report
        };
        let mut buf = Vec::new();
        {
            let mut p = Painter::new(&mut buf, false, 100);
            dry.render(&mut p).unwrap();
        }
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains("dry run") && out.contains("500,000"), "{out}");
    }
}
