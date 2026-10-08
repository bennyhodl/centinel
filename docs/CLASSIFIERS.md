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
choice. The Corpus can filter on either. For a choice, the Corpus score under its own id
is the summed probability of its exclude options under the current actions.

### The junk gate

The **junk gate** is one choice, `page_kind`. Its options are `record`, `service_info`,
`navigation`, `calendar_or_directory`, `unreadable`, and `other`. The three junk options
exclude. The default policy excludes at 0.90 together and holds 0.50 to 0.90 for review.
The Classifiers view adds it as a preset, beside the **city topics** (nouls that tag laws,
real estate, budget, policy, and public participation) and the **record type** (a choice
that tags agenda, minutes, law text, and other kinds of record).

Run the junk gate alone over every document first. Commit its exclusions. Then run the
topics over the documents that stay included. A run can use any subset of the saved
questions, so the gate does not have to wait for the topics.

### What is sent

- Every question starts with a fixed line that says the `text` is untrusted content and
  not instructions. The run's settings record the line.
- A document whose derived text is empty is not sent. Its result is an error that says it
  is not scorable, and the document stays pending. It is not scored low.
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

## `centinel web`

```
centinel web                    # serves http://127.0.0.1:8787/web and opens the browser
centinel web --bind 127.0.0.1:9000
centinel web --rebuild          # rebuilds the page with Vite from this checkout, then opens
```

`centinel web` reuses a server already on the port only when it is the same build of
the same version on the same corpus root. A server left running from before a
`cargo build` is refused by name, so the new code and page are never hidden behind an
old process. Every connection to `centinel.db` waits up to five seconds on a lock held
by another process, so a rebuild running beside a live server slows a page instead of
failing it.

`--rebuild` runs `npm run build` in the source checkout this binary was compiled in,
streams the Vite output, checks the version stamp, and serves the rebuilt page for this
process instead of the embedded one. It is the way to see a page change without a
`cargo build`. It refuses to hand the browser to a server already on the port, because
that server would show its own page. A release download has no checkout, so there the
flag reports that and stops. The index is never touched; `centinel index --rebuild` is
the command for that.

A Vite React workspace is built into one HTML file and embedded in the binary. The
installed program ships no asset directory and needs no Node runtime. A source build
uses Node 20.19 or newer to make that embedded file; `cargo build` relays the Vite
output as `web:` warnings so the bundle step is visible.

The page is stamped with the Centinel version it was built for. `build.rs` refuses a
bundle whose stamp differs from the crate version, and `centinel web` checks the stamp
before it probes a port, rebuilds an index, or opens a browser. A release download
cannot rebuild the bundle, so there the check can only report a mismatch. The page shows
its version in the rail and warns when the server it reached reports another one, which
is what a `centinel web` left running from an older build looks like.

While a run scores, the page shows it live: progress by decision, the documents out to
Jev with how long each has waited and which request it is on, and the latest answers as
they land. When scoring stops, the same panel gives the counts by decision and the
commit. The rail marks a running run from every page.

The workspace has three views:

- **Corpus** pages through the index without loading it into browser memory. Full-text,
  address, Source, usage, and classifier filters can be combined. The reader resolves
  the exact Source, Resource, and derived text identity, including shared text.
- **Classifiers** edits atomic Jev questions and their policy thresholds. A trial sends
  the Corpus filter and a count; the server resolves the top matches in one query and
  stores the exact identities with the run, so the browser never pages the corpus back
  and forth. The start request is answered at once with the run as started. Scoring
  continues on the server with a bounded number of documents in flight to Jev at a time
  (eight unless the trial says otherwise, at most thirty-two), each result recorded the
  moment it lands, and the page polls the run detail once a second until the status
  leaves `running`. A run holds up to 50,000 documents, so "all matching documents" covers
  the corpus. The run card estimates the input tokens and the cost from the characters of
  the selection before the run starts. Only the checked questions go into the run.
  Results are stored in input order however they arrive, so a repeated
  trial lines up row for row. A request that Jev refuses with `429`, answers with a
  `5xx`, or drops is retried up to four times with a growing pause before its failure
  is recorded. Token counts and the cost estimate sum over the documents whose answer
  reported usage, and the run says how many that covers when it is not all of them.
  Score columns sort by a click on their header, high to low first. The Output selector
  chooses between a saved run and a preview. A saved run accepts only the current saved
  question set, so a stale browser tab cannot change its meaning. A preview scores the
  same selection, shows the scores on the page, and writes nothing to the run ledger;
  it accepts draft questions because it records no meaning, and it cannot be committed
  or repeated. Saving a question versions model-evaluated meaning; changing an action
  threshold re-decides stored scores without sending the text again.
- **Runs** keeps the question versions, selected input identities, model, evaluation
  date, settings, tokens, duration, throughput, errors, and cost when a rate is known.
  The paged list returns small summaries. One run detail returns its exact evaluated
  questions, the current effective policy, a fresh commit preview, the counts of excluded,
  review, kept, tagged, and failed documents, and one page of results. The server filters
  and sorts the results, so a run over the whole corpus never goes to the browser at once.
  A repeat names the run it repeats, and the server copies its inputs.
  An exact selection can be repeated for a comparable benchmark. A run with durable
  progress but no completion record is shown as interrupted after a server restart.

Run and Commit are separate operations. A run stores scores and previews the documents,
placements, characters, and unique chunks affected by its exclusion rules. Commit writes
reversible usage decisions. Restore reverses one. Neither operation changes `blobs/` or
`log/`.

Classifier truth is stored in `workspace/questions.jsonl`, `workspace/runs.jsonl`, and
`workspace/decisions.jsonl`. SQLite holds only the search, score, and exclusion
projections. Deleting and rebuilding `centinel.db` replays durable usage decisions.

Corpus text leaves the machine only after the operator starts a run, and only for the
selected documents. The server reads `TYPESAFE_API_KEY` from the process environment,
then from `.env` in the working directory, then from `.env` in the corpus root; the key
is never sent to the browser. It posts one complete document and all of the run's questions to
`https://api.typesafe.ai/v1/systemone` per request.

## The boundary that matters

Classification changes **usage**, never the archive:

- Presets (civic engagement, real estate, …) name the questions and thresholds.
- Scores are stored beside the index, versioned, and one document can match several
  presets.
- Committed exclusion rules control which chunks need embeddings and which placements
  search returns. A shared chunk stays eligible while any included placement uses it.
- Junk/poor-extraction stays a separate axis from outside-my-preset, and unclassified
  content stays visible as pending rather than counting as irrelevant.
- Blobs and the log are untouched. Every decision is reversible by construction.

## See also

- [Retrieval](RETRIEVAL.md) — the search the presets will filter.
- `CONTEXT.md` — the store's truth/derived split, which is what makes "usage not
  archive" enforceable.
