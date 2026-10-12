# The classifier workspace

Scoring the corpus for usefulness with Jev, and where `centinel web` fits.

## The problem

A corpus of 30 sources holds a lot of nothing: navigation shells, meeting calendars,
staff directories, holiday schedules. It all gets collected, extracted, indexed and
chunked, and the index grows to a size the embed stage then pays for on every run.
Most of it will never answer a question anybody asks.

What is "nothing" depends on who is asking, so it cannot be decided once in the
pipeline. It has to be a judgment the operator makes, against questions they can see
and change.

## What was measured

A trial (2026-09-16) scored nine real corpus documents against two questions with
[Jev](https://docs.typesafe.ai/introduction) (`jev-1.13.0`), TypeSafe's System One
model. Jev evaluates typed questions against a state and returns probabilities, not
prose. The two questions:

1. **participation** — does the text announce a *current or upcoming* opportunity for
   the public to give input into a government decision?
2. **past_participation** — does it report that the public *already* gave input?

Both are Nouls (yes/no probabilities), sent in one request per document. The point of
asking both: minutes of a past meeting score low on the first and high on the second.
They are not noise. They are the record of participation that already happened, and a
classifier that dropped them for not being *future* hearings would lose exactly the
material the corpus exists to keep.

Results, against labels fixed before the calls:

| Document | participation | label |
|---|---|---|
| FY2027 budget presentation (Sept 22 hearing listed) | 0.96 | true |
| BOCC / LDC speaking guides (undated, standing) | 0.95, 0.82 | review |
| ARC agenda (hearing already held) | 0.72 | review |
| Rezoning application guide (procedure, not a notice) | 0.30 | false |
| FY25 budget archive (2024 hearings) | 0.08 | false |
| Closure, park map, trash schedule | 0.01 | false |

The separation is wide on clear cases and the middle band is exactly where it should
be: standing instructions and past hearings, which are *related to* participation but
not *announcements* of it. That band is the argument for a second question rather than
a tighter threshold.

Method notes that matter for repeating this:

- **Synthetic first.** Eight constructed notices were scored before any real text
  left the machine. The "details coming soon" case scored 0.11, so the question's
  phrasing against promises of later information was settled before real documents
  were sent.
- **Complete documents only.** Texts were read through `centinel read` with
  `truncated: false` and no ambiguous matches; each text's SHA-256 matched its
  `derived_sha`. A partial page would test the truncation, not the question.
- **Scores are not accuracy.** One targeted sample of nine documents estimates
  nothing about 19,000 addresses. It says the question separates the obvious cases,
  which is the precondition for the larger labelled set.

## What Jev is for here

Jev answers *questions*; the program applies *rules*. "Does this announce a hearing?"
is a question. "Documents scoring above 0.9 join the civic-engagement preset" is a
rule, and it lives in code where the threshold can change without re-sending anything.

The working patterns, from the [primitives](https://docs.typesafe.ai/primitives) and
[patterns](https://docs.typesafe.ai/patterns) pages:

- **Noul** for a clean yes/no condition. 0.5 means uncertain, never medium.
- **Score** for a spectrum with described levels (no detail → brief mention →
  substantive).
- **Choice** for one category out of a set (agenda, minutes, notice, other).
- **Atomic questions, composed in code.** Instead of "rate this document", ask
  separately about extraction quality, boilerplate share, and topic fit, then weight
  them. Answers are independent, so adding questions barely changes latency.
- **Thresholds are policy.** Store the answers with the model, question version and
  input hash; a threshold change then re-decides without re-scoring.

One caution from the [confidence](https://docs.typesafe.ai/confidence) page, applied
here: confidence describes the shape of the probability distribution. It is not
accuracy. Low confidence routes to review; it does not excuse a wrong answer.

## Questions and policy

Every question is a **noul** or a **choice**. Its meaning is the wording, the kind, and
each option's id and description. A change to any of these makes a new version. Its
policy is the threshold, the review floor, and the actions. A change to policy decides
the stored scores again. It does not send text to Jev again.

- **Noul.** One probability. At or above the threshold, its action applies: exclude,
  tag, or score only.
- **Choice.** One probability for each option, summing to one. Each option has its own
  action. The probabilities of the exclude options are **added**, so a page that is half
  navigation and half calendar is still junk. An option with a tag action tags the
  document when that option alone reaches the threshold.
- **Review band.** Scores from the review floor up to the threshold are held for review.
  They are not excluded. A run shows them as their own count and filter.

Each choice must have an `other` option. Without it, a document that fits no option is
forced into a wrong one. Write options by purpose, with the words a reader sees on the
page, and name the one or two options each is most likely confused with.

The answers are stored as `question` for a noul and `question:option` for each option of a
choice. Search can filter on either. For a choice, the score Search filters under its own id
is the summed probability of its exclude options under the current actions.

### Follow-ups: the question chain

A question can be asked only after another one. Its `when` names one answer of another
question in the set as a tag: `page_kind:record` for an option of a choice, or `spending`
for a yes-or-no question's yes. A no is no tag, so nothing follows it. A question with no
`when` is a **root** and is asked of every document; a question with one is a
**follow-up**, asked of a document only when the document's answer to its parent landed on
that tag and the parent was itself asked. `when` is policy, like a threshold: changing it
makes no new version.

- **Landed** means the answer, not the policy. A choice lands on its likeliest option, the
  first in order on a tie; a yes-or-no question lands on yes at 0.5 or above. A person's
  verdict in Review beats the model's. Moving a threshold never changes which follow-ups
  apply.
- **A run walks the chain one level at a time.** For each document, the questions that
  reach it go in one request; the follow-ups its answers lead to go in the next, and so on
  until nothing is left that applies. A document is never sent a question its parent did
  not lead to, so it costs nothing. Each level is a request of its own and carries the
  text again. A follow-up whose parent the run does not ask hangs off the answer the
  document already holds. A failure at any level fails the document whole; the next run
  asks it from the top. Every path follows the same rule: a saved run, a preview, the CLI,
  and the Test on the Classify canvas.
- **Not asked is not no.** A follow-up that did not reach a document has no answer at all.
  It does not count as a no, holds no score in Search's facets, sits in no review band, and
  leaves nothing pending. A run counts it apart: per question as `not_asked`, and a
  document that no question of the run reached as `not_asked` in the run's totals, with a
  filter of its own.
- **When a parent's answer moves, its follow-ups' answers go.** Every result and every
  review is written in one transaction that also deletes the document's answers to every
  follow-up the chain no longer reaches, with the latest verdicts standing for the parent.
  A later run that moves the parent elsewhere, or a person's verdict that does, removes the
  follow-ups' answers from Search, the review queue and the evaluation for good: they are
  never revived, whether a run or a person moves the parent back, because the rows are
  gone. The follow-up is owed again and asked again, and only an answer recorded after the
  move counts. An edit to `when`, or a reworded parent with no answer at its new version
  yet, changes what the stored answers reach without deleting any; the run history is
  never edited.
- **Saving checks the chain.** A `when` that names no answer of a question in the set, and
  a chain that leads back to its own answers, are refused with the question's id.
- **The estimate follows the chain too.** Before a run, a follow-up is priced for the share
  of documents expected to reach it: its parent's share times the share of documents
  scored on its tag that score 0.5 or more there today. That is exact for a yes, and a
  floor under "landed on this option" for a choice, since an option can win below even
  odds. A tag no document has an answer for yet counts as every document. Each group of
  follow-ups sharing a `when` is priced as a request that sends the text again; two groups
  that reach the same document at the same depth share one, so the estimate errs high.

### The junk gate

The **junk gate** is one choice, `page_kind`. Its options are `record`, `service_info`,
`navigation`, `calendar_or_directory`, `unreadable`, and `other`. The three junk options
exclude. The default policy excludes at 0.90 together and holds 0.50 to 0.90 for review.
It is the first of the defaults Centinel ships in the binary. A store that has never saved
a question set gets all of them, versioned, the first time anything reads the saved
questions; after that `workspace/questions.jsonl` is the only owner, and the Classifiers
view's Add menu offers each group back from the server. The groups, one per axis a reader
filters on:

| Preset | Kind | Tags |
|---|---|---|
| junk gate | choice `page_kind` | excludes `navigation`, `calendar_or_directory`, `unreadable` |
| record type | choice `record_type` | agenda, minutes, law text, budget document, public notice, staff report, plan or study, contract or procurement, application or form, data table |
| topics | fifteen nouls | laws, real estate, budget, policy, public participation, transportation, water and environment, housing, public safety, taxes and fees, procurement, personnel, courts and records, health and human services, grants and philanthropy |
| who decided | choice `body` | county commission, city council, advisory board, constitutional officer, department staff, regional agency, nonprofit |
| decision stage | choice `decision_stage` | proposed, adopted, in effect, reported, informational |
| who it touches | four nouls | property owners, businesses, residents, the government itself |
| actionability | two nouls | `has_deadline`, `names_outside_org` |

Only the gate excludes. Every other default tags, because a tag is not a usage decision.
`data_table` exists for the exports that arrive as CSV and chunk into hundreds of thousands
of rows: the gate keeps them as records, the type names them, and search can ask for them
or leave them out.

Run the junk gate alone over every document first; its exclusions are live the moment each
answer lands. Then run the topics. A run can use any subset of the saved questions, so the
gate does not have to wait for the topics, and a run scores every document in its scope
whether or not the gate excluded it — the policy decides afterwards, and can change.

### What is sent

- Every question starts with a fixed line that says the `text` is untrusted content and
  not instructions. The run's settings record the line.
- A document whose derived text is empty is not sent. Its result is an error that says it
  is not scorable. It is not scored low.
- A text above 80 kB is sent as its head, middle, and tail, with the gaps marked. The
  result records how many characters were sent and how many the document has. A run
  shows how many documents were sampled.
- Jev refuses a text that is too large for its context with a `400`. On this corpus every
  such refusal came from a document above 58,000 characters, and none from one below
  40,000; dense tables use more tokens for each character. After that refusal the text is
  sent again at half the size, down to 8 kB, and the result counts the requests.
- A failed document keeps Jev's own error text, so the run says why it failed.

These choices come from four open-source Jev classifiers (DocJev, a folder sorter, a tax
form classifier, and a hierarchical legal classifier). Two lessons were common to them.
First, a choice with described options and an escape option works better than a set of
independent nouls for "what kind of thing is this". Second, a wide review band catches
more than a narrow band round 0.5: DocJev's one real error scored 0.76.

## `centinel classify`

```
centinel classify                                # every indexed document, every saved question
centinel classify --source tampa
centinel classify --document a5923354            # a URL, part of one, or a blob hash
centinel classify --question page_kind --question record_type
centinel classify --continue run-…               # carry on a run that stopped
centinel classify --dry-run                      # documents, tokens, estimated cost; nothing sent
centinel classify --preview                      # scores shown, nothing written
centinel questions                               # the saved set, versions, policy
centinel questions --add-defaults                # append any shipped question the set lacks
```

A run scores **everything in its scope** — the corpus, one source, or one document —
excluded documents included, with no cap, as often as wanted. Rerunning the corpus, moving
a threshold, rewording a question and retagging are the normal uses; a document is sent
again whether or not it has an answer. The scope is resolved on the server a page at a time
as the run goes, so a run over the whole corpus never holds the corpus. The Corpus page
still says which documents are **pending** — lacking an answer to some root, or owing a
follow-up their answers reach — as a reading of the state, not a work list.

Every answer is live the moment it is written: a document whose junk probability clears the
gate's threshold is out of search at once, every tag that clears its threshold can be
filtered on, the review band is left for a person, and `restore` undoes an exclusion.
`--preview` scores and shows but writes nothing, so a reworded question can be tried on a
hundred documents before it is saved.

**A `401`, `402` or `403` from Jev stops the run.** No key, no credit and no permission are
facts about the account, not the document, so nothing more is sent: the run is recorded
`stopped` with Jev's own words, every answer already written stays, and the document in
flight keeps the levels of its chain it was answered. `--continue <run>` carries it on —
the same scope, question snapshot and settings as the run that started the chain, over the
documents in scope now that no run in the chain has answered without error. Any other
per-document error is recorded on the document and the run goes on.

Classifying is **not a pipeline stage**. `centinel run` extracts, indexes and embeds;
`embed` embeds everything indexed. Text leaves the machine only when a person starts a run,
from this command or the web's Classify button. The `[classify]` block in `centinel.toml`
names the model the command uses when none is given:

```toml
[classify]
model = "jev-1.13.0"
```

### Tags in search

```
centinel search "stormwater" --tag record_type:minutes --tag water_environment
centinel search "permits" --not-tag record_type:data_table
```

A tag is spelled the way the answers are stored: a yes-or-no question's id, or
`choice:option`. Repeated `--tag` is AND; `--not-tag` removes. Both arms of the search
apply the filter, and every result carries the `tags` of the document it cites. The HTTP
op and the MCP tool take `tags` and `not_tags` arrays.

Tags and exclusions live in `workspace/workspace.db`, which the search index attaches and
reads: one `tag` row per document and tag the current policy puts on it, marked `model` or
`human`, and one `excluded` row per document the policy or a person took out. Both are
rewritten the moment a result lands, a review is recorded, or a threshold moves — so moving
a threshold in the Classify view changes what the next `centinel search --tag` returns, and
sends nothing to Jev. The vector rows carry the same tags and exclusion and are brought up
to date after every run, review and save, so both arms of a search filter on one fact. A
tag no saved question defines is refused with the list to check, so a typo is an error
rather than an empty result. See [RETRIEVAL.md](RETRIEVAL.md).

## Review, and the loop that tunes the questions

The fourth view of `centinel web` is **Review**: one document at a time, its full text on
the left, and on the right what the current policy decided for each saved question — the
gate's verdict, each choice's winning option, each tag it put on or left off. The person
answers with the keyboard. Right arrow says the document is a record, left arrow says it is
junk; the chips and options can be changed first; Enter records the card as it stands;
`s` skips. A box takes tag names that do not exist yet, and a note.

Every card is one review in `workspace/workspace.db`, kept whole in its history and as the
document's latest word:

```json
{"at":"2026-10-07T21:10:00Z","source":"tampa","resource":"https://…","derived_sha":"a59…",
 "verdicts":{"page_kind":{"model":"navigation","human":"record"},
             "laws":{"model":0.41,"human":true}},
 "proposed":["ordinance amendment"],"note":"agenda packet, menus around it","reviewer":"ben"}
```

A verdict is not only a label for later. It acts at once: a `record` said of an excluded
menu restores it to search, a junk option said of a kept page excludes it, a `yes` to a
question that tags puts the tag on the document marked `human`, and a `no` takes the
model's tag away. A review's say on usage is a **usage override**, like a restore: a
person's word on a document stands through every later run and threshold change until a
person says otherwise. The latest review of a document replaces the one before it whole; a
question it leaves out falls back to the model. The queue offers the review band first —
documents with some answer between a question's review floor and its threshold — then a
random sample of the decided ones, and skips documents already reviewed unless asked.

`centinel evaluate` reads the same reviews back beside the stored answers. Per question: how
many reviews had a model score to compare with, how often the policy's decision matched
the person's, precision and recall at the current threshold, and for a yes-or-no question
the threshold that would have matched most (judged by F1, so "no to everything" cannot
win). For a choice, what people said against what the model's top option was. And every
proposed tag with a count. The `--json` form is the one an agent reads: change a wording
in `workspace/questions.jsonl`, `classify --preview` the reviewed documents, read it again.
Reviewed documents are the regression set for every later run.

## `centinel web`

```
centinel web                    # serves http://127.0.0.1:8787/web and opens the browser
centinel web --bind 127.0.0.1:9000
centinel web --rebuild          # rebuilds the Start SPA shell from this checkout, then opens
centinel web --server https://box.tailnet.ts.net   # this page, another machine's corpus
```

`--server` serves this binary's page on loopback and forwards the API to that server, so
no local store is opened and the remote needs no CORS. Both must be the same release.

`centinel web` reuses a server already on the port only when it is the same build of
the same version on the same corpus root. A server left running from before a
`cargo build` is refused by name, so the new code and page are never hidden behind an
old process. Every connection to `centinel.db` waits up to five seconds on a lock held
by another process, so a rebuild running beside a live server slows a page instead of
failing it.

`--rebuild` runs `npm run build` in the source checkout this binary was compiled in,
streams the Start build output, checks the version stamp, and serves the rebuilt page for this
process instead of the embedded one. It is the way to see a page change without a
`cargo build`. It refuses to hand the browser to a server already on the port, because
that server would show its own page. A release download has no checkout, so there the
flag reports that and stops. The index is never touched; `centinel index --rebuild` is
the command for that.

The React workspace uses TanStack Start in SPA mode with file-based routes under
`web/src/routes/`, Tailwind v4, and stock neutral shadcn/ui components. Start generates
a static shell at build time; `web/build-shell.mjs` copies it and its hashed CSS and
JavaScript into `web-dist/`. A bundled TanStack Start server handles authentication and forwards protected API requests to Rust at runtime.

`web-dist/` is embedded in the binary: the shell is served at `/web` and every path under
it, the assets at `/web/assets/`. The installed program ships no asset directory and
requires Node 22.13 or newer to serve the application. The Start server bundle is embedded too; `cargo build` relays the Start build
output as `web:` warnings so the bundle step is visible.

The page is stamped with the Centinel version it was built for. `build.rs` refuses a
bundle whose stamp differs from the crate version, and `centinel web` checks the stamp
before it probes a port, rebuilds an index, or opens a browser. A release download
cannot rebuild the bundle, so there the check can only report a mismatch. The page shows
its version in the sidebar and warns when the server it reached reports another one, which
is what a `centinel web` left running from an older build looks like.

While a run scores, the page shows it live: progress by decision, the documents out to
Jev with how long each has waited and which request it is on, and the latest answers as
they land; the Corpus refreshes every few seconds meanwhile, because every answer is
already in it. When scoring stops, the same panel gives the counts by decision, and for a
run Jev refused, Jev's message and **Continue**. The sidebar marks a running run from every
page.

### Jobs, live

Every long-running op the server process runs is a job: a scheduled `run` under
`centinel serve`, and a classifier run started from the page (its job id is the run's id).
The sidebar's **Working now** lists each active job with what it is doing (`Collecting
tampa.gov`, `Embedding`), its count through the current stage, and the item in hand;
clicking one opens its log: every page fetched, document read or document scored, on
the server's clock, failures in their own colour. Nothing polls for this. The page holds
one stream open and folds its events into the cache.

| Route | Answers |
|---|---|
| `GET /workspace/jobs` | `{ "jobs": [JobState] }`: active jobs in the order they started, then the last twenty finished, newest first |
| `GET /workspace/jobs/events` | Server-Sent Events: one `snapshot` event (the same body), then a `job` event per change. `?job=<id>` narrows both to one job |

A `job` event carries `seq` (grows by one per event across every job), `at`
(milliseconds since the epoch, server clock), `job` (the id, which is the topic), and a
`type`:

| `type` | Fields | From |
|---|---|---|
| `started` | `kind` (the op, or `classify`), `label` | the job starting |
| `step` | `step`, e.g. `tampa.gov · collect` | `run` entering a stage |
| `progress` | `message`, `done`, `total`, `current` (the item in hand) | a stage's count |
| `item` | `item`: `address`, `tag`, `verdict` (`ok`, `warn`, `missing`, `fail`), `bytes`, `millis`, `detail` | one finished page or document |
| `note` | `message` | a log line |
| `finished` | `outcome` (`ok`, `failed`, `cancelled`), `error` | the job ending |

A `JobState` is those events folded: `step`, `done`, `total`, `current`, `ok` and
`failed` item counts, `outcome`, and `log`, the last two hundred events other than
`progress`. A client that falls behind the stream is sent a fresh `snapshot`; any event
whose `seq` is not above its job's `seq` is already in it. Jobs live in the server's
memory: a restart empties the list, and a `centinel run` typed in another terminal is
another process and does not appear.

The workspace has six views, in a sidebar grouped Archive, Classifiers, and Agent:

- **Search** opens on one question box with the corpus at a glance, and lists nothing
  until something is asked. It pages through the index without loading it into browser
  memory. Full-text,
  address, any number of Sources, usage, and a classifier score range can be combined.
  Each filter shows what it would find: documents per Source, per usage, and each
  classifier's scores by tenth, every count taken with the other filters but not its own. The reader
  resolves the exact Source, Resource, and derived text identity, including shared text.
  It shows the document as collected beside its extracted text: a PDF in the browser's
  viewer, a CSV as a table, HTML as the page and as its source. `GET
  /workspace/original` serves those bytes, and Download saves them. Collected HTML is
  served with a sandbox policy, so a page never runs on the workspace's origin.
- **Classify** opens with **Run classifiers**: a source (all, or one), the checked
  questions, the server's count and price of what would be sent, and one button —
  *Classify N documents* — that scores every indexed document in that scope, excluded ones
  too, with no cap. A document's own page has a **Classify** button for that document, and
  the Corpus page, narrowed to one source, offers **Classify this source**. Below it the
  canvas draws one chain at a time, panned and zoomed: a source
  question on top, its answers along its foot, and each follow-up below the answer it
  is asked after. Deleting a question moves its follow-ups up to the answer it followed. A question's
  `when` names that answer as a tag (`page_kind:record`, or `spending` for a noul's yes);
  it is policy, so changing it makes no new version. Runs follow the tree: a checked
  follow-up is asked only of the documents whose answer leads to it (see
  [Follow-ups](#follow-ups-the-question-chain)). Test sends one document through as a
  preview, asks only the questions on its path, and lights the answers Jev gave. The
  run card and the results show documents nothing reached as **not asked**, and a
  question a document was not asked as not asked rather than a score. The page edits atomic
  Jev questions and their policy thresholds. A trial sends
  the Search filter and a count; the server resolves the top matches in one query and
  stores the exact identities with the run, so the browser never pages the corpus back
  and forth. The start request is answered at once with the run as started. Scoring
  continues on the server with a bounded number of documents in flight to Jev at a time
  (eight unless the trial says otherwise, at most thirty-two), each result written to the
  workspace the moment it lands and kept nowhere else, and the page polls the run detail
  once a second until the status leaves `running`. The run card estimates the input
  tokens and the cost from the characters of the selection before the run starts. Only
  the checked questions go into the run. A request that Jev refuses with `429`, answers
  with a `5xx`, or drops is retried up to four times with a growing pause before its
  failure is recorded; a `401`, `402` or `403` stops the run. Token counts and the cost
  estimate sum over the documents whose answer reported usage, and the run says how many
  that covers when it is not all of them. Score columns sort by a click on their header,
  high to low first. The Output selector chooses between a saved run and a preview. A
  saved run accepts only the current saved question set, so a stale browser tab cannot
  change its meaning. A preview scores the same selection, shows the scores on the page,
  and writes nothing to the workspace (its calls are paid for, so they still land in
  `spend/`, like every Jev answer); it accepts draft questions because it records no
  meaning, and it cannot be repeated. Saving a question versions model-evaluated meaning;
  changing an action threshold re-decides stored scores without sending the text again.
- **Runs** keeps the question versions, the scope or the selected input identities, model,
  evaluation date, settings, tokens, duration, throughput, errors, and cost when the model
  has a price. Prices live in one table, `spend::PRICES`; the Spend page shows every call.
  The paged list returns small summaries. One run detail returns its exact evaluated
  questions, the current effective policy, the counts of excluded, review, kept, tagged,
  not asked, and failed documents, and one page of results. The server filters and sorts
  the results, so a run over the whole corpus never goes to the browser at once. A repeat
  names the run it repeats, and the server copies its inputs or resolves its scope again.
  A run Jev refused reads `stopped` with Jev's words and offers **Continue**; a run with
  durable progress but no completion record is shown as interrupted after a server
  restart. A run from before exclusions were live reads as completed.
- **Review** puts one document beside every answer Jev gave it, for a person's verdict.
- **Connect** shows how to add Centinel's MCP server to an agent over HTTP, and lists
  the tools the registry offers it. The address is `CENTINEL_PUBLIC_URL` when the server
  sits behind another address, and the page's own host otherwise. **Skills** shows `npx skills add bennyhodl/centinel` and what
  each skill in `contrib/skills/` does.

There is no commit. What the current policy makes of the stored answers is what search
sees, as soon as a result lands or a threshold moves; a review or a restore overrides it
for one document. Nothing changes `blobs/` or `log/`.

Classifier truth is stored in `workspace/questions.jsonl` and `workspace/workspace.db`:
every run and its results, the latest answer per question and version, every review, a
person's usage overrides, and the tags, exclusions and scores materialized from them. The
search index attaches the database and reads it; deleting and rebuilding `centinel.db`
loses no answer and no verdict. A store that still has `runs.jsonl`, `decisions.jsonl` and
`reviews.jsonl` from before is imported once, on first open, and the files are left where
they are.

Corpus text leaves the machine only after the operator starts a run, and only for the
selected documents. The server reads `TYPESAFE_API_KEY` from the process environment,
then from `.env` in the working directory, then from `.env` in the corpus root; the key
is never sent to the browser. It posts one complete document and the run's questions that
reach it at one level of the chain to `https://api.typesafe.ai/v1/systemone` per request.

## The boundary that matters

Classification changes **usage**, never the archive:

- Presets (civic engagement, real estate, …) name the questions and thresholds.
- Scores are stored beside the index, versioned, and one document can match several
  presets.
- Exclusion controls which placements search returns, in both arms. A shared chunk stays
  citable while any included document carries it. `embed` embeds everything indexed;
  an excluded document's rows are there, marked excluded, and come back the moment a
  threshold or a person restores it.
- Junk/poor-extraction stays a separate axis from outside-my-preset, and unclassified
  content stays visible as pending rather than counting as irrelevant.
- Blobs and the log are untouched. Every decision is reversible by construction.

## See also

- [Retrieval](RETRIEVAL.md) — the search the presets will filter.
- `CONTEXT.md` — the store's truth/derived split, which is what makes "usage not
  archive" enforceable.
