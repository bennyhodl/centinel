//! `questions` — the saved classifier questions, and the shipped set to fall back on.
//!
//! The questions live in `workspace/questions.jsonl`, which the web Classify view edits.
//! This op is the terminal's view of that file: what is saved, at which version, under
//! what policy — the set a `classify` run would send. With `--add-defaults` it appends
//! every shipped question the file lacks, which is how a store that saved its own set
//! before the defaults existed, or deleted one, gets them back without the browser.
//!
//! Adding never overwrites: a saved question with the same id as a default keeps its
//! wording and policy, because it is the operator's.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::prelude::*;
use crate::workspace::{Question, QuestionAction, QuestionKind, Workspace, default_questions};

#[derive(Clone, Debug, Default, clap::Args, Serialize, Deserialize, JsonSchema)]
pub struct QuestionsArgs {
    /// Append every shipped default the saved set does not have, and save.
    #[arg(long)]
    #[serde(default)]
    pub add_defaults: bool,
}

/// One saved question, as a line of a table.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct QuestionSummary {
    pub id: String,
    /// `yes/no` or `choice`.
    pub kind: String,
    pub version: u64,
    /// Options of a choice; zero for a yes-or-no question.
    pub options: usize,
    /// The policy in a few words: what acts, at what score, and the review band.
    pub policy: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct QuestionsReport {
    /// The file that holds them.
    pub path: String,
    pub questions: Vec<QuestionSummary>,
    /// Ids appended by `--add-defaults`, in the order they were added.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub added: Vec<String>,
}

/// List the saved classifier questions, or restore the shipped defaults among them.
#[op(reach = "operator", group = "corpus")]
pub async fn questions(ctx: &Ctx, args: QuestionsArgs) -> anyhow::Result<QuestionsReport> {
    let ws = Workspace::new(&ctx.store);
    let mut saved = ws.questions()?;
    let mut added = Vec::new();
    if args.add_defaults {
        for question in default_questions() {
            if !saved.iter().any(|q| q.id == question.id) {
                added.push(question.id.clone());
                saved.push(question);
            }
        }
        if !added.is_empty() {
            saved = ws.save_questions(saved)?;
        }
    }
    Ok(QuestionsReport {
        path: ctx.store.workspace_questions_path().display().to_string(),
        questions: saved.iter().map(summary).collect(),
        added,
    })
}

fn summary(question: &Question) -> QuestionSummary {
    QuestionSummary {
        id: question.id.clone(),
        kind: match question.kind {
            QuestionKind::Noul => "yes/no",
            QuestionKind::Choice => "choice",
        }
        .into(),
        version: question.version,
        options: question.options.len(),
        policy: policy(question),
    }
}

/// A policy in a few words, the way the web page shortens it.
fn policy(question: &Question) -> String {
    let at = format!("{:.2}", question.threshold);
    let band = match question.review {
        Some(review) => format!(" · review from {review:.2}"),
        None => String::new(),
    };
    match question.kind {
        QuestionKind::Noul => match question.action {
            QuestionAction::Keep => "score only".to_string(),
            QuestionAction::Exclude => format!("exclude at {at}{band}"),
            QuestionAction::Tag => format!("tag at {at}{band}"),
        },
        QuestionKind::Choice => {
            let excluded = question
                .options
                .iter()
                .filter(|o| o.action == QuestionAction::Exclude)
                .count();
            let tagged = question
                .options
                .iter()
                .filter(|o| o.action == QuestionAction::Tag)
                .count();
            let mut parts = Vec::new();
            if excluded > 0 {
                parts.push(format!(
                    "{} excluded at {at}",
                    render::plural(excluded, "junk kind", "junk kinds")
                ));
            }
            if tagged > 0 {
                parts.push(format!(
                    "{} tagged at {at}",
                    render::plural(tagged, "kind", "kinds")
                ));
            }
            if parts.is_empty() {
                format!("score only{band}")
            } else {
                format!("{}{band}", parts.join(" · "))
            }
        }
    }
}

