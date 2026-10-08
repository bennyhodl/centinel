//! `classify` — score indexed documents with Jev and act on the answers.
//!
//! The pipeline's labelling stage, and the CLI for running it by hand over the corpus, one
//! source, or one document. It is the same run the web workspace starts from its Classify
//! view: the saved questions go to Jev with each document's derived text, the answers land
//! in `workspace/runs.jsonl`, and the current policy decides what they mean. What differs
//! is what happens at the end — this op **commits**. A document whose junk probability
//! clears the gate's threshold is excluded from search and embedding before `embed` ever
//! sees it, and every tag that clears its threshold is on the record for search to filter
//! by. The review band is left pending for a person, and `restore` undoes any exclusion.
//!
//! ## The work list is a subtraction
//!
//! Like every stage, this one asks what is left rather than what is new: an included
//! document is pending when it has no answer for some saved question *at that question's
//! current version*. Scoring a thousand documents and stopping leaves the next run the
//! rest; rewording a question puts every document back in the queue for that question
//! alone; a document the gate already excluded is not sent again to be tagged.
//! `--rescore` ignores all of that and sends every included document.
//!
//! ## Nothing leaves the machine unasked
//!
//! Text goes to TypeSafe only when this op runs, and the pipeline runs it only where
//! `centinel.toml` has a `[classify]` block. `--dry-run` prices the work and sends
//! nothing; `--preview` sends and shows, and writes nothing.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::prelude::*;
use crate::workspace::{
    self, MAX_RUN_DOCUMENTS, MAX_TEXT_BYTES, PendingDocument, Question, QuestionKind,
    RunDetailQuery, RunRequest, RunResult, RunSettings, Workspace,
};

#[derive(Clone, Debug, Default, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct ClassifyArgs {
    /// Limit to one source. Omit for the whole corpus.
    #[arg(long)]
    #[serde(default)]
    pub source: Option<String>,

    /// One document: a URL, a substring of one, or a blob hash — the same targets `search`
    /// prints. Scored whether or not it is pending.
    #[arg(long, value_name = "TARGET")]
    #[serde(default)]
    pub document: Option<String>,

    /// Question id to run. Repeatable. Omit for every saved question.
    #[arg(long = "question", value_name = "ID")]
    #[serde(default)]
    pub questions: Vec<String>,

    /// Score documents that already have answers under the current question versions.
    #[arg(long)]
    #[serde(default)]
    pub rescore: bool,

    /// Stop after this many documents. The rest stay pending for the next run.
    #[arg(long)]
    #[serde(default)]
    pub limit: Option<usize>,

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
    /// Documents chosen for this run, after `--limit`.
    pub selected: usize,
    /// Pending documents left behind by `--limit` or the run ceiling.
    pub remaining: usize,
    /// Documents Jev answered for.
    pub scored: usize,
    pub failed: usize,
    /// Under the current policy: excluded by the gate, held for review, carrying at least
    /// one tag, or none of those.
    pub excluded: usize,
    pub review: usize,
    pub tagged: usize,
    pub kept: usize,
    /// Documents per tag. A yes-or-no question tags under its id; a choice option under
    /// `question:option`.
    pub tags: BTreeMap<String, usize>,
    /// Before sending: the characters that would go, counted at the sampling limit, plus
    /// the questions, at about four characters a token.
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
    /// Exclusions written to `workspace/decisions.jsonl` by this run.
    pub committed: usize,
    pub dry_run: bool,
    pub preview: bool,
    /// The first few failures, with Jev's own words.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failures: Vec<ClassifyFailure>,
}

/// How many failures the report names before it counts the rest.
const FAILURES_SHOWN: usize = 10;

/// TypeSafe's published Jev input rate, per million tokens, for the estimate before a run.
/// A run records the rate it was actually priced at in its settings.
const JEV_INPUT_RATE: f64 = 0.042;

