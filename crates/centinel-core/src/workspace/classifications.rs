//! Live classification state: `workspace/workspace.db`.
//!
//! What every classifier run answered, what a person said about it, and what the current
//! policy makes of both, in one SQLite file under `workspace/`. Durable, because
//! `centinel.db` is disposable: an index rebuild must lose no answer and no verdict.
//! `centinel.db` attaches this file as `ws` and reads it; nothing else writes it.
//!
//! Three kinds of table. History, never rewritten: `run`, `run_input`, `run_result`,
//! `review_history`. The latest word per document: `answer` (one row per
//! `question@version` or `question:option@version`, overwritten by a later run),
//! `review` (the latest whole review), `usage_override` (a person's include or exclude).
//! Materialized from those under the current questions by [`materialize`] and by nothing
//! else: `tag`, `excluded`, `score`, `open_follow_up`. Beside them, `required_question`
//! lists the roots every document owes, and `vector_sync` the documents whose Lance rows
//! no longer say what `tag` and `excluded` say.
//!
//! Every write that moves a document's answers or verdicts is one transaction: the
//! history row, the latest-word rows, the chain's deletions, and the four materialized
//! tables. A crash loses at most the result in flight, and history and live state never
//! disagree.
//!
//! ## Policy changes
//!
//! A threshold, an action, a `when`, or a reworded question changes what the same answers
//! mean. [`Classifications::questions_changed`] rewrites the materialized rows of every
//! document that holds an answer to a changed question, without sending anything to Jev.
//! The questions file's fingerprint is recorded in `meta` in the transaction that finishes
//! the rewrite, so a crash halfway, or a questions file edited by hand, shows as a
//! mismatch the next time the file is opened and the rewrite runs again.
//!
//! ## The ledgers this replaced
//!
//! `workspace/runs.jsonl`, `reviews.jsonl` and `decisions.jsonl` are read once, by
//! [`Classifications::open`] on a store that has them, replayed through the same
//! per-event path a live run takes, and never written again. They stay on disk.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use anyhow::Context as _;
use jiff::Timestamp;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::Deserialize;
use serde_json::Value;

use super::chain::{Chain, Verdicts, question_of};
use super::{
    ClassifierRun, DETAIL_INPUTS, DocumentId, Question, QuestionKind, Review, RunResult,
    RunSummary, answers_for, decide, human_tags, option_key, question_version, read_json_lines,
    saved_questions_at,
};
use crate::store::Store;

/// How long a connection waits on a lock held by another process. A rematerialization
/// over the corpus holds the write lock for a few seconds at a time; a reader that gave
/// up sooner would show a browser "database is locked" for a page that did nothing wrong.
pub(crate) const BUSY_TIMEOUT: Duration = Duration::from_secs(60);

/// Documents rewritten per transaction by a rematerialization, so a reader is never
/// locked out for longer than one chunk takes.
const REMATERIALIZE_CHUNK: usize = 1_000;

/// The saved questions read as one tree: what [`materialize`] decides under.
struct Policy<'q> {
    questions: &'q [Question],
    chain: Chain<'q>,
}

impl<'q> Policy<'q> {
    fn new(questions: &'q [Question]) -> Self {
        Self {
            questions,
            chain: Chain::new(questions),
        }
    }
}

/// What a document's Lance rows carry: the tags on it and whether it is excluded.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DocState {
    pub tags: Vec<String>,
    pub excluded: bool,
}

/// One document whose vector rows are out of date, at the revision the metadata reached.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaleDocument {
    pub id: DocumentId,
    pub revision: i64,
    pub state: DocState,
}

/// A connection to `workspace/workspace.db`, created and brought up to date on open.
///
/// It decides under the questions file as it stands at each write, not under whatever
/// questions a caller held when it started: a run that is still going when someone moves a
/// threshold decides its next document under the new one, and a rematerialization that
/// began under older questions finishes under the newer. The file is read again only when
/// its fingerprint moves.
pub(crate) struct Classifications {
    conn: Connection,
    questions_path: PathBuf,
    /// The questions last read, under the fingerprint they were read at.
    cached: RefCell<Option<(String, Arc<Vec<Question>>)>>,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS {s}meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS {s}run (
  id TEXT PRIMARY KEY, created_at TEXT NOT NULL, completed_at TEXT, status TEXT NOT NULL,
  error TEXT, model TEXT NOT NULL, evaluation_date TEXT NOT NULL, questions TEXT NOT NULL,
  settings TEXT NOT NULL, scope TEXT, continue_from TEXT, document_count INTEGER NOT NULL,
  usage_documents INTEGER NOT NULL DEFAULT 0, input_tokens INTEGER, output_tokens INTEGER,
  cost_usd REAL, duration_ms INTEGER, errors INTEGER NOT NULL DEFAULT 0,
  scored INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS {s}run_by_created ON run(created_at);
CREATE INDEX IF NOT EXISTS {s}run_by_continue ON run(continue_from);
CREATE TABLE IF NOT EXISTS {s}run_input (
  run_id TEXT NOT NULL, ordinal INTEGER NOT NULL,
  source TEXT NOT NULL, resource TEXT NOT NULL, derived_sha TEXT NOT NULL,
  PRIMARY KEY(run_id, ordinal)
);
CREATE TABLE IF NOT EXISTS {s}run_result (
  run_id TEXT NOT NULL, source TEXT NOT NULL, resource TEXT NOT NULL, derived_sha TEXT NOT NULL,
  result TEXT NOT NULL, error TEXT, input_tokens INTEGER, output_tokens INTEGER, at TEXT NOT NULL,
  PRIMARY KEY(run_id, source, resource, derived_sha)
);
CREATE INDEX IF NOT EXISTS {s}run_result_by_time ON run_result(run_id, at);
CREATE INDEX IF NOT EXISTS {s}run_result_by_document ON run_result(source, resource, derived_sha);
CREATE TABLE IF NOT EXISTS {s}answer (
  source TEXT NOT NULL, resource TEXT NOT NULL, derived_sha TEXT NOT NULL,
  key TEXT NOT NULL, score REAL NOT NULL, run_id TEXT NOT NULL, at TEXT NOT NULL,
  PRIMARY KEY(source, resource, derived_sha, key)
);
CREATE INDEX IF NOT EXISTS {s}answer_by_key ON answer(key);
CREATE TABLE IF NOT EXISTS {s}review (
  source TEXT NOT NULL, resource TEXT NOT NULL, derived_sha TEXT NOT NULL,
  verdicts TEXT NOT NULL, proposed TEXT NOT NULL, note TEXT NOT NULL, reviewer TEXT NOT NULL,
  at TEXT NOT NULL,
  PRIMARY KEY(source, resource, derived_sha)
);
CREATE TABLE IF NOT EXISTS {s}review_history (
  id INTEGER PRIMARY KEY,
  source TEXT NOT NULL, resource TEXT NOT NULL, derived_sha TEXT NOT NULL,
  verdicts TEXT NOT NULL, proposed TEXT NOT NULL, note TEXT NOT NULL, reviewer TEXT NOT NULL,
  at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS {s}usage_override (
  source TEXT NOT NULL, resource TEXT NOT NULL, derived_sha TEXT NOT NULL,
  excluded INTEGER NOT NULL, reason TEXT NOT NULL, at TEXT NOT NULL,
  PRIMARY KEY(source, resource, derived_sha)
);
CREATE TABLE IF NOT EXISTS {s}tag (
  source TEXT NOT NULL, resource TEXT NOT NULL, derived_sha TEXT NOT NULL,
  tag TEXT NOT NULL, by TEXT NOT NULL,
  PRIMARY KEY(source, resource, derived_sha, tag, by)
);
CREATE INDEX IF NOT EXISTS {s}tag_by_tag ON tag(tag);
CREATE TABLE IF NOT EXISTS {s}excluded (
  source TEXT NOT NULL, resource TEXT NOT NULL, derived_sha TEXT NOT NULL,
  by TEXT NOT NULL, reason TEXT,
  PRIMARY KEY(source, resource, derived_sha)
);
CREATE INDEX IF NOT EXISTS {s}excluded_by_source ON excluded(source);
CREATE TABLE IF NOT EXISTS {s}score (
  source TEXT NOT NULL, resource TEXT NOT NULL, derived_sha TEXT NOT NULL,
  question TEXT NOT NULL, value REAL NOT NULL,
  PRIMARY KEY(source, resource, derived_sha, question)
);
CREATE INDEX IF NOT EXISTS {s}score_by_question ON score(question, value);
CREATE TABLE IF NOT EXISTS {s}open_follow_up (
  source TEXT NOT NULL, resource TEXT NOT NULL, derived_sha TEXT NOT NULL,
  question TEXT NOT NULL,
  PRIMARY KEY(source, resource, derived_sha, question)
);
CREATE TABLE IF NOT EXISTS {s}required_question (question TEXT PRIMARY KEY);
CREATE TABLE IF NOT EXISTS {s}vector_sync (
  source TEXT NOT NULL, resource TEXT NOT NULL, derived_sha TEXT NOT NULL,
  revision INTEGER NOT NULL,
  PRIMARY KEY(source, resource, derived_sha)
);
"#;

/// Creates the tables in schema `prefix` (`""` for a connection's main database, `"ws."`
/// for the attached one) when they do not exist.
///
/// `CREATE INDEX` names an index inside a schema by prefixing the index, not the table,
/// which is why the placeholder sits where it does.
fn create_tables(conn: &Connection, prefix: &str) -> anyhow::Result<()> {
    conn.execute_batch(&SCHEMA.replace("{s}", prefix))?;
    Ok(())
}

/// The three columns every per-document table is keyed by, as positional parameters
/// `?1`, `?2`, `?3`.
fn doc_params(doc: &DocumentId) -> (&str, &str, &str) {
    (&doc.source, &doc.resource, &doc.derived_sha)
}

const ON_DOCUMENT: &str = "source=?1 AND resource=?2 AND derived_sha=?3";

impl Classifications {
    /// Opens the database, creating it on first use, importing the JSONL ledgers once, and
    /// rematerializing when the questions file is not the one the live state was decided
    /// under.
    pub(crate) fn open(store: &Store) -> anyhow::Result<Self> {
        let path = store.workspace_db_path();
        std::fs::create_dir_all(store.workspace_dir())?;
        let conn =
            Connection::open(&path).with_context(|| format!("opening {}", path.display()))?;
        conn.busy_timeout(BUSY_TIMEOUT)?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")?;
        create_tables(&conn, "")?;
        let this = Self {
            conn,
            questions_path: store.workspace_questions_path(),
            cached: RefCell::new(None),
        };
        this.import_once(store)?;
        this.catch_up()?;
        Ok(this)
    }

    /// Attaches the database as `ws` to a connection on `centinel.db`, opening it first so
    /// an attach can never create an empty file that skips the import. The connection's
    /// lock wait becomes this file's, since it now waits on both.
    pub(crate) fn attach(conn: &Connection, store: &Store) -> anyhow::Result<()> {
        Self::open(store)?;
        conn.busy_timeout(BUSY_TIMEOUT)?;
        let path = store.workspace_db_path();
        conn.execute(
            "ATTACH DATABASE ?1 AS ws",
            [path.to_string_lossy().as_ref()],
        )?;
        Ok(())
    }

    /// Attaches an empty in-memory `ws` with the same tables. For an index that lives in
    /// memory and has no store around it.
    pub(crate) fn attach_in_memory(conn: &Connection) -> anyhow::Result<()> {
        conn.execute("ATTACH DATABASE ':memory:' AS ws", [])?;
        create_tables(conn, "ws.")
    }

    fn immediate(&self) -> anyhow::Result<Transaction<'_>> {
        Ok(Transaction::new_unchecked(
            &self.conn,
            TransactionBehavior::Immediate,
        )?)
    }

    fn meta(&self, key: &str) -> anyhow::Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM meta WHERE key=?1", [key], |r| r.get(0))
            .optional()?)
    }

