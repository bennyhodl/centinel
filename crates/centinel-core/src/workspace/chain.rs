//! The question chain: which questions a document is asked, given its answers.
//!
//! A question whose `when` names a tag is a follow-up. It is asked of a document only when
//! the document's answer to the question that owns the tag — its parent — landed there,
//! and the parent was itself asked. A question with no `when` is a root, asked of every
//! document. Every path that needs to know whether a question reaches a document asks
//! [`Chain`]: the runner choosing what to send next, the fold over the runs ledger choosing
//! which stored answers still count, the projection listing the follow-ups a document
//! still owes, and the estimate before a run.
//!
//! "Landed" is the answer itself, not the policy: a choice lands on its likeliest option, a
//! yes-or-no question on yes at even odds or better, and a person's verdict beats both.
//! Moving a threshold therefore never changes which follow-ups apply; a new answer does.

use std::collections::{BTreeMap, HashMap, HashSet};

use anyhow::bail;

use super::{Question, QuestionKind, Verdict, option_key};

/// A person's verdicts on one document, by question id.
pub(super) type Verdicts = BTreeMap<String, Verdict>;

/// The question id an answer key belongs to: `laws`, `page_kind:record`, and either one
/// with an `@version` suffix all belong to their first segment. Ids and option ids hold
/// only letters, numbers, and underscores, so neither separator can be part of one.
pub(super) fn question_of(key: &str) -> &str {
    key.split([':', '@']).next().unwrap_or(key)
}

/// The tags a question's answers can land on, spelled the way `when` names them: a
/// yes-or-no question's id for its yes, `question:option` for each option of a choice. A
/// no is no tag, so nothing can follow it.
fn tags(question: &Question) -> Vec<String> {
    match question.kind {
        QuestionKind::Noul => vec![question.id.clone()],
        QuestionKind::Choice => question
            .options
            .iter()
            .map(|option| option_key(&question.id, &option.id))
            .collect(),
    }
}

/// The tag a document's answer to `question` landed on. `answers` are keyed bare, as a run
/// result holds them. A person's verdict wins; otherwise a choice lands on its likeliest
/// option, the first in order on a tie, and a yes-or-no question on yes at 0.5 or above.
/// `None` when the question is unanswered or the answer is no.
fn landed(
    question: &Question,
    answers: &BTreeMap<String, f64>,
    verdict: Option<&Verdict>,
) -> Option<String> {
    if let Some(verdict) = verdict {
        return match question.kind {
            QuestionKind::Noul => verdict.yes()?.then(|| question.id.clone()),
            QuestionKind::Choice => verdict
                .option()
                .filter(|chosen| question.options.iter().any(|o| o.id == *chosen))
                .map(|chosen| option_key(&question.id, chosen)),
        };
    }
    match question.kind {
        QuestionKind::Noul => (*answers.get(&question.id)? >= 0.5).then(|| question.id.clone()),
        QuestionKind::Choice => {
            let mut best: Option<(&str, f64)> = None;
            for option in &question.options {
                if let Some(&score) = answers.get(&option_key(&question.id, &option.id))
                    && best.is_none_or(|(_, top)| score > top)
                {
                    best = Some((option.id.as_str(), score));
                }
            }
            best.map(|(option, _)| option_key(&question.id, option))
        }
    }
}

/// One question set read as a tree.
pub(super) struct Chain<'q> {
    questions: &'q [Question],
    /// The question each tag is an answer of.
    owners: HashMap<String, &'q Question>,
    /// Each question's version, for reading versioned answer keys.
    versions: HashMap<&'q str, u64>,
}

impl<'q> Chain<'q> {
    pub(super) fn new(questions: &'q [Question]) -> Self {
        Self {
            questions,
            owners: questions
                .iter()
                .flat_map(|question| tags(question).into_iter().map(move |tag| (tag, question)))
                .collect(),
            versions: questions
                .iter()
                .map(|question| (question.id.as_str(), question.version))
                .collect(),
        }
    }