/// Score indexed documents with Jev: exclude junk, tag the rest.
#[op(long_running, reach = "operator", group = "stage")]
pub async fn classify(
    ctx: &Ctx,
    args: ClassifyArgs,
    progress: &Progress,
    cancel: &Cancel,
) -> anyhow::Result<ClassifyReport> {
    let ws = Workspace::new(&ctx.store);
    let questions = chosen(&ws.questions()?, &args.questions)?;

    // The config is opened only for what the flags did not say, so a run that names its
    // model works in a directory with no `centinel.toml` at all.
    let (model, concurrency) = match &args.model {
        Some(model) => (model.clone(), args.concurrency),
        None => {
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
    let mut pending = match &args.document {
        Some(target) => vec![
            ws.document_by_target(ctx, target, args.source.as_deref())
                .await?,
        ],
        None => ws.pending_documents(&questions, args.source.as_deref(), args.rescore)?,
    };
    let outstanding = pending.len();
    pending.truncate(
        args.limit
            .unwrap_or(MAX_RUN_DOCUMENTS)
            .min(MAX_RUN_DOCUMENTS),
    );
    let (estimated_tokens, estimated_cost_usd) = estimate(&pending, &questions, &model);

    let mut report = ClassifyReport {
        model: model.clone(),
        questions: questions.iter().map(|q| q.id.clone()).collect(),
        selected: pending.len(),
        remaining: outstanding - pending.len(),
        scored: 0,
        failed: 0,
        excluded: 0,
        review: 0,
        tagged: 0,
        kept: 0,
        tags: BTreeMap::new(),
        estimated_tokens,
        estimated_cost_usd,
        input_tokens: None,
        cost_usd: None,
        elapsed_secs: 0.0,
        run_id: None,
        committed: 0,
        dry_run: args.dry_run,
        preview: args.preview,
        failures: Vec::new(),
    };
    if args.dry_run || pending.is_empty() {
        return Ok(report);
    }

    let mut settings = RunSettings::default();
    if let Some(concurrency) = concurrency {
        settings
            .values
            .insert("concurrency".into(), json!(concurrency));
    }
    let request = RunRequest {
        questions: questions.clone(),
        documents: pending.into_iter().map(|p| p.id).collect(),
        selection: None,
        repeat: None,
        model,
        evaluation_date: today(),
        settings,
        record: !args.preview,
    };

    progress.say(format!(
        "sending {} to {}",
        render::plural(report.selected, "document", "documents"),
        report.model
    ));
    let prepared = ws.prepare(request)?;
    let run = ws.execute_with(prepared, progress, cancel).await?;

    // The same counting the Runs view does, under the policy the run was asked with.
    let (view, _) = workspace::view_results(
        &run.questions,
        run.results.clone(),
        &RunDetailQuery::default(),
        false,
    );
    report.scored = run.results.len() - view.documents.errors;
    report.failed = view.documents.errors;
    report.excluded = view.documents.excluded;
    report.review = view.documents.review;
    report.tagged = view.documents.tagged;
    report.kept = view.documents.kept;
    report.tags = tag_counts(&questions, &view.questions);
    report.input_tokens = run.input_tokens;
    report.cost_usd = run.cost_usd;
    report.elapsed_secs = run.duration_ms.unwrap_or_default() as f64 / 1000.0;
    report.failures = failures(&run.results);

    if !args.preview {
        progress.say("committing exclusions");
        let committed = ws.commit(&run.id)?;
        report.committed = committed.committed;
        report.run_id = Some(run.id);
    }
    Ok(report)
}

/// The saved questions a run asks: all of them, or the ids named. An id nobody saved is
/// an error rather than a quiet omission — a typo would otherwise score a corpus with the
/// wrong set and say nothing.
fn chosen(saved: &[Question], ids: &[String]) -> anyhow::Result<Vec<Question>> {
    if saved.is_empty() {
        anyhow::bail!(
            "no questions are saved — `centinel questions --add-defaults` restores the shipped \
             set"
        );
    }
    if ids.is_empty() {
        return Ok(saved.to_vec());
    }
    if let Some(unknown) = ids.iter().find(|id| !saved.iter().any(|q| &q.id == *id)) {
        anyhow::bail!("no saved question is called `{unknown}` — `centinel questions` lists them");
    }
    Ok(saved
        .iter()
        .filter(|q| ids.contains(&q.id))
        .cloned()
        .collect())
}

/// Input tokens and cost before anything is sent. Mirrors the web page's estimate: each
/// document's text at the sampling limit, plus the questions, which travel with every
/// request, at about four characters a token.
fn estimate(
    pending: &[PendingDocument],
    questions: &[Question],
    model: &str,
) -> (u64, Option<f64>) {
    let question_chars: usize = questions
        .iter()
        .map(|q| {
            q.instructions.len()
                + 70
                + q.options
                    .iter()
                    .map(|o| o.id.len() + o.description.len())
                    .sum::<usize>()
        })
        .sum();
    let chars: usize = pending
        .iter()
        .map(|p| p.chars.min(MAX_TEXT_BYTES) + question_chars)
        .sum();
    let tokens = (chars / 4) as u64;
    let cost = model
        .starts_with("jev-")
        .then(|| tokens as f64 * JEV_INPUT_RATE / 1_000_000.0);
    (tokens, cost)
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
                p.line(p.paint(
                    "nothing pending — every included document is scored",
                    Ink::Dim,
                ))?;
            } else {
                let mut figures = vec![
                    (self.scored as u64, "scored"),
                    (self.excluded as u64, "excluded"),
                    (self.review as u64, "held for review"),
                    (self.tagged as u64, "tagged"),
                    (self.kept as u64, "kept untagged"),
                ];
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
                    line.push_str(&format!(
                        " · {} committed · run {id}",
                        render::plural(self.committed, "exclusion", "exclusions")
                    ));
                }
                p.line(p.paint(&line, Ink::Dim))?;
            }

            if self.remaining > 0 {
                let text = format!(
                    "{} more pending — re-run to continue",
                    render::count(self.remaining as u64)
                );
                p.marked(Mark::Warn, p.paint(&text, Ink::Dim))?;
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
    use crate::workspace::DocumentId;
    use jiff::Timestamp;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A server that answers like Jev, on loopback. A yes-or-no question is 0.95; a
    /// choice gives 0.95 to `navigation` when the text says MENU and to `record`
    /// otherwise, so one document is junk and the other is not, and the test knows which.
    async fn fake_jev() -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 4096];
                    let body_start = loop {
                        let n = socket.read(&mut chunk).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                        if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            break at + 4;
                        }
                    };
                    let head = String::from_utf8_lossy(&buf[..body_start]).to_string();
                    let length: usize = head
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse().unwrap())
                        })
                        .unwrap_or(0);
                    while buf.len() < body_start + length {
                        let n = socket.read(&mut chunk).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                    }
                    let request: serde_json::Value =
                        serde_json::from_slice(&buf[body_start..]).unwrap();
                    let text = request["state"]["text"].as_str().unwrap_or_default();
                    let junk = text.contains("MENU");
                    let mut answers = serde_json::Map::new();
                    for (id, question) in request["questions"].as_object().unwrap() {
                        if question["type"] == "noul" {
                            let yes = if junk { 0.05 } else { 0.95 };
                            answers.insert(id.clone(), json!({ "noul": yes }));
                            continue;
                        }
                        let options: Vec<&String> =
                            question["criteria"].as_object().unwrap().keys().collect();
                        let want = if junk { "navigation" } else { "record" };
                        let pick = options
                            .iter()
                            .find(|o| o.as_str() == want)
                            .unwrap_or(&options[0]);
                        let rest = 0.05 / (options.len() - 1) as f64;
                        let probabilities: serde_json::Map<String, serde_json::Value> = options
                            .iter()
                            .map(|o| ((*o).clone(), json!(if o == pick { 0.95 } else { rest })))
                            .collect();
                        answers.insert(id.clone(), json!({ "choice": pick, "probabilities": probabilities, "confidence": 0.9 }));
                    }
                    let body = json!({ "model": "jev-test", "answers": answers, "usage": { "input_tokens": 100, "output_tokens": 4 } }).to_string();
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        format!("http://{addr}/v1/systemone")
    }

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
    /// nothing, a run scores, commits the gate's exclusion, and leaves nothing pending.
    #[tokio::test]
    async fn a_run_scores_what_is_pending_and_commits_the_gate() {
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
        assert!(
            !ctx.store.workspace_runs_path().exists(),
            "a dry run writes nothing"
        );

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
        assert!(
            !ctx.store.workspace_runs_path().exists(),
            "a preview writes nothing"
        );
        assert!(!ctx.store.workspace_decisions_path().exists());

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
        assert_eq!(run.committed, 1);
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
        assert_eq!(run.questions, ["page_kind", "laws"]);

        let decisions = std::fs::read_to_string(ctx.store.workspace_decisions_path()).unwrap();
        assert_eq!(decisions.lines().count(), 1);
        assert!(decisions.contains("example.gov/menu"));

        // Search and embed both read the exclusion: the menu's chunks are gone from the
        // hash list `embed` builds its work list from.
        let index = Index::open(ctx.store.index_path()).unwrap();
        let hashes = index.chunk_hashes().unwrap();
        let texts = index.chunk_texts(&hashes).unwrap();
        assert!(texts.iter().any(|t| t.contains("Budget hearing")));
        assert!(!texts.iter().any(|t| t.contains("Quick Links")));
        drop(index);

        // Nothing pending: the record is answered, the menu is excluded.
        let questions = ws.questions().unwrap();
        let asked: Vec<Question> = questions
            .iter()
            .filter(|q| q.id == "page_kind" || q.id == "laws")
            .cloned()
            .collect();
        assert!(
            ws.pending_documents(&asked, None, false)
                .unwrap()
                .is_empty()
        );
        let again = classify(
            &ctx,
            args(&["page_kind", "laws"]),
            &Progress::none(),
            &Cancel::none(),
        )
        .await
        .unwrap();
        assert_eq!(again.selected, 0);
        assert!(again.run_id.is_none(), "nothing to send is not a run");

        // A question nobody has asked yet puts the record back in the queue, not the junk.
        let budget: Vec<Question> = questions
            .iter()
            .filter(|q| q.id == "budget")
            .cloned()
            .collect();
        let pending = ws.pending_documents(&budget, None, false).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id.resource, "https://example.gov/budget");

        // One document by its address, scored regardless of pending.
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
        assert_eq!(one.committed, 0, "a tag is not a usage decision");
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
        assert!(!ctx.store.workspace_runs_path().exists());
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
    fn the_estimate_counts_text_at_the_sampling_limit_plus_the_questions() {
        let doc = |chars: usize| PendingDocument {
            id: DocumentId {
                source: "s".into(),
                resource: "r".into(),
                derived_sha: "d".into(),
            },
            chars,
        };
        let (tokens, cost) = estimate(&[doc(4_000), doc(4_000)], &[], "jev-test");
        assert_eq!(tokens, 2_000);
        assert!((cost.unwrap() - 2_000.0 * JEV_INPUT_RATE / 1e6).abs() < 1e-12);
        let (capped, _) = estimate(&[doc(10 * MAX_TEXT_BYTES)], &[], "jev-test");
        assert_eq!(capped, (MAX_TEXT_BYTES / 4) as u64);
        let with_questions = estimate(&[doc(4_000)], &workspace::default_questions(), "jev-test").0;
        assert!(with_questions > 1_000);
        assert_eq!(estimate(&[doc(4_000)], &[], "other-model").1, None);
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
            remaining: 30,
            scored: 118,
            failed: 2,
            excluded: 40,
            review: 10,
            tagged: 50,
            kept: 18,
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
            committed: 40,
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
        assert!(out.contains("40 exclusions committed"), "{out}");
        assert!(out.contains("30 more pending"), "{out}");
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