    fn set_meta(conn: &Connection, key: &str, value: &str) -> anyhow::Result<()> {
        conn.execute(
            "INSERT INTO meta(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    // ── policy ────────────────────────────────────────────────────────────────

    /// The saved questions as the file holds them now, with the file's fingerprint. Every
    /// write asks for this inside its transaction, so what it decides under is never older
    /// than the file was when it took the write lock.
    fn saved(&self) -> anyhow::Result<(String, Arc<Vec<Question>>)> {
        loop {
            let fingerprint = file_fingerprint(&self.questions_path)?;
            if let Some((read_at, questions)) = self.cached.borrow().as_ref()
                && *read_at == fingerprint
            {
                return Ok((fingerprint, Arc::clone(questions)));
            }
            let questions = Arc::new(saved_questions_at(&self.questions_path)?);
            // Looked at on both sides of the read: a save that landed during it is read
            // again rather than cached under the older fingerprint.
            if file_fingerprint(&self.questions_path)? == fingerprint {
                *self.cached.borrow_mut() = Some((fingerprint.clone(), Arc::clone(&questions)));
                return Ok((fingerprint, questions));
            }
        }
    }

    /// Rematerializes everything when the questions file is not the one the live state was
    /// decided under: a save that crashed halfway, a file edited by hand, or a first open.
    fn catch_up(&self) -> anyhow::Result<()> {
        let (fingerprint, _) = self.saved()?;
        if self.meta("questions_fingerprint")?.as_deref() == Some(&fingerprint) {
            return Ok(());
        }
        let documents = self.documents_with_state()?;
        self.rematerialize(&documents)
    }

    /// Re-decides every document that holds an answer to a question whose policy, meaning,
    /// `when` or presence changed between `previous` and `next`, and every reviewed
    /// document, in chunks of [`REMATERIALIZE_CHUNK`]. Nothing is sent to Jev.
    ///
    /// A change to the chain's shape — a `when`, a question added or removed — can move
    /// what any document reaches, so that re-decides everything that holds state.
    pub(crate) fn questions_changed(
        &self,
        previous: &[Question],
        next: &[Question],
    ) -> anyhow::Result<usize> {
        let before: HashMap<&str, &Question> =
            previous.iter().map(|q| (q.id.as_str(), q)).collect();
        let after: HashMap<&str, &Question> = next.iter().map(|q| (q.id.as_str(), q)).collect();
        let shape_changed = before.len() != after.len()
            || before.keys().any(|id| !after.contains_key(id))
            || next
                .iter()
                .any(|q| before.get(q.id.as_str()).is_none_or(|b| b.when != q.when));
        let changed: HashSet<&str> = before
            .iter()
            .filter(|&(id, b)| after.get(id).is_none_or(|a| a != b))
            .map(|(id, _)| *id)
            .chain(
                next.iter()
                    .filter(|q| !before.contains_key(q.id.as_str()))
                    .map(|q| q.id.as_str()),
            )
            .collect();
        let documents = if shape_changed {
            self.documents_with_state()?
        } else {
            self.documents_touching(&changed)?
        };
        self.rematerialize(&documents)?;
        Ok(documents.len())
    }

    /// Every document with an answer, a review or an override.
    fn documents_with_state(&self) -> anyhow::Result<Vec<DocumentId>> {
        let mut stmt = self.conn.prepare(
            "SELECT source,resource,derived_sha FROM answer
             UNION SELECT source,resource,derived_sha FROM review
             UNION SELECT source,resource,derived_sha FROM usage_override",
        )?;
        let rows = stmt.query_map([], read_document)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Documents with an answer to one of `questions`, and every reviewed document.
    fn documents_touching(&self, questions: &HashSet<&str>) -> anyhow::Result<Vec<DocumentId>> {
        let mut found: HashSet<DocumentId> = HashSet::new();
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT source,resource,derived_sha,key FROM answer")?;
        let rows = stmt.query_map([], |r| Ok((read_document(r)?, r.get::<_, String>(3)?)))?;
        for row in rows {
            let (doc, key) = row?;
            if questions.contains(question_of(&key)) {
                found.insert(doc);
            }
        }
        let mut stmt = self
            .conn
            .prepare("SELECT source,resource,derived_sha FROM review")?;
        for row in stmt.query_map([], read_document)? {
            found.insert(row?);
        }
        Ok(found.into_iter().collect())
    }

    /// Rewrites `documents` under the saved questions — first the chain's deletions, so a
    /// follow-up a reworded or moved parent no longer reaches loses its answers for good,
    /// as it would have had the parent's answer moved in a run — then the roots every
    /// document owes, and last the fingerprint of the questions it finished under.
    ///
    /// Each chunk reads the questions inside its own write lock. A save that lands midway
    /// is what the remaining chunks decide under; the documents it changes are its own
    /// rematerialization's to rewrite, and that one starts after the save, so it writes
    /// after any chunk here that read the older file.
    fn rematerialize(&self, documents: &[DocumentId]) -> anyhow::Result<()> {
        for chunk in documents.chunks(REMATERIALIZE_CHUNK) {
            let tx = self.immediate()?;
            let (_, questions) = self.saved()?;
            let policy = Policy::new(&questions);
            for doc in chunk {
                prune_unreached(&tx, doc, &policy.chain)?;
                materialize(&tx, doc, &policy)?;
            }
            tx.commit()?;
        }
        let tx = self.immediate()?;
        let (fingerprint, questions) = self.saved()?;
        tx.execute("DELETE FROM required_question", [])?;
        for question in questions.iter().filter(|q| q.when.is_none()) {
            tx.execute(
                "INSERT INTO required_question(question) VALUES (?1)",
                [&question.id],
            )?;
        }
        Self::set_meta(&tx, "questions_fingerprint", &fingerprint)?;
        tx.commit()?;
        Ok(())
    }

    // ── runs ──────────────────────────────────────────────────────────────────

    /// Records a run as started, with its explicit inputs when it has them.
    pub(crate) fn insert_run(
        &self,
        run: &ClassifierRun,
        inputs: &[DocumentId],
    ) -> anyhow::Result<()> {
        let tx = self.immediate()?;
        tx.execute(
            "INSERT INTO run(id,created_at,completed_at,status,error,model,evaluation_date,questions,settings,
                             scope,continue_from,document_count,usage_documents,input_tokens,output_tokens,
                             cost_usd,duration_ms,errors,scored)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,0)",
            params![
                run.id,
                run.created_at,
                run.completed_at,
                run.status,
                run.error,
                run.model,
                run.evaluation_date,
                serde_json::to_string(&run.questions)?,
                serde_json::to_string(&run.settings)?,
                run.scope.as_ref().map(serde_json::to_string).transpose()?,
                run.continue_from,
                run.document_count as i64,
                run.usage_documents as i64,
                run.input_tokens.map(|n| n as i64),
                run.output_tokens.map(|n| n as i64),
                run.cost_usd,
                run.duration_ms.map(|n| n as i64),
                run.errors as i64,
            ],
        )?;
        {
            let mut insert = tx.prepare(
                "INSERT INTO run_input(run_id,ordinal,source,resource,derived_sha) VALUES (?1,?2,?3,?4,?5)",
            )?;
            for (ordinal, doc) in inputs.iter().enumerate() {
                insert.execute(params![
                    run.id,
                    ordinal as i64,
                    doc.source,
                    doc.resource,
                    doc.derived_sha
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Writes one scored document in one transaction: the result, the run's counters as
    /// the loop holds them, the answers under the versions the run asked, the chain's
    /// deletions, and the document's materialized rows under the questions saved now —
    /// which may not be the ones the run started with.
    ///
    /// A result's answers count whether or not it carries an error: a document stopped
    /// halfway down its chain keeps the levels it was answered, and the error says the
    /// chain did not finish.
    pub(crate) fn record_result(
        &self,
        run: &ClassifierRun,
        result: &RunResult,
        usage: Option<(u64, u64)>,
    ) -> anyhow::Result<()> {
        let doc = result.id();
        let at = Timestamp::now().to_string();
        let tx = self.immediate()?;
        let (_, saved) = self.saved()?;
        let policy = Policy::new(&saved);
        tx.execute(
            "INSERT OR REPLACE INTO run_result(run_id,source,resource,derived_sha,result,error,input_tokens,output_tokens,at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                run.id,
                doc.source,
                doc.resource,
                doc.derived_sha,
                serde_json::to_string(result)?,
                result.error,
                usage.map(|(i, _)| i as i64),
                usage.map(|(_, o)| o as i64),
                at,
            ],
        )?;
        update_run_counters(&tx, run)?;
        let versions: HashMap<&str, u64> = run
            .questions
            .iter()
            .map(|q| (q.id.as_str(), q.version))
            .collect();
        {
            let mut upsert = tx.prepare_cached(
                "INSERT INTO answer(source,resource,derived_sha,key,score,run_id,at) VALUES (?1,?2,?3,?4,?5,?6,?7)
                 ON CONFLICT(source,resource,derived_sha,key) DO UPDATE SET score=excluded.score, run_id=excluded.run_id, at=excluded.at",
            )?;
            for (key, score) in &result.answers {
                // `question:option` carries the version of its question.
                let Some(version) = versions.get(question_of(key)) else {
                    continue;
                };
                upsert.execute(params![
                    doc.source,
                    doc.resource,
                    doc.derived_sha,
                    question_version(key, *version),
                    score,
                    run.id,
                    at
                ])?;
            }
        }
        prune_unreached(&tx, &doc, &policy.chain)?;
        materialize(&tx, &doc, &policy)?;
        tx.commit()?;
        Ok(())
    }

    /// Writes the run's summary as it ends: its status, the error that stopped it, and
    /// the counters.
    pub(crate) fn finish_run(&self, run: &ClassifierRun) -> anyhow::Result<()> {
        let tx = self.immediate()?;
        update_run_counters(&tx, run)?;
        tx.execute(
            "UPDATE run SET status=?2, error=?3, completed_at=?4, cost_usd=?5 WHERE id=?1",
            params![
                run.id,
                run.status,
                run.error,
                run.completed_at,
                run.cost_usd
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// One run's summary, with the first [`DETAIL_INPUTS`] of its explicit inputs. A run
    /// left `running` by a process that is gone reads as `interrupted`; a legacy
    /// `committed` run as `completed`.
    pub(crate) fn run(
        &self,
        id: &str,
        active: &HashSet<String>,
    ) -> anyhow::Result<Option<ClassifierRun>> {
        let Some(mut run) = self
            .conn
            .query_row(
                "SELECT id,created_at,completed_at,status,error,model,evaluation_date,questions,settings,
                        scope,continue_from,document_count,usage_documents,input_tokens,output_tokens,
                        cost_usd,duration_ms,errors,scored
                 FROM run WHERE id=?1",
                [id],
                read_run,
            )
            .optional()?
        else {
            return Ok(None);
        };
        settle_status(&mut run, active);
        run.inputs = self.inputs(id, Some(DETAIL_INPUTS))?;
        Ok(Some(run))
    }

    /// The explicit inputs of a run, in order, up to `limit` of them.
    pub(crate) fn inputs(&self, id: &str, limit: Option<usize>) -> anyhow::Result<Vec<DocumentId>> {
        let mut stmt = self.conn.prepare(
            "SELECT source,resource,derived_sha FROM run_input WHERE run_id=?1 ORDER BY ordinal LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![id, limit.map_or(-1, |n| n as i64)], read_document)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// A page of run summaries, newest first, and how many there are.
    pub(crate) fn runs(
        &self,
        page: usize,
        page_size: usize,
        active: &HashSet<String>,
    ) -> anyhow::Result<(Vec<RunSummary>, usize)> {
        let total: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM run", [], |r| r.get(0))?;
        let mut stmt = self.conn.prepare(
            "SELECT id,created_at,completed_at,status,error,model,evaluation_date,questions,settings,
                    scope,continue_from,document_count,usage_documents,input_tokens,output_tokens,
                    cost_usd,duration_ms,errors,scored
             FROM run ORDER BY created_at DESC, id DESC LIMIT ?1 OFFSET ?2",
        )?;
        let rows = stmt.query_map(
            params![page_size as i64, ((page - 1) * page_size) as i64],
            read_run,
        )?;
        let mut runs = Vec::new();
        for row in rows {
            let mut run = row?;
            settle_status(&mut run, active);
            runs.push(RunSummary::from(&run));
        }
        Ok((runs, total as usize))
    }

    /// Every result of a run in the order they arrived.
    pub(crate) fn results(&self, id: &str) -> anyhow::Result<Vec<RunResult>> {
        let mut stmt = self
            .conn
            .prepare("SELECT result FROM run_result WHERE run_id=?1 ORDER BY at, rowid")?;
        let rows = stmt.query_map([id], |r| r.get::<_, String>(0))?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }

    /// The run `id` continues, followed back to the run that started the chain: the one
    /// with no `continue_from`.
    pub(crate) fn root_of(&self, id: &str) -> anyhow::Result<Option<String>> {
        let mut current = id.to_string();
        // Bounded by the number of runs: a chain written by this code never cycles, but a
        // database is a file somebody can edit.
        let runs: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM run", [], |r| r.get(0))?;
        for _ in 0..=runs {
            let parent: Option<Option<String>> = self
                .conn
                .query_row(
                    "SELECT continue_from FROM run WHERE id=?1",
                    [&current],
                    |r| r.get(0),
                )
                .optional()?;
            match parent {
                None => return Ok(None),
                Some(None) => return Ok(Some(current)),
                Some(Some(parent)) => current = parent,
            }
        }
        anyhow::bail!("run `{id}` continues a chain that leads back to itself")
    }

    /// The ids of a root run and every continuation of it.
    pub(crate) fn lineage(&self, root: &str) -> anyhow::Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM run WHERE id=?1 OR continue_from=?1")?;
        let rows = stmt.query_map([root], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    // ── reviews and overrides ─────────────────────────────────────────────────

    /// Records a review in one transaction: the history line, the latest whole review,
    /// the person's usage decision when the review holds one, the chain's deletions under
    /// the new verdicts, and the document's materialized rows. Returns whether the
    /// document was excluded before and after, and its tags now.
    pub(crate) fn record_review(
        &self,
        review: &Review,
        usage: Option<bool>,
    ) -> anyhow::Result<(bool, bool, Vec<String>)> {
        let doc = review.id();
        let verdicts = serde_json::to_string(&review.verdicts)?;
        let proposed = serde_json::to_string(&review.proposed)?;
        let tx = self.immediate()?;
        let (_, saved) = self.saved()?;
        let policy = Policy::new(&saved);
        let before = is_excluded(&tx, &doc)?;
        tx.execute(
            "INSERT INTO review_history(source,resource,derived_sha,verdicts,proposed,note,reviewer,at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                doc.source,
                doc.resource,
                doc.derived_sha,
                verdicts,
                proposed,
                review.note,
                review.reviewer,
                review.at
            ],
        )?;
        tx.execute(
            "INSERT INTO review(source,resource,derived_sha,verdicts,proposed,note,reviewer,at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
             ON CONFLICT(source,resource,derived_sha) DO UPDATE SET verdicts=excluded.verdicts,
               proposed=excluded.proposed, note=excluded.note, reviewer=excluded.reviewer, at=excluded.at",
            params![
                doc.source,
                doc.resource,
                doc.derived_sha,
                verdicts,
                proposed,
                review.note,
                review.reviewer,
                review.at
            ],
        )?;
        if let Some(excluded) = usage {
            set_override(&tx, &doc, excluded, "review", &review.at)?;
        }
        prune_unreached(&tx, &doc, &policy.chain)?;
        materialize(&tx, &doc, &policy)?;
        let after = is_excluded(&tx, &doc)?;
        let tags = tags_of(&tx, &doc)?;
        tx.commit()?;
        Ok((before, after, tags))
    }

    /// A person's include or exclude of one document, standing until a person says
    /// otherwise. Returns whether the document was excluded before.
    pub(crate) fn set_usage(
        &self,
        doc: &DocumentId,
        excluded: bool,
        reason: &str,
    ) -> anyhow::Result<bool> {
        let tx = self.immediate()?;
        let (_, saved) = self.saved()?;
        let policy = Policy::new(&saved);
        let before = is_excluded(&tx, doc)?;
        set_override(&tx, doc, excluded, reason, &Timestamp::now().to_string())?;
        materialize(&tx, doc, &policy)?;
        tx.commit()?;
        Ok(before)
    }

    /// Every review ever recorded, oldest first.
    pub(crate) fn review_history(&self) -> anyhow::Result<Vec<Review>> {
        let mut stmt = self.conn.prepare(
            "SELECT source,resource,derived_sha,verdicts,proposed,note,reviewer,at FROM review_history ORDER BY id",
        )?;
        let rows = stmt.query_map([], read_review)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// The latest review of every reviewed document.
    pub(crate) fn latest_reviews(&self) -> anyhow::Result<HashMap<DocumentId, Review>> {
        let mut stmt = self.conn.prepare(
            "SELECT source,resource,derived_sha,verdicts,proposed,note,reviewer,at FROM review",
        )?;
        let rows = stmt.query_map([], read_review)?;
        let mut out = HashMap::new();
        for row in rows {
            let review = row?;
            out.insert(review.id(), review);
        }
        Ok(out)
    }

    // ── reads ─────────────────────────────────────────────────────────────────

    /// A document's stored answers the chain still reaches, keyed `key@version`, and the
    /// verdicts of its latest review.
    pub(super) fn reached(
        &self,
        doc: &DocumentId,
        chain: &Chain<'_>,
    ) -> anyhow::Result<(BTreeMap<String, f64>, Option<Verdicts>)> {
        reached(&self.conn, doc, chain)
    }

    /// What a document's Lance rows should carry right now.
    pub(crate) fn state_of(&self, doc: &DocumentId) -> anyhow::Result<DocState> {
        Ok(DocState {
            tags: tags_of(&self.conn, doc)?,
            excluded: is_excluded(&self.conn, doc)?,
        })
    }

    /// Up to `limit` documents whose Lance rows are stale, each with the revision its
    /// metadata is at and the state to write.
    pub(crate) fn stale_vectors(&self, limit: usize) -> anyhow::Result<Vec<StaleDocument>> {
        let mut stmt = self.conn.prepare(
            "SELECT source,resource,derived_sha,revision FROM vector_sync ORDER BY rowid LIMIT ?1",
        )?;
        let rows = stmt.query_map((limit as i64,), |r| {
            Ok((read_document(r)?, r.get::<_, i64>(3)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, revision) = row?;
            let state = self.state_of(&id)?;
            out.push(StaleDocument {
                id,
                revision,
                state,
            });
        }
        Ok(out)
    }

    /// Acknowledges that `documents` were written at the revisions given. A document
    /// materialized again meanwhile holds a newer revision, and stays pending.
    pub(crate) fn vectors_written(&self, documents: &[StaleDocument]) -> anyhow::Result<()> {
        let tx = self.immediate()?;
        {
            let mut ack = tx.prepare_cached(
                "DELETE FROM vector_sync WHERE source=?1 AND resource=?2 AND derived_sha=?3 AND revision=?4",
            )?;
            for stale in documents {
                ack.execute(params![
                    stale.id.source,
                    stale.id.resource,
                    stale.id.derived_sha,
                    stale.revision
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// How many documents have stale Lance rows.
    pub(crate) fn stale_vector_count(&self) -> anyhow::Result<usize> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM vector_sync", [], |r| r.get(0))?;
        Ok(n as usize)
    }

    // ── import ────────────────────────────────────────────────────────────────

    /// Reads the JSONL ledgers once into the tables, under a write lock so two processes
    /// opening a store at the same time cannot both import. Runs and reviews are replayed
    /// in the order they happened through the same per-event path a live run takes, so a
    /// parent answer that moved takes its follow-ups' answers with it exactly as the old
    /// fold did. Commit-written exclusions are dropped: policy recomputes them. A person's
    /// restore or review stands as an override.
    fn import_once(&self, store: &Store) -> anyhow::Result<()> {
        if self.meta("import_done")?.is_some() {
            return Ok(());
        }
        let tx = self.immediate()?;
        // Checked again under the lock: another process may have imported meanwhile.
        let done: Option<String> = tx
            .query_row("SELECT value FROM meta WHERE key='import_done'", [], |r| {
                r.get(0)
            })
            .optional()?;
        if done.is_some() {
            return Ok(());
        }
        let (_, questions) = self.saved()?;
        let policy = Policy::new(&questions);
        let runs = legacy_runs(&store.workspace_runs_path())?;
        let reviews = read_json_lines::<Review>(&store.workspace_reviews_path())?;

        let recorded_at = |at: &str| at.parse::<Timestamp>().unwrap_or(Timestamp::MIN);
        let mut events: Vec<(Timestamp, usize, LegacyEvent<'_>)> = Vec::new();
        for (order, run) in runs.iter().enumerate() {
            events.push((recorded_at(&run.created_at), order, LegacyEvent::Run(run)));
        }
        for (order, review) in reviews.iter().enumerate() {
            events.push((
                recorded_at(&review.at),
                runs.len() + order,
                LegacyEvent::Review(review),
            ));
        }
        // Stable in ledger order among equal times, as the fold was.
        events.sort_by_key(|(at, order, _)| (*at, *order));

        for (_, _, event) in events {
            match event {
                LegacyEvent::Run(run) => import_run(&tx, run, &policy.chain)?,
                LegacyEvent::Review(review) => import_review(&tx, review, &policy.chain)?,
            }
        }
        // A person's last word per document, whatever a commit wrote after it: a commit
        // is policy, which is recomputed below, and a person's word outlasts policy.
        let mut latest: HashMap<DocumentId, LegacyDecision> = HashMap::new();
        for decision in read_json_lines::<LegacyDecision>(&store.workspace_decisions_path())?
            .into_iter()
            .filter(|d| matches!(d.reason.as_str(), "review" | "restored by operator"))
        {
            latest.insert(decision.id(), decision);
        }
        for decision in latest.into_values() {
            set_override(
                &tx,
                &decision.id(),
                decision.excluded,
                &decision.reason,
                &decision.at,
            )?;
        }
        Self::set_meta(&tx, "import_done", &Timestamp::now().to_string())?;
        tx.commit()?;
        // Everything materialized under the saved questions, chunk by chunk; the
        // fingerprint lands with the last chunk, so an open that fails here starts over.
        let documents = self.documents_with_state()?;
        self.rematerialize(&documents)
    }
}

/// The run's counters as the loop holds them, mirrored onto its row.
fn update_run_counters(tx: &Connection, run: &ClassifierRun) -> anyhow::Result<()> {
    tx.execute(
        "UPDATE run SET model=?2, usage_documents=?3, input_tokens=?4, output_tokens=?5,
                        duration_ms=?6, errors=?7, scored=?8
         WHERE id=?1",
        params![
            run.id,
            run.model,
            run.usage_documents as i64,
            run.input_tokens.map(|n| n as i64),
            run.output_tokens.map(|n| n as i64),
            run.duration_ms.map(|n| n as i64),
            run.errors as i64,
            run.scored as i64,
        ],
    )?;
    Ok(())
}

/// A run left `running` by a process that is gone is `interrupted`; a `committed` run
/// from before exclusions were live reads as what it is now, `completed`.
fn settle_status(run: &mut ClassifierRun, active: &HashSet<String>) {
    if run.status == "running" && !active.contains(&run.id) {
        run.status = "interrupted".into();
    }
    if run.status == "committed" {
        run.status = "completed".into();
    }
}

fn read_document(r: &rusqlite::Row<'_>) -> rusqlite::Result<DocumentId> {
    Ok(DocumentId {
        source: r.get(0)?,
        resource: r.get(1)?,
        derived_sha: r.get(2)?,
    })
}

/// A JSON column, with a conversion failure that names what would not parse.
fn json_column<T: serde::de::DeserializeOwned>(
    r: &rusqlite::Row<'_>,
    index: usize,
    what: &str,
) -> rusqlite::Result<T> {
    let text: String = r.get(index)?;
    serde_json::from_str(&text).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::other(format!("{what}: {e}"))),
        )
    })
}

fn read_run(r: &rusqlite::Row<'_>) -> rusqlite::Result<ClassifierRun> {
    let scope: Option<String> = r.get(9)?;
    let mut run = ClassifierRun {
        id: r.get(0)?,
        created_at: r.get(1)?,
        completed_at: r.get(2)?,
        status: r.get(3)?,
        error: r.get(4)?,
        model: r.get(5)?,
        evaluation_date: r.get(6)?,
        questions: json_column(r, 7, "run questions")?,
        effective_questions: Vec::new(),
        settings: json_column(r, 8, "run settings")?,
        scope: scope.map(|_| json_column(r, 9, "run scope")).transpose()?,
        continue_from: r.get(10)?,
        inputs: Vec::new(),
        document_count: r.get::<_, i64>(11)? as usize,
        usage_documents: r.get::<_, i64>(12)? as usize,
        input_tokens: r.get::<_, Option<i64>>(13)?.map(|n| n as u64),
        output_tokens: r.get::<_, Option<i64>>(14)?.map(|n| n as u64),
        cost_usd: r.get(15)?,
        cost_estimated: false,
        duration_ms: r.get::<_, Option<i64>>(16)?.map(|n| n as u64),
        throughput_docs_sec: None,
        errors: r.get::<_, i64>(17)? as usize,
        scored: r.get::<_, i64>(18)? as usize,
        results: Vec::new(),
        view: None,
    };
    run.cost_estimated = run.cost_usd.is_some();
    run.throughput_docs_sec = run
        .duration_ms
        .filter(|ms| *ms > 0)
        .map(|ms| run.scored as f64 / (ms as f64 / 1000.0));
    Ok(run)
}

fn read_review(r: &rusqlite::Row<'_>) -> rusqlite::Result<Review> {
    Ok(Review {
        source: r.get(0)?,
        resource: r.get(1)?,
        derived_sha: r.get(2)?,
        verdicts: json_column(r, 3, "review verdicts")?,
        proposed: json_column(r, 4, "review proposals")?,
        note: r.get(5)?,
        reviewer: r.get(6)?,
        at: r.get(7)?,
    })
}

fn set_override(
    tx: &Connection,
    doc: &DocumentId,
    excluded: bool,
    reason: &str,
    at: &str,
) -> anyhow::Result<()> {
    tx.execute(
        "INSERT INTO usage_override(source,resource,derived_sha,excluded,reason,at) VALUES (?1,?2,?3,?4,?5,?6)
         ON CONFLICT(source,resource,derived_sha) DO UPDATE SET excluded=excluded.excluded, reason=excluded.reason, at=excluded.at",
        params![doc.source, doc.resource, doc.derived_sha, excluded, reason, at],
    )?;
    Ok(())
}

fn is_excluded(conn: &Connection, doc: &DocumentId) -> anyhow::Result<bool> {
    Ok(conn.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM excluded WHERE {ON_DOCUMENT})"),
        doc_params(doc),
        |r| r.get(0),
    )?)
}

fn tags_of(conn: &Connection, doc: &DocumentId) -> anyhow::Result<Vec<String>> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT DISTINCT tag FROM tag WHERE {ON_DOCUMENT} ORDER BY tag"
    ))?;
    let rows = stmt.query_map(doc_params(doc), |r| r.get::<_, String>(0))?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// The document's stored answers and latest verdicts, the answers filtered to what the
/// chain reaches under those verdicts. The filter is the one [`Chain::retain_reached`]
/// applies; nothing is deleted here.
fn reached(
    conn: &Connection,
    doc: &DocumentId,
    chain: &Chain<'_>,
) -> anyhow::Result<(BTreeMap<String, f64>, Option<Verdicts>)> {
    let mut stmt =
        conn.prepare_cached(&format!("SELECT key,score FROM answer WHERE {ON_DOCUMENT}"))?;
    let rows = stmt.query_map(doc_params(doc), |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
    })?;
    let mut stored: BTreeMap<String, f64> = rows.collect::<Result<_, _>>()?;
    let verdicts: Option<String> = conn
        .query_row(
            &format!("SELECT verdicts FROM review WHERE {ON_DOCUMENT}"),
            doc_params(doc),
            |r| r.get(0),
        )
        .optional()?;
    let verdicts: Option<Verdicts> = verdicts
        .map(|text| serde_json::from_str::<Verdicts>(&text))
        .transpose()?;
    chain.retain_reached(&mut stored, verdicts.as_ref());
    Ok((stored, verdicts))
}

/// Deletes the document's answers to every follow-up the chain no longer reaches, as
/// it stands after the event just written. The one place answer rows are removed, and
/// the reason a follow-up's answer never revives when a parent moves away and back: the
/// row is gone, and only an answer recorded after the move counts.
fn prune_unreached(tx: &Connection, doc: &DocumentId, chain: &Chain<'_>) -> anyhow::Result<()> {
    let mut stmt =
        tx.prepare_cached(&format!("SELECT key,score FROM answer WHERE {ON_DOCUMENT}"))?;
    let rows = stmt.query_map(doc_params(doc), |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
    })?;
    let all: BTreeMap<String, f64> = rows.collect::<Result<_, _>>()?;
    drop(stmt);
    let (kept, _) = reached(tx, doc, chain)?;
    let mut delete = tx.prepare_cached(&format!(
        "DELETE FROM answer WHERE {ON_DOCUMENT} AND key=?4"
    ))?;
    for key in all.keys().filter(|key| !kept.contains_key(*key)) {
        delete.execute(params![doc.source, doc.resource, doc.derived_sha, key])?;
    }
    Ok(())
}

/// Rewrites what the current policy says about one document: its scores under every
/// current question, its tags, whether it is excluded and by whom, and the follow-ups it
/// still owes. Returns whether its tags or exclusion changed, in which case the document
/// is queued for its Lance rows to be rewritten.
///
/// A person's verdict beats the model's for the question it answers: a `yes` is a tag
/// marked `human`, a `no` or a different option takes the model's tag away. A person's
/// usage override beats the policy's exclusion either way; without one, any question
/// whose outcome excludes does. A choice's own score is the probability that the
/// document is one of its exclude options under today's actions, so the Corpus can
/// filter a junk gate like a noul.
fn materialize(tx: &Connection, doc: &DocumentId, policy: &Policy<'_>) -> anyhow::Result<bool> {
    let before = (tags_of(tx, doc)?, is_excluded(tx, doc)?);
    let (stored, verdicts) = reached(tx, doc, &policy.chain)?;
    let override_: Option<(bool, String)> = tx
        .query_row(
            &format!("SELECT excluded,reason FROM usage_override WHERE {ON_DOCUMENT}"),
            doc_params(doc),
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    for table in ["tag", "excluded", "score", "open_follow_up"] {
        tx.execute(
            &format!("DELETE FROM {table} WHERE {ON_DOCUMENT}"),
            doc_params(doc),
        )?;
    }

    let mut scores: Vec<(String, f64)> = Vec::new();
    let mut tags: Vec<(String, &str)> = Vec::new();
    let mut policy_excludes: Option<String> = None;
    for question in policy.questions {
        let verdict = verdicts.as_ref().and_then(|v| v.get(&question.id));
        let current = answers_for(question, &stored);
        scores.extend(current.iter().map(|(key, score)| (key.clone(), *score)));
        let outcome = decide(question, &current);
        if question.kind == QuestionKind::Choice
            && let Some(outcome) = &outcome
        {
            scores.push((question.id.clone(), outcome.exclusion.unwrap_or(0.0)));
        }
        if policy_excludes.is_none() && outcome.as_ref().is_some_and(|o| o.excluded) {
            policy_excludes = Some(question.id.clone());
        }
        let (question_tags, by): (Vec<String>, &str) = match verdict {
            Some(verdict) => (human_tags(question, verdict), "human"),
            None => (
                outcome
                    .map(|o| o.tags)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|tagged| match question.kind {
                        QuestionKind::Noul => tagged,
                        QuestionKind::Choice => option_key(&question.id, &tagged),
                    })
                    .collect(),
                "model",
            ),
        };
        tags.extend(question_tags.into_iter().map(|tag| (tag, by)));
    }
    let excluded: Option<(&str, String)> = match override_ {
        Some((excluded, reason)) => excluded.then_some(("human", reason)),
        None => policy_excludes.map(|question| ("model", question)),
    };

    {
        let mut score = tx.prepare_cached(
            "INSERT OR REPLACE INTO score(source,resource,derived_sha,question,value) VALUES (?1,?2,?3,?4,?5)",
        )?;
        for (question, value) in &scores {
            score.execute(params![
                doc.source,
                doc.resource,
                doc.derived_sha,
                question,
                value
            ])?;
        }
        let mut tag = tx.prepare_cached(
            "INSERT OR IGNORE INTO tag(source,resource,derived_sha,tag,by) VALUES (?1,?2,?3,?4,?5)",
        )?;
        for (name, by) in &tags {
            tag.execute(params![doc.source, doc.resource, doc.derived_sha, name, by])?;
        }
        if let Some((by, reason)) = &excluded {
            tx.execute(
                "INSERT INTO excluded(source,resource,derived_sha,by,reason) VALUES (?1,?2,?3,?4,?5)",
                params![doc.source, doc.resource, doc.derived_sha, by, reason],
            )?;
        }
        let mut open = tx.prepare_cached(
            "INSERT OR IGNORE INTO open_follow_up(source,resource,derived_sha,question) VALUES (?1,?2,?3,?4)",
        )?;
        for question in policy.chain.open(&stored, verdicts.as_ref()) {
            open.execute(params![
                doc.source,
                doc.resource,
                doc.derived_sha,
                question.id
            ])?;
        }
    }

    let after = (tags_of(tx, doc)?, excluded.is_some());
    let changed = before != after;
    if changed {
        tx.execute(
            "INSERT INTO vector_sync(source,resource,derived_sha,revision) VALUES (?1,?2,?3,1)
             ON CONFLICT(source,resource,derived_sha) DO UPDATE SET revision=revision+1",
            doc_params(doc),
        )?;
    }
    Ok(changed)
}

/// The size and modification time of a file, or `missing`: enough to say whether the
/// questions file is the one the live state was decided under.
fn file_fingerprint(path: &Path) -> anyhow::Result<String> {
    match std::fs::metadata(path) {
        Ok(meta) => {
            let modified = meta
                .modified()?
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            Ok(format!("{}:{modified}", meta.len()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok("missing".into()),
        Err(error) => Err(error.into()),
    }
}

// ── the legacy ledgers ──────────────────────────────────────────────────────

enum LegacyEvent<'a> {
    Run(&'a ClassifierRun),
    Review(&'a Review),
}

/// One line of the old `workspace/runs.jsonl`: a run as started, one result as it landed,
/// or the run as it ended. Read here and nowhere else.
#[derive(Deserialize)]
#[serde(tag = "record", rename_all = "snake_case")]
enum LegacyRecord {
    Start {
        run: ClassifierRun,
    },
    Progress {
        run_id: String,
        result: RunResult,
        model: String,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
        #[serde(default)]
        usage_documents: usize,
        duration_ms: u64,
        errors: usize,
    },
    Complete {
        run: ClassifierRun,
    },
}

/// One line of the old `workspace/decisions.jsonl`.
#[derive(Deserialize)]
struct LegacyDecision {
    at: String,
    source: String,
    resource: String,
    derived_sha: String,
    excluded: bool,
    reason: String,
}

impl LegacyDecision {
    fn id(&self) -> DocumentId {
        DocumentId {
            source: self.source.clone(),
            resource: self.resource.clone(),
            derived_sha: self.derived_sha.clone(),
        }
    }
}

/// Every run in the old ledger, its results folded in, in ledger order. A run with
/// progress but no completion record is `interrupted`; a `committed` one is `completed`.
fn legacy_runs(path: &Path) -> anyhow::Result<Vec<ClassifierRun>> {
    /// A start or completion record replaces the run it names, keeping the results
    /// progress records already folded in when the new record carries none.
    fn place(
        runs: &mut Vec<ClassifierRun>,
        by_id: &mut HashMap<String, usize>,
        run: ClassifierRun,
    ) {
        match by_id.get(&run.id).copied() {
            Some(at) => {
                let results = std::mem::take(&mut runs[at].results);
                runs[at] = run;
                if runs[at].results.is_empty() {
                    runs[at].results = results;
                }
            }
            None => {
                by_id.insert(run.id.clone(), runs.len());
                runs.push(run);
            }
        }
    }

    let mut runs: Vec<ClassifierRun> = Vec::new();
    let mut by_id: HashMap<String, usize> = HashMap::new();
    for value in read_json_lines::<Value>(path)? {
        if value.get("record").is_none() {
            place(&mut runs, &mut by_id, serde_json::from_value(value)?);
            continue;
        }
        match serde_json::from_value::<LegacyRecord>(value)? {
            LegacyRecord::Start { run } | LegacyRecord::Complete { run } => {
                place(&mut runs, &mut by_id, run)
            }
            LegacyRecord::Progress {
                run_id,
                result,
                model,
                input_tokens,
                output_tokens,
                usage_documents,
                duration_ms,
                errors,
            } => {
                if let Some(&at) = by_id.get(&run_id) {
                    let run = &mut runs[at];
                    run.results.push(result);
                    run.model = model;
                    run.input_tokens = input_tokens;
                    run.output_tokens = output_tokens;
                    run.usage_documents = usage_documents;
                    run.duration_ms = Some(duration_ms);
                    run.errors = errors;
                }
            }
        }
    }
    for run in &mut runs {
        run.scored = run.results.len();
        match run.status.as_str() {
            "running" => run.status = "interrupted".into(),
            "committed" => run.status = "completed".into(),
            _ => {}
        }
    }
    Ok(runs)
}

fn import_run(tx: &Connection, run: &ClassifierRun, chain: &Chain<'_>) -> anyhow::Result<()> {
    tx.execute(
        "INSERT OR IGNORE INTO run(id,created_at,completed_at,status,error,model,evaluation_date,questions,settings,
                                   scope,continue_from,document_count,usage_documents,input_tokens,output_tokens,
                                   cost_usd,duration_ms,errors,scored)
         VALUES (?1,?2,?3,?4,NULL,?5,?6,?7,?8,NULL,NULL,?9,?10,?11,?12,?13,?14,?15,?16)",
        params![
            run.id,
            run.created_at,
            run.completed_at,
            run.status,
            run.model,
            run.evaluation_date,
            serde_json::to_string(&run.questions)?,
            serde_json::to_string(&run.settings)?,
            run.document_count as i64,
            run.usage_documents as i64,
            run.input_tokens.map(|n| n as i64),
            run.output_tokens.map(|n| n as i64),
            run.cost_usd,
            run.duration_ms.map(|n| n as i64),
            run.errors as i64,
            run.results.len() as i64,
        ],
    )?;
    {
        let mut insert = tx.prepare_cached(
            "INSERT OR IGNORE INTO run_input(run_id,ordinal,source,resource,derived_sha) VALUES (?1,?2,?3,?4,?5)",
        )?;
        for (ordinal, doc) in run.inputs.iter().enumerate() {
            insert.execute(params![
                run.id,
                ordinal as i64,
                doc.source,
                doc.resource,
                doc.derived_sha
            ])?;
        }
    }
    let at = run
        .completed_at
        .clone()
        .unwrap_or_else(|| run.created_at.clone());
    let versions: HashMap<&str, u64> = run
        .questions
        .iter()
        .map(|q| (q.id.as_str(), q.version))
        .collect();
    let mut insert = tx.prepare_cached(
        "INSERT OR IGNORE INTO run_result(run_id,source,resource,derived_sha,result,error,input_tokens,output_tokens,at)
         VALUES (?1,?2,?3,?4,?5,?6,NULL,NULL,?7)",
    )?;
    let mut upsert = tx.prepare_cached(
        "INSERT INTO answer(source,resource,derived_sha,key,score,run_id,at) VALUES (?1,?2,?3,?4,?5,?6,?7)
         ON CONFLICT(source,resource,derived_sha,key) DO UPDATE SET score=excluded.score, run_id=excluded.run_id, at=excluded.at",
    )?;
    for result in &run.results {
        let doc = result.id();
        insert.execute(params![
            run.id,
            doc.source,
            doc.resource,
            doc.derived_sha,
            serde_json::to_string(result)?,
            result.error,
            at
        ])?;
        if result.error.is_some() {
            continue;
        }
        for (key, score) in &result.answers {
            let Some(version) = versions.get(question_of(key)) else {
                continue;
            };
            upsert.execute(params![
                doc.source,
                doc.resource,
                doc.derived_sha,
                question_version(key, *version),
                score,
                run.id,
                at
            ])?;
        }
        prune_unreached(tx, &doc, chain)?;
    }
    Ok(())
}

fn import_review(tx: &Connection, review: &Review, chain: &Chain<'_>) -> anyhow::Result<()> {
    let doc = review.id();
    let verdicts = serde_json::to_string(&review.verdicts)?;
    let proposed = serde_json::to_string(&review.proposed)?;
    tx.execute(
        "INSERT INTO review_history(source,resource,derived_sha,verdicts,proposed,note,reviewer,at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        params![
            doc.source,
            doc.resource,
            doc.derived_sha,
            verdicts,
            proposed,
            review.note,
            review.reviewer,
            review.at
        ],
    )?;
    tx.execute(
        "INSERT INTO review(source,resource,derived_sha,verdicts,proposed,note,reviewer,at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
         ON CONFLICT(source,resource,derived_sha) DO UPDATE SET verdicts=excluded.verdicts,
           proposed=excluded.proposed, note=excluded.note, reviewer=excluded.reviewer, at=excluded.at",
        params![
            doc.source,
            doc.resource,
            doc.derived_sha,
            verdicts,
            proposed,
            review.note,
            review.reviewer,
            review.at
        ],
    )?;
    prune_unreached(tx, &doc, chain)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::{RunSettings, Verdict, default_questions};
    use serde_json::json;

    fn doc(resource: &str) -> DocumentId {
        DocumentId {
            source: "city".into(),
            resource: resource.into(),
            derived_sha: "11".repeat(32),
        }
    }

    fn run_of(id: &str, questions: &[Question]) -> ClassifierRun {
        ClassifierRun {
            id: id.into(),
            created_at: Timestamp::now().to_string(),
            completed_at: None,
            model: "jev-test".into(),
            evaluation_date: "2026-10-11".into(),
            questions: questions.to_vec(),
            effective_questions: Vec::new(),
            inputs: Vec::new(),
            settings: RunSettings::default(),
            scope: None,
            continue_from: None,
            document_count: 1,
            status: "running".into(),
            error: None,
            input_tokens: Some(0),
            output_tokens: Some(0),
            cost_usd: None,
            cost_estimated: false,
            usage_documents: 0,
            duration_ms: None,
            throughput_docs_sec: None,
            errors: 0,
            scored: 0,
            results: Vec::new(),
            view: None,
        }
    }

    fn result(id: &DocumentId, answers: &[(&str, f64)]) -> RunResult {
        RunResult {
            source: id.source.clone(),
            resource: id.resource.clone(),
            derived_sha: id.derived_sha.clone(),
            answers: answers.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
            ..RunResult::default()
        }
    }

    /// The junk gate and `laws` asked only of records, saved, with a store around them.
    async fn gate_and_laws() -> (tempfile::TempDir, Store, Vec<Question>) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        let defaults = default_questions();
        let pick = |id: &str| defaults.iter().find(|q| q.id == id).unwrap().clone();
        let saved = crate::workspace::Workspace::new(&store)
            .save_questions(vec![
                pick("page_kind"),
                Question {
                    when: Some(option_key("page_kind", "record")),
                    ..pick("laws")
                },
            ])
            .unwrap();
        (dir, store, saved)
    }

    /// A result lands as answers, tags and an exclusion in one transaction; a lowered
    /// threshold re-decides them without a new score; a reworded question drops its tag
    /// until it is asked again; only the documents whose rows changed are queued for Lance.
    #[tokio::test]
    async fn a_result_materializes_and_policy_changes_rematerialize() {
        let (_dir, store, saved) = gate_and_laws().await;
        let cls = Classifications::open(&store).unwrap();
        let policy = Policy::new(&saved);
        let budget = doc("https://example.gov/budget");
        let menu = doc("https://example.gov/menu");
        let run = run_of("run-1", &saved);
        cls.insert_run(&run, &[budget.clone(), menu.clone()])
            .unwrap();
        cls.record_result(
            &run,
            &result(
                &budget,
                &[
                    ("page_kind:record", 0.95),
                    ("page_kind:navigation", 0.05),
                    ("laws", 0.85),
                ],
            ),
            Some((100, 4)),
        )
        .unwrap();
        cls.record_result(
            &run,
            &result(
                &menu,
                &[("page_kind:record", 0.05), ("page_kind:navigation", 0.95)],
            ),
            Some((100, 4)),
        )
        .unwrap();

        assert_eq!(cls.state_of(&budget).unwrap().tags, ["laws"]);
        assert!(!cls.state_of(&budget).unwrap().excluded);
        assert!(
            cls.state_of(&menu).unwrap().excluded,
            "the gate excludes live"
        );
        assert_eq!(
            cls.stale_vectors(10)
                .unwrap()
                .iter()
                .map(|s| s.id.resource.as_str())
                .collect::<Vec<_>>(),
            [budget.resource.as_str(), menu.resource.as_str()]
        );
        let stale = cls.stale_vectors(10).unwrap();
        cls.vectors_written(&stale).unwrap();
        assert_eq!(cls.stale_vector_count().unwrap(), 0);

        // Policy, not meaning: `laws` acts at 0.9 now, so the tag goes and nothing is
        // asked again. Only the budget changed, so only the budget is queued.
        let mut stricter = saved.clone();
        stricter[1].threshold = 0.9;
        let stricter = crate::workspace::Workspace::new(&store)
            .save_questions(stricter)
            .unwrap();
        assert_eq!(stricter[1].version, saved[1].version);
        assert!(cls.state_of(&budget).unwrap().tags.is_empty());
        let queued = cls.stale_vectors(10).unwrap();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].id, budget);
        assert_eq!(
            queued[0].revision, 1,
            "an acknowledged row is gone, so a later change starts a fresh revision"
        );
        let (answers, _) = cls.reached(&budget, &policy.chain).unwrap();
        assert!(answers.contains_key("laws@1"), "the answer stays");

        // A reworded question is a new version: its old answer no longer materializes.
        let mut reworded = stricter.clone();
        reworded[1].threshold = 0.8;
        reworded[1].instructions = "Does `text` set out a law?".into();
        let reworded = crate::workspace::Workspace::new(&store)
            .save_questions(reworded)
            .unwrap();
        assert_eq!(reworded[1].version, 2);
        let conn = &cls.conn;
        let has_laws: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM score WHERE resource=?1 AND question='laws')",
                [&budget.resource],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!has_laws, "no score at the new version until it is asked");
        let owed: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM open_follow_up WHERE resource=?1 AND question='laws')",
                [&budget.resource],
                |r| r.get(0),
            )
            .unwrap();
        assert!(owed, "the record owes the reworded follow-up");
    }

    /// A whole review replaces the last: a question it omits falls back to the model. A
    /// restore survives a policy change. A conditional acknowledgement leaves a newer
    /// revision pending.
    #[tokio::test]
    async fn reviews_replace_whole_and_overrides_outlast_policy() {
        let (_dir, store, saved) = gate_and_laws().await;
        let cls = Classifications::open(&store).unwrap();
        let menu = doc("https://example.gov/menu");
        let run = run_of("run-1", &saved);
        cls.insert_run(&run, std::slice::from_ref(&menu)).unwrap();
        cls.record_result(
            &run,
            &result(
                &menu,
                &[("page_kind:record", 0.05), ("page_kind:navigation", 0.95)],
            ),
            None,
        )
        .unwrap();
        assert!(cls.state_of(&menu).unwrap().excluded);

        // A person calls it a record with a law in it.
        let review = |verdicts: Vec<(&str, Value)>| Review {
            at: Timestamp::now().to_string(),
            source: menu.source.clone(),
            resource: menu.resource.clone(),
            derived_sha: menu.derived_sha.clone(),
            verdicts: verdicts
                .into_iter()
                .map(|(id, human)| (id.to_string(), Verdict { model: None, human }))
                .collect(),
            proposed: Vec::new(),
            note: String::new(),
            reviewer: "ben".into(),
        };
        let (before, after, tags) = cls
            .record_review(
                &review(vec![("page_kind", json!("record")), ("laws", json!(true))]),
                Some(false),
            )
            .unwrap();
        assert!(before && !after);
        assert_eq!(tags, ["laws"]);
        let by: String = cls
            .conn
            .query_row("SELECT by FROM tag WHERE tag='laws'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(by, "human");

        // A second review that says only `record` replaces the first whole: `laws` falls
        // back to the model, which never answered it, so the tag goes.
        cls.record_review(&review(vec![("page_kind", json!("record"))]), Some(false))
            .unwrap();
        assert!(cls.state_of(&menu).unwrap().tags.is_empty());
        assert!(!cls.state_of(&menu).unwrap().excluded);

        // The gate's threshold drops to where the menu would be junk again; the person's
        // word stands.
        let mut looser = saved.clone();
        looser[0].threshold = 0.5;
        crate::workspace::Workspace::new(&store)
            .save_questions(looser)
            .unwrap();
        assert!(
            !cls.state_of(&menu).unwrap().excluded,
            "a restore survives policy"
        );

        // The drain reads revision r; a materialize lands before the ack; the ack leaves
        // the newer revision pending.
        let pending = cls.stale_vectors(10).unwrap();
        assert_eq!(pending.len(), 1);
        cls.set_usage(&menu, true, "review").unwrap();
        cls.vectors_written(&pending).unwrap();
        let still = cls.stale_vectors(10).unwrap();
        assert_eq!(still.len(), 1, "the newer revision is still owed");
        assert!(still[0].revision > pending[0].revision);
        assert!(still[0].state.excluded);
    }

    /// The ledgers of a store from before this file are read once, in the order things
    /// happened, and land as the same tags and exclusions the old projection made of
    /// them: the gate excludes the menu, the record carries `laws`, a parent that moved
    /// took its follow-up's answer with it, and a person's restore stands while a
    /// commit-written exclusion does not.
    #[tokio::test]
    async fn legacy_ledgers_import_once_into_the_same_state() {
        let (_dir, store, saved) = gate_and_laws().await;
        // Saving the questions opened the database; a store from before it had none.
        let db = store.workspace_db_path();
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", db.display()));
        }
        let budget = doc("https://example.gov/budget");
        let menu = doc("https://example.gov/menu");
        let moved = doc("https://example.gov/moved");
        let restored = doc("https://example.gov/restored");
        let committed = doc("https://example.gov/committed");
        let recommitted = doc("https://example.gov/recommitted");
        let line = |value: Value| format!("{value}\n");
        let run = |id: &str, at: &str, results: Vec<RunResult>| {
            let mut run = run_of(id, &saved);
            run.created_at = at.into();
            run.status = "committed".into();
            run.document_count = results.len();
            run.inputs = results.iter().map(RunResult::id).collect();
            run.results = results;
            line(json!({ "record": "complete", "run": run }))
        };
        let junk = |id: &DocumentId| {
            result(
                id,
                &[("page_kind:record", 0.05), ("page_kind:navigation", 0.95)],
            )
        };
        let record = |id: &DocumentId| {
            result(
                id,
                &[
                    ("page_kind:record", 0.95),
                    ("page_kind:navigation", 0.05),
                    ("laws", 0.9),
                ],
            )
        };
        let runs = [
            run(
                "run-1",
                "2026-10-01T00:00:00Z",
                vec![
                    record(&budget),
                    junk(&menu),
                    record(&moved),
                    junk(&restored),
                    junk(&committed),
                    junk(&recommitted),
                ],
            ),
            // The moved document becomes navigation: its `laws` answer goes and never comes back.
            run("run-2", "2026-10-02T00:00:00Z", vec![junk(&moved)]),
            run(
                "run-3",
                "2026-10-03T00:00:00Z",
                vec![result(
                    &moved,
                    &[("page_kind:record", 0.95), ("page_kind:navigation", 0.05)],
                )],
            ),
        ]
        .concat();
        let decisions = [
            line(json!({ "at": "2026-10-01T01:00:00Z", "source": "city", "resource": menu.resource, "derived_sha": menu.derived_sha, "excluded": true, "reason": "classifier run run-1", "run_id": "run-1" })),
            line(json!({ "at": "2026-10-01T01:00:00Z", "source": "city", "resource": restored.resource, "derived_sha": restored.derived_sha, "excluded": true, "reason": "classifier run run-1" })),
            line(json!({ "at": "2026-10-01T02:00:00Z", "source": "city", "resource": restored.resource, "derived_sha": restored.derived_sha, "excluded": false, "reason": "restored by operator" })),
            line(json!({ "at": "2026-10-01T01:00:00Z", "source": "city", "resource": committed.resource, "derived_sha": committed.derived_sha, "excluded": true, "reason": "classifier run run-1" })),
            // Restored by a person, then excluded again by a later commit: the person's
            // word is the one that stands.
            line(json!({ "at": "2026-10-01T02:00:00Z", "source": "city", "resource": recommitted.resource, "derived_sha": recommitted.derived_sha, "excluded": false, "reason": "restored by operator" })),
            line(json!({ "at": "2026-10-02T01:00:00Z", "source": "city", "resource": recommitted.resource, "derived_sha": recommitted.derived_sha, "excluded": true, "reason": "classifier run run-2", "run_id": "run-2" })),
        ]
        .concat();
        let reviews = line(json!({
            "at": "2026-10-04T00:00:00Z", "source": "city", "resource": budget.resource, "derived_sha": budget.derived_sha,
            "verdicts": { "laws": { "model": 0.9, "human": false } }, "proposed": ["ordinance"], "reviewer": "ben"
        }));
        std::fs::write(store.workspace_runs_path(), runs).unwrap();
        std::fs::write(store.workspace_decisions_path(), decisions).unwrap();
        std::fs::write(store.workspace_reviews_path(), reviews).unwrap();

        let cls = Classifications::open(&store).unwrap();
        assert!(
            cls.state_of(&menu).unwrap().excluded,
            "the gate's verdict excludes"
        );
        assert!(
            cls.state_of(&committed).unwrap().excluded,
            "by policy, not by the dropped commit line"
        );
        assert!(
            !cls.state_of(&restored).unwrap().excluded,
            "the person's restore stands"
        );
        assert!(
            !cls.state_of(&recommitted).unwrap().excluded,
            "a commit after a restore does not undo the person"
        );
        assert!(
            cls.state_of(&budget).unwrap().tags.is_empty(),
            "the person said no to `laws`"
        );
        assert!(cls.state_of(&moved).unwrap().tags.is_empty());
        let (answers, _) = cls.reached(&moved, &Policy::new(&saved).chain).unwrap();
        assert!(
            !answers.contains_key("laws@1"),
            "moving back revives nothing"
        );
        let (runs, total) = cls.runs(1, 10, &HashSet::new()).unwrap();
        assert_eq!(total, 3);
        assert!(runs.iter().all(|r| r.status == "completed"), "{runs:?}");
        assert_eq!(cls.results("run-1").unwrap().len(), 6);
        assert_eq!(cls.review_history().unwrap().len(), 1);

        // A second open imports nothing again.
        let again = Classifications::open(&store).unwrap();
        assert_eq!(again.review_history().unwrap().len(), 1);
        assert_eq!(again.results("run-1").unwrap().len(), 6);
    }

    /// A run decides each document under the questions saved when the document lands,
    /// not the ones it started with: a threshold moved mid-run governs every answer after
    /// it, so a run cannot put back a tag the person just took away.
    #[tokio::test]
    async fn a_run_decides_under_the_policy_saved_now() {
        let (_dir, store, saved) = gate_and_laws().await;
        let cls = Classifications::open(&store).unwrap();
        let budget = doc("https://example.gov/budget");
        let run = run_of("run-1", &saved);
        cls.insert_run(&run, std::slice::from_ref(&budget)).unwrap();

        // Mid-run, `laws` is made stricter. Same meaning, so the run's snapshot still
        // names the current version.
        let mut stricter = saved.clone();
        stricter[1].threshold = 0.9;
        crate::workspace::Workspace::new(&store)
            .save_questions(stricter)
            .unwrap();

        cls.record_result(
            &run,
            &result(
                &budget,
                &[
                    ("page_kind:record", 0.95),
                    ("page_kind:navigation", 0.05),
                    ("laws", 0.85),
                ],
            ),
            None,
        )
        .unwrap();
        assert!(
            cls.state_of(&budget).unwrap().tags.is_empty(),
            "0.85 does not clear the threshold saved after the run began"
        );
    }

    /// Rewording a parent leaves it no answer at its new version, so its follow-up is no
    /// longer reached and the rematerialization deletes the follow-up's answer — as a run
    /// moving the parent would. Answering the reworded parent onto the same tag again does
    /// not bring the old follow-up answer back.
    #[tokio::test]
    async fn a_reworded_parent_takes_its_follow_ups_answers_for_good() {
        let (_dir, store, saved) = gate_and_laws().await;
        let cls = Classifications::open(&store).unwrap();
        let budget = doc("https://example.gov/budget");
        let run = run_of("run-1", &saved);
        cls.insert_run(&run, std::slice::from_ref(&budget)).unwrap();
        cls.record_result(
            &run,
            &result(
                &budget,
                &[
                    ("page_kind:record", 0.95),
                    ("page_kind:navigation", 0.05),
                    ("laws", 0.85),
                ],
            ),
            None,
        )
        .unwrap();
        assert_eq!(cls.state_of(&budget).unwrap().tags, ["laws"]);

        let mut reworded = saved.clone();
        reworded[0].instructions = format!("{} Read the whole page.", saved[0].instructions);
        let reworded = crate::workspace::Workspace::new(&store)
            .save_questions(reworded)
            .unwrap();
        assert!(reworded[0].version > saved[0].version);

        // The reworded parent is answered onto `record` again; `laws` is not asked.
        let again = run_of("run-2", &reworded[..1]);
        cls.insert_run(&again, std::slice::from_ref(&budget))
            .unwrap();
        cls.record_result(
            &again,
            &result(
                &budget,
                &[("page_kind:record", 0.95), ("page_kind:navigation", 0.05)],
            ),
            None,
        )
        .unwrap();
        let (answers, _) = cls.reached(&budget, &Policy::new(&reworded).chain).unwrap();
        assert!(
            !answers.keys().any(|key| key.starts_with("laws")),
            "the old `laws` answer did not come back: {answers:?}"
        );
        assert!(cls.state_of(&budget).unwrap().tags.is_empty());
    }

    /// A questions file the live state was not decided under — here, edited by hand —
    /// is rematerialized on the next open, which is also what a save that crashed
    /// halfway looks like.
    #[tokio::test]
    async fn a_changed_questions_file_rematerializes_on_open() {
        let (_dir, store, saved) = gate_and_laws().await;
        let cls = Classifications::open(&store).unwrap();
        let budget = doc("https://example.gov/budget");
        let run = run_of("run-1", &saved);
        cls.insert_run(&run, std::slice::from_ref(&budget)).unwrap();
        cls.record_result(
            &run,
            &result(
                &budget,
                &[
                    ("page_kind:record", 0.95),
                    ("page_kind:navigation", 0.05),
                    ("laws", 0.85),
                ],
            ),
            None,
        )
        .unwrap();
        assert_eq!(cls.state_of(&budget).unwrap().tags, ["laws"]);
        drop(cls);

        // The file changes under the database: a threshold `laws` no longer clears.
        let mut edited = saved.clone();
        edited[1].threshold = 0.95;
        let path = store.workspace_questions_path();
        let mut lines = std::fs::read_to_string(&path).unwrap();
        lines.push_str(&serde_json::to_string(&edited).unwrap());
        lines.push('\n');
        std::fs::write(&path, lines).unwrap();

        let cls = Classifications::open(&store).unwrap();
        assert!(cls.state_of(&budget).unwrap().tags.is_empty());
        assert!(cls.meta("questions_fingerprint").unwrap().is_some());
    }
}