    /// The question whose answer `tag` is.
    pub(super) fn owner(&self, tag: &str) -> Option<&'q Question> {
        self.owners.get(tag).copied()
    }

    /// Every `when` names an answer of a question in this set, and no chain comes back to
    /// where it started. Checked when a set is saved, so a chain that cannot be followed is
    /// refused with its name rather than asked of nothing.
    pub(super) fn validate(&self) -> anyhow::Result<()> {
        for question in self.questions {
            let Some(tag) = &question.when else {
                continue;
            };
            if self.owner(tag).is_none() {
                bail!(
                    "question `{}` follows `{tag}`, which no question in the set answers; name \
                     a choice's option as `question:option` or a yes-or-no question's yes by its id",
                    question.id
                );
            }
            let mut current = question;
            for _ in 0..self.questions.len() {
                let Some(parent) = current.when.as_deref().and_then(|tag| self.owner(tag)) else {
                    break;
                };
                if parent.id == question.id {
                    bail!(
                        "question `{}` follows `{tag}`, which leads back to its own answers; a \
                         chain has to start at a question asked of every document",
                        question.id
                    );
                }
                current = parent;
            }
        }
        Ok(())
    }

    /// Whether `question` is asked of a document with these answers: a root always; a
    /// follow-up when its parent landed on its `when` and the parent was reached too. The
    /// walk takes at most as many steps as there are questions, so a chain that never meets
    /// a root — a cycle in a file edited by hand — reaches nothing.
    pub(super) fn reaches(
        &self,
        question: &Question,
        answers: &BTreeMap<String, f64>,
        verdicts: Option<&Verdicts>,
    ) -> bool {
        let mut current = question;
        for _ in 0..=self.questions.len() {
            let Some(tag) = &current.when else {
                return true;
            };
            let Some(parent) = self.owner(tag) else {
                return false;
            };
            let verdict = verdicts.and_then(|v| v.get(&parent.id));
            if landed(parent, answers, verdict).as_ref() != Some(tag) {
                return false;
            }
            current = parent;
        }
        false
    }

    /// The questions of `asking` to send next: those not yet `asked` that reach the
    /// document under the answers so far. A run calls this until it comes back empty, so
    /// roots go first and each follow-up waits for the answer it hangs off.
    pub(super) fn next<'a>(
        &self,
        asking: &'a [Question],
        answers: &BTreeMap<String, f64>,
        verdicts: Option<&Verdicts>,
        asked: &HashSet<String>,
    ) -> Vec<&'a Question> {
        asking
            .iter()
            .filter(|question| {
                !asked.contains(&question.id) && self.reaches(question, answers, verdicts)
            })
            .collect()
    }

    /// The answers in `stored` at each question's current version, keyed bare.
    pub(super) fn current(&self, stored: &BTreeMap<String, f64>) -> BTreeMap<String, f64> {
        stored
            .iter()
            .filter_map(|(key, score)| {
                let (bare, version) = key.rsplit_once('@')?;
                let current = self.versions.get(question_of(bare))?;
                (version.parse::<u64>().ok()? == *current).then(|| (bare.to_owned(), *score))
            })
            .collect()
    }

    /// Drops a document's stored answers to every follow-up the chain no longer reaches.
    /// `stored` is keyed `key@version`, as the runs ledger folds it.
    ///
    /// This is the one place answers are removed. The fold applies it after every run
    /// result and every review, in the order they happened, so a parent whose answer
    /// changes — by Jev or by a person's verdict, which counts as the parent's answer from
    /// the moment it is given — takes its follow-ups' answers with it. They are asked again
    /// if the parent comes back to their tag, never revived. A reworded parent has no
    /// answer at its new version until it is asked again.
    pub(super) fn retain_reached(
        &self,
        stored: &mut BTreeMap<String, f64>,
        verdicts: Option<&Verdicts>,
    ) {
        if self
            .questions
            .iter()
            .all(|question| question.when.is_none())
        {
            return;
        }
        let answers = self.current(stored);
        let unreached: HashSet<&str> = self
            .questions
            .iter()
            .filter(|question| !self.reaches(question, &answers, verdicts))
            .map(|question| question.id.as_str())
            .collect();
        if !unreached.is_empty() {
            stored.retain(|key, _| !unreached.contains(question_of(key)));
        }
    }

    /// The follow-ups a document is owed: reached by its answers, and not answered at
    /// their current version. With the roots it has not answered, this is what makes a
    /// document pending.
    pub(super) fn open(
        &self,
        stored: &BTreeMap<String, f64>,
        verdicts: Option<&Verdicts>,
    ) -> Vec<&'q Question> {
        let answers = self.current(stored);
        self.questions
            .iter()
            .filter(|question| {
                question.when.is_some()
                    && !answers.keys().any(|key| question_of(key) == question.id)
                    && self.reaches(question, &answers, verdicts)
            })
            .collect()
    }

    /// The share of documents a run is expected to ask `question` of: one for a root; for a
    /// follow-up, `share` of its tag times its parent's own reach. A tag `share` knows
    /// nothing about counts as every document, so an estimate built on this errs high.
    pub(super) fn reach(&self, question: &Question, share: impl Fn(&str) -> Option<f64>) -> f64 {
        let mut reach = 1.0;
        let mut current = question;
        for _ in 0..=self.questions.len() {
            let Some(tag) = &current.when else {
                break;
            };
            reach *= share(tag).unwrap_or(1.0);
            let Some(parent) = self.owner(tag) else {
                break;
            };
            current = parent;
        }
        reach
    }
}