/// One row per question, then where they live, then what was added.
impl Render for QuestionsReport {
    fn render(&self, p: &mut Painter<'_>) -> std::io::Result<()> {
        if self.questions.is_empty() {
            p.line(p.paint("No questions saved.", Ink::Dim))?;
            return p.note("centinel questions --add-defaults");
        }
        let mut table = Table::new(&[
            ("question", Align::Left),
            ("kind", Align::Left),
            ("v", Align::Right),
            ("options", Align::Right),
            ("policy", Align::Left),
        ]);
        for q in &self.questions {
            table.push(vec![
                Cell::new(&q.id, Ink::Label),
                Cell::dim(&q.kind),
                Cell::plain(q.version.to_string()),
                if q.options == 0 {
                    Cell::dim("—")
                } else {
                    Cell::plain(q.options.to_string())
                },
                Cell::plain(&q.policy),
            ]);
        }
        p.table(&table)?;
        p.blank()?;
        p.line(p.paint(&self.path, Ink::Dim))?;
        if !self.added.is_empty() {
            let text = format!(
                "added {}: {}",
                render::plural(self.added.len(), "default", "defaults"),
                self.added.join(", ")
            );
            p.marked(Mark::Ok, p.paint(&text, Ink::Dim))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    fn mine() -> Question {
        Question {
            id: "mine".into(),
            instructions: "Does `text` mention a bridge?".into(),
            version: 0,
            kind: QuestionKind::Noul,
            options: Vec::new(),
            threshold: 0.7,
            review: None,
            action: QuestionAction::Tag,
        }
    }

    #[tokio::test]
    async fn lists_the_saved_set_and_adds_only_the_missing_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(Store::open(dir.path()).await.unwrap());
        let ws = Workspace::new(&ctx.store);

        // A set saved before the defaults existed: one of theirs, one of ours.
        let mut gate = default_questions().remove(0);
        gate.threshold = 0.95;
        ws.save_questions(vec![gate, mine()]).unwrap();

        let listed = questions(&ctx, QuestionsArgs::default()).await.unwrap();
        assert_eq!(listed.questions.len(), 2);
        assert_eq!(listed.questions[0].id, "page_kind");
        assert_eq!(listed.questions[0].kind, "choice");
        assert!(
            listed.questions[0].policy.contains("0.95"),
            "{}",
            listed.questions[0].policy
        );
        assert_eq!(listed.questions[1].policy, "tag at 0.70");
        assert!(listed.added.is_empty());

        let restored = questions(&ctx, QuestionsArgs { add_defaults: true })
            .await
            .unwrap();
        let defaults = default_questions();
        assert_eq!(
            restored.added.len(),
            defaults.len() - 1,
            "the gate was already there"
        );
        assert!(!restored.added.contains(&"page_kind".to_string()));
        assert_eq!(restored.questions.len(), defaults.len() + 1);
        assert_eq!(restored.questions[0].id, "page_kind");
        assert_eq!(restored.questions[1].id, "mine", "the saved order is kept");

        let saved = ws.questions().unwrap();
        assert_eq!(
            saved[0].threshold, 0.95,
            "a saved question keeps its policy"
        );
        assert!(saved.iter().all(|q| q.version >= 1));

        let again = questions(&ctx, QuestionsArgs { add_defaults: true })
            .await
            .unwrap();
        assert!(again.added.is_empty(), "adding twice adds nothing");
    }

    #[test]
    fn a_policy_reads_in_a_few_words() {
        let defaults = default_questions();
        let gate = defaults.iter().find(|q| q.id == "page_kind").unwrap();
        assert_eq!(
            policy(gate),
            "3 junk kinds excluded at 0.90 · review from 0.50"
        );
        let laws = defaults.iter().find(|q| q.id == "laws").unwrap();
        assert_eq!(policy(laws), "tag at 0.80 · review from 0.50");
        let kinds = defaults.iter().find(|q| q.id == "record_type").unwrap();
        assert_eq!(policy(kinds), "10 kinds tagged at 0.75 · review from 0.50");
    }
}
