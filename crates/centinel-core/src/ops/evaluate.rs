//! `evaluate` — how the classifier's answers hold up against the people who checked them.
//!
//! The review tool writes `workspace/reviews.jsonl`, one line per document a person read.
//! This op reads it back beside the runs ledger and says, per saved question, how often the
//! current policy's decision matched the person's, and for a yes-or-no question which
//! threshold would have matched most. It is the measurement the agent loop tunes against:
//! read this, change a wording or a threshold, run `classify --preview` on the reviewed
//! documents, read this again.
//!
//! Nothing here is sent anywhere, and the `--json` form is the one an agent reads.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::prelude::*;
use crate::workspace::{Evaluation, Workspace};

#[derive(Clone, Debug, Default, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct EvaluateArgs {}

/// Compare the classifier's answers with the review verdicts, per question.
#[op(group = "corpus")]
pub async fn evaluate(ctx: &Ctx, _args: EvaluateArgs) -> anyhow::Result<Evaluation> {
    Workspace::new(&ctx.store).evaluation()
}

fn percent(value: Option<f64>) -> String {
    match value {
        Some(v) => format!("{:.0}%", v * 100.0),
        None => "—".into(),
    }
}

/// One row per question, then where the choices went wrong, then what people asked for.
impl Render for Evaluation {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        let aside = format!(
            "{} · {}",
            render::plural(self.reviews, "review", "reviews"),
            render::plural(self.documents, "document", "documents")
        );
        p.title("classifier evaluation", &aside)?;
        if self.reviews == 0 {
            p.line(p.paint("No reviews yet.", Ink::Dim))?;
            return p.note("centinel web — Review");
        }
        p.nest(|p| {
            let mut table = Table::new(&[
                ("question", Align::Left),
                ("kind", Align::Left),
                ("compared", Align::Right),
                ("agree", Align::Right),
                ("at", Align::Right),
                ("suggest", Align::Right),
                ("precision", Align::Right),
                ("recall", Align::Right),
            ]);
            for q in &self.questions {
                let suggest = match q.suggested_threshold {
                    Some(t) if (t - q.threshold).abs() > f64::EPSILON => {
                        Cell::new(format!("{t:.2}"), Ink::Bold)
                    }
                    Some(_) => Cell::dim("same"),
                    None => Cell::dim("—"),
                };
                table.push(vec![
                    Cell::new(&q.id, Ink::Label),
                    Cell::dim(&q.kind),
                    if q.compared == 0 {
                        Cell::dim("—")
                    } else {
                        Cell::plain(render::count(q.compared as u64))
                    },
                    Cell::plain(percent(q.agreement)),
                    Cell::plain(format!("{:.2}", q.threshold)),
                    suggest,
                    Cell::plain(percent(q.precision)),
                    Cell::plain(percent(q.recall)),
                ]);
            }
            p.table(&table)?;

            let unscored: usize = self.questions.iter().map(|q| q.unscored).sum();
            if unscored > 0 {
                let text = format!(
                    "{} reviewed before the model scored them at the current version — run `centinel classify` and look again",
                    render::plural(unscored, "verdict was", "verdicts were")
                );
                p.marked(Mark::Warn, p.paint(&text, Ink::Dim))?;
            }

            for q in self.questions.iter().filter(|q| !q.confusion.is_empty()) {
                p.section(&format!("{} — people said → model said", q.id))?;
                for (human, models) in &q.confusion {
                    for (model, count) in models {
                        let line = format!(
                            "{human} → {model} ×{count}{}",
                            if human == model { "" } else { "  ✗" }
                        );
                        p.line(p.paint(&line, if human == model { Ink::Dim } else { Ink::Plain }))?;
                    }
                }
            }

            if !self.proposed.is_empty() {
                p.section("proposed tags")?;
                let mut rows: Vec<_> = self.proposed.iter().collect();
                rows.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
                for (name, count) in rows {
                    p.line(format!("{name} ×{count}"))?;
                }
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::QuestionEvaluation;
    use std::collections::BTreeMap;

    fn rendered(report: &Evaluation) -> String {
        let mut buf = Vec::new();
        {
            let mut p = Painter::new(&mut buf, false, 110);
            report.render(&mut p).unwrap();
        }
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn no_reviews_points_at_the_review_tool() {
        let out = rendered(&Evaluation::default());
        assert!(out.contains("No reviews yet"), "{out}");
        assert!(out.contains("Review"), "{out}");
    }

    #[test]
    fn the_table_shows_agreement_suggestions_and_where_choices_went_wrong() {
        let report = Evaluation {
            reviews: 12,
            documents: 11,
            questions: vec![
                QuestionEvaluation {
                    id: "laws".into(),
                    version: 1,
                    kind: "yes/no".into(),
                    compared: 10,
                    unscored: 1,
                    agreement: Some(0.8),
                    threshold: 0.8,
                    suggested_threshold: Some(0.65),
                    precision: Some(1.0),
                    recall: Some(0.6),
                    ..Default::default()
                },
                QuestionEvaluation {
                    id: "page_kind".into(),
                    version: 1,
                    kind: "choice".into(),
                    compared: 4,
                    agreement: Some(0.75),
                    threshold: 0.9,
                    confusion: BTreeMap::from([(
                        "record".to_string(),
                        BTreeMap::from([("record".to_string(), 3), ("navigation".to_string(), 1)]),
                    )]),
                    ..Default::default()
                },
            ],
            proposed: BTreeMap::from([("ordinance amendment".to_string(), 2)]),
        };
        let out = rendered(&report);
        assert!(out.contains("laws") && out.contains("80%"), "{out}");
        assert!(
            out.contains("0.65"),
            "the suggested threshold is shown: {out}"
        );
        assert!(out.contains("record → navigation ×1"), "{out}");
        assert!(out.contains("ordinance amendment ×2"), "{out}");
        assert!(out.contains("1 verdict was reviewed before"), "{out}");
    }
}
