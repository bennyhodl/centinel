# Centinel — Scheduling Specification

Cadence, and the boundary that makes it safe: **the server reports on work, it never
causes it.**

This is the design for the part of [#7](https://github.com/bennyhodl/centinel/issues/7)
that SPEC §8 left open — "*`centinel run` walks the sources and every stage skips work it
has already done, so a cron entry is enough for cadence — but when to recrawl … still
belongs to #7*". It settles cadence, idempotency and resumability. It does **not** settle
when `Live` becomes `Gone`, the `fingerprint` normalization rules, or the phantom-diff
policy; those stay with #7 and are named in §12.

It also closes a hole that exists in the code today: `POST /ops/run` is reachable, with no
authentication, on the same server whose access control SPEC §8 calls unspecified.

---

## 1. What scheduling is for

An operator will not type `centinel run` twice a day for a year. So the cadence has to
live somewhere, and the only process that is running anyway is `centinel serve`.

That immediately collides with the rule this document exists to establish, so state both
plainly:

> **The server, MCP, and every consumer of either may read the record. None of them may
> cause the record to grow.**

> **The server runs collection on a cadence its operator wrote down.**

Those are not in tension, and the reason is worth naming precisely, because the whole
design falls out of it:

### 1.1 The scheduler is not a consumer

A scheduled run is not the server deciding to collect. It is **the operator's instruction,
written in the operator's config file, executed later on the operator's machine.** The
authority comes from a file on disk that only the operator can write, not from a request
that anyone who can reach the port can send.

So the test for whether an op may fire is not "is it dangerous" but **"who asked"**:

| Asked by | May cause collection |
|---|---|
| The operator, at the CLI | Yes |
| The operator, through a `[[schedule]]` block they committed | Yes |
| An MCP client | **No** |
| An HTTP caller | **No** |

*Why it matters:* an agent is a client of the record, never its author (SPEC §1). A model
that can trigger a crawl decides what the corpus contains, and "what is collected does not
depend on what any model happened to think that day" (README) stops being true. It is also
the concrete denial-of-service: `POST /ops/run` twenty times is twenty crawls against a
city's web server, attributed to the operator, from a port with no authentication.

### 1.2 A schedule is a saved `run` and a cadence — nothing more

The scheduler introduces **no new pipeline semantics**. It does not decide what to collect,
does not diff anything, does not track state. It fires `run` with arguments the operator
wrote, at times the operator wrote.

Everything that makes this cheap is already true and is inherited, not implemented:
`collect` subtracts observed markers from the latest DiscoveryRun, `extract` skips blobs
that already carry a Derivation or an Underivable, `index` skips placements already
written, `embed` subtracts stored chunk hashes from indexed ones. A second run does nothing
at every stage, for the same structural reason the first one was resumable (`ops/run.rs`).

*Why it matters:* a scheduler that computed its own idea of what was due would be a second,
unenforced copy of five skip predicates that already exist and are already exactly right.
The failure mode is not slowness — it is a scheduler that believes a source is current when
the log says otherwise.

### 1.3 Locked scope

| | |
|---|---|
| **Where it runs** | Inside **`centinel serve`**. No second daemon, no second binary, no systemd timer generator. |
| **What it fires** | **`run`, and only `run`.** Not individual stages, not a job DSL. §11. |
| **Where a schedule is written** | **`centinel.toml`**, beside the sources it names. §3. |
| **Cadence** | **5-field cron in a named IANA zone**, with jitter. §4. |
| **Concurrency** | **One run at a time per store**, enforced across processes. §5. |
| **What is recorded** | **Every attempt**, including the ones that did nothing — in `runs/`, append-only, beside `log/`. §6. |
| **What consumers get** | Two **read-only** ops, `schedules` and `history`, on every surface. §8. |
| **What consumers lose** | `run`, every stage op, `ingest` and `source` disappear from HTTP and MCP. §2. |
| **How one is written** | An **interactive selector** on the CLI that fills in absent arguments. The op itself never prompts. §3.4. |
| **Isolation** | The scheduler and every run it drives get **their own runtime and their own threads**, at lower OS priority. The read path never waits on collection. §5.3. |
| **Where the code lives** | **No new crate.** A pure module in `centinel-core`, a loop beside `http.rs` in the binary. §10. |

---

## 2. The authority axis — `Reach`

`OpDef` carries two exposure flags today: `mcp: bool` (curation — "a model does not need
forty tools") and `local_only: bool` (acts on this machine; excluded from HTTP **and**
MCP, and enforced on call rather than merely hidden from `tools/list`).

Neither expresses the rule in §1.1. `local_only` is about *where* an op acts — `open`
launches a GUI, `models` pulls gigabytes into a host cache. The new question is about
*who may ask*, and it has a third answer that neither flag can spell: **the scheduler may,
and nobody remote may.**

Replace `local_only` with one enum:

```rust
/// Who may cause this op to run.
pub enum Reach {
    /// Anyone who can reach a surface. Read-only by construction.
    Public,
    /// The operator: the CLI, and the scheduler acting on their written instruction.
    /// Never HTTP, never MCP.
    Operator,
    /// The CLI alone. Not even the scheduler — this op acts on the host, and a
    /// multi-gigabyte download must never ambush a 3am run (SPEC §3.6).
    Host,
}
```

| `Reach` | CLI | Scheduler | HTTP | MCP |
|---|:--:|:--:|:--:|:--:|
| `Public` | ● | — | ● | ● *(if `mcp`)* |
| `Operator` | ● | ● | ✕ | ✕ |
| `Host` | ● | ✕ | ✕ | ✕ |

*Why an enum and not a second bool:* two independent booleans describe four states, and
only three exist. The fourth — "the scheduler may fire it **and** so may any HTTP caller" —
is the exact defect this document exists to prevent, and a pair of booleans leaves it
one typo away.

Assignment across the current registry:

| `Reach` | Ops |
|---|---|
| `Operator` | `run`, `discover`, `collect`, `extract`, `transcribe`, `index`, `embed`, `ingest`, `source` |
| `Host` | `open`, `models` |
| `Public` | `search`, `read`, `list`, `doctor`, and the new `schedules`, `history` |

`source` is `Operator` for the same reason as the stages: `source add` changes what a bare
`run` collects. Letting a remote caller add a source is letting it choose the corpus one
step earlier.

### 2.1 The invariant a future op cannot miss

`Group` already partitions the registry, and the partition happens to be exactly right:

> **Every op in `Group::Pipeline` or `Group::Stage` must have `Reach::Operator`.**

That is a test over the whole registry rather than a list of names — the same shape as
`host_local_ops_are_neither_listed_nor_invokable` in `http.rs`, and for the same reason:
the guard has to cover the op somebody adds next year, not the ones somebody remembered.

Two enforcement points, both already built: `op::remote_ops()` and `op::mcp_tools()` filter
the listings, and `http::remote_op` refuses on call so a non-`Public` op is **invisible and
also unreachable**. Hiding alone is not access control.

### 2.2 What the server is still for

Stripping nine ops off HTTP does not hollow it out. What remains is the read/query API
SPEC §1 assigned it — `search`, `read`, `list`, `doctor` — plus the two new ops, plus MCP
over HTTP. The server's job becomes exactly: **serve the record, and keep the collection
current on the operator's cadence.**

---

## 3. Where a schedule lives — `centinel.toml`

A `[[schedule]]` block, in the same file as the `[[source]]` blocks it names.

```toml
[[schedule]]
id      = "tampa-daily"
sources = ["tampa-gov"]
cron    = "0 3 * * *"
tz      = "America/New_York"

[[schedule]]
id      = "council-monthly"
sources = ["tampa-council"]
cron    = "0 2 1 * *"
tz      = "America/New_York"
skip    = ["embed"]          # leave the hours-long stage to the nightly block

[[schedule]]
id      = "nightly-derive"
sources = []                 # every enabled source; the derive stages find the backlog
cron    = "0 1 * * *"
tz      = "America/New_York"
skip    = ["discover", "collect"]
```

### 3.1 The fields are `RunArgs`, plus a cadence

| Field | Meaning |
|---|---|
| `id` | Names this schedule in the journal and in `schedules`. Unique. |
| `cron` | 5-field expression. §4. |
| `tz` | IANA zone name. Defaults to the host zone, which is recorded rather than assumed. |
| `jitter` | Duration; defaults to `5m`. §4.2. |
| `enabled` | Defaults true. |
| `catch_up` | Defaults true. §9.3. |
| `sources`, `skip`, `limit`, `refresh` | **Verbatim `RunArgs`.** |

The last row was going to be the mechanism: embed `RunArgs` with `#[serde(flatten)]` and a
flag added to `run` is schedulable the day it exists, with no second list to fall behind.

**It does not survive contact with serde, and the alternative is better anyway.**
`deny_unknown_fields` cannot be applied to a struct containing a flattened field — the
flattened half absorbs whatever it does not recognise. So a flattened schedule block would
accept `soruces = ["tampa"]` silently, and produce a schedule that fires on time, collects
nothing, and reports success forever. That is the same silence `[[sources]]` typed by
reflex already produces, and the reason this file denies unknown keys at all; it is
strictly worse at 3am than in a terminal.

So the run options are **spelled out**, and the drift they invite is caught by a test
rather than by a mechanism: `run_options_stay_in_step_with_run` serializes `RunArgs` and
asserts every field is either expressible in a `[[schedule]]` block or named in a short
exclusion list with its reason. A new `run` flag is then a failing test, not an omission.

*Foot-gun, deliberately not forbidden:* `refresh = true` on a schedule re-fetches and
re-derives the entire corpus at every fire. It is legal, because "the extractor got better,
re-read everything monthly" is a real operator intent. It is also the single most expensive
thing this file can express, so `schedules` renders it in its own column and the loader logs
it at startup. Refusing it would push the operator to a shell script, where nothing renders
it at all.

### 3.2 Why the config file and not a table in the store

The store has two categories and a schedule fits neither. `blobs/` and `log/` are truth;
`current/`, `centinel.db` and `vectors.lance/` are derived and rebuildable — which is what
makes the index disposable and the corpus `rsync`-able (SPEC §5.4, CONTEXT.md).

A crontab in `centinel.db` would be a third category: mutable state that is neither
evidentiary nor rebuildable. The documentation says deleting `centinel.db` is safe. It
would stop being safe — and the failure is silent, because a store with no schedules looks
exactly like a store that is up to date.

The config file also gets three properties for free that a database table would each have
to earn: it diffs, it reviews, and it is the file the operator already edits.

### 3.3 Creation is a command, because TOML invites two silent mistakes

```
centinel schedule set tampa-daily --source tampa-gov --cron "0 3 * * *" --tz America/New_York
centinel schedule rm  tampa-daily
```

`schedule set` writes one block and stops, exactly as `source add` does — and for the
reason `ops/source.rs` already gives: the mistakes the file invites are silent until a run
is well underway. Here they are an `id` in `sources` that names no `[[source]]`, and a cron
expression that parses cleanly and means something else (`0 3 * * 1` is Mondays, not the
1st). Both are caught at write time, against the config being written.

`schedule set` is `Reach::Operator`. It writes the file that grants authority; reachable
remotely it would be privilege escalation with extra steps.

Its output ends with the line that keeps the operator honest:

```
wrote centinel.toml — a running server has not picked this up; send SIGHUP or restart
```

### 3.4 The selector — `centinel schedule set` with nothing to type

Run with no arguments, on a terminal, it asks:

```
? Schedule id ...................  tampa-daily
? Which sources? (space to select)
  ◉ tampa-gov        site     1,005 addresses · collected 2026-08-05
  ◯ tampa-council    channel    312 addresses · collected 2026-07-29
  ◯ hillsborough     site      4,417 addresses · never collected
? How often?
  ❯ Daily, early morning              0 3 * * *
    Weekly, Sunday early morning      0 3 * * 0
    Monthly, the 1st                  0 2 1 * *
    Twice a day                       0 3,15 * * *
    Custom cron expression …
? Time zone ....................  America/New_York  [host]
? Skip any stages? (space to select)
  ◯ discover  ◯ collect  ◯ extract  ◯ transcribe  ◯ index  ◯ embed

  tampa-daily — every day at 3:00 AM EDT, ±5m jitter
  next three:  Thu 7 Aug 03:04 · Fri 8 Aug 03:04 · Sat 9 Aug 03:04

? Write this to centinel.toml? (y/N)
```

Three things it puts on screen that an operator cannot hold in their head:

- **The source ids that exist**, with kind and when each was last collected — so a
  schedule cannot name a source that is not there, and the one you forgot is visible
  rather than remembered.
- **The cadence in words**, beside the expression that produces it.
- **The next three fire times, in the chosen zone, with jitter applied.** This is the one
  that earns the feature. `0 3 * * 1` is Mondays and `0 3 1 * *` is the 1st, and §3.3
  already names that confusion as a mistake TOML invites silently. A preview turns it from
  a guess into a decision, and it is also where the DST rules of §4.2 become visible
  instead of theoretical.

#### The op never prompts

The wizard is a **CLI-side layer above the op**, not a mode inside it. The op receives a
complete argument set from every surface, exactly as it does today.

*Why it matters:* an op that prompts blocks an MCP call until the client times out, and
hangs a script forever with no output explaining why. It is the same rule `tool.rs` already
enforces one level down — every child process Centinel starts is denied our stdin, because
a tool that reads it is a tool that can wedge the run.

So: prompting happens only when stdin **and** stderr are a terminal. With arguments
missing and no terminal, `schedule set` fails naming what is absent. It never waits.

Validation is unchanged and shared: the wizard cannot produce a block that `schedule set
--id … --cron …` would have rejected, because both hand the same struct to the same
validator.

#### The crate: `dialoguer`

It is built on `console`, which is **already in the tree under `indicatif`** — the same
reasoning, in the same words, that the binary's `Cargo.toml` already gives for depending on
`console` directly: *"Already in the tree under indicatif, so this costs no new download."*

`inquire` is the better-known one and its API is nicer, but it sits on `crossterm`. That
puts a second terminal backend in a binary whose progress renderer already drives one, and
two libraries deciding independently what to do about raw mode and cursor position is a
class of bug nobody wants inside a tool that also prints hours of progress bars.

### 4.1 Why cron and not an interval

`every = "24h"` drifts. A 24-hour interval that fires after a 40-minute run moves 40
minutes later every day and walks into business hours within a fortnight — against a city's
web server, which is precisely what SPEC #4's politeness stance is about. It also cannot
say "02:00 on the 1st", which is the natural cadence for a source that publishes monthly.

Cron says both, and every operator who would deploy this already knows it.

### 4.2 The zone is part of the schedule

`.gov` publishing is a business-hours phenomenon in a specific city, and an operator
reasons in local time. So `tz` is an IANA name, not an offset, and it is **recorded rather
than inherited silently** — a server that moves machines must not quietly shift its
collection by five hours.

`jiff` is already the workspace's time library and already does zoned arithmetic, so the
two ugly cases get answered rather than discovered:

| Case | Rule |
|---|---|
| A fire time that does not exist (02:30 on a spring-forward day) | Shifted forward by the length of the gap, so it fires at 03:30. |
| A fire time that happens twice (autumn fall-back) | Fires **once**, at the earlier of the two. |

Getting these wrong is a missed day and a doubled day, once a year each, in the direction
nobody is watching.

The first rule is jiff's `compatible` disambiguation, which is also what Temporal and ICU
do. "The first instant after the gap" was the original wording here and is worse: it
collapses an 02:00 schedule and an 02:30 schedule onto the same minute, on the one morning
of the year they would otherwise stay half an hour apart. A bespoke rule would also be a
second answer to a question the ecosystem has already settled.

### 4.3 Jitter, because forks are the point

The licence is MIT and "forks are the point — other cities run their own instance"
(SPEC §1.4). Twenty installs sharing a default `0 3 * * *` and a handful of shared vendor
platforms — Granicus, Legistar, Swagit — is a small synchronized flood at 03:00, from a
project whose stated stance is politeness.

So every schedule carries jitter, defaulting to five minutes. It is **deterministic per
install** — derived from the node identity and the schedule id, not random — so the fire
time is stable and predictable to its own operator, and `schedules` can print the real one.
An operator who wants exactly 03:00 sets `jitter = "0s"`.

---

## 5. One lane

**At most one run executes at a time, per store.** Not per schedule, and not per process.

Three reasons, in descending order of how quickly they bite:

1. `transcribe` and `embed` each build a multi-gigabyte model. Two at once thrash, and both
   finish later than either would have run alone. Loading the model once is the whole reason
   `run` is two-phase (`ops/run.rs`).
2. Every stage's work list is a **subtraction computed at the start**. Two concurrent runs
   over the same source compute the same list and do all of it twice.
3. `index` commits per document into one SQLite file.

### 5.1 The queue: depth one per schedule, FIFO, no merging

A fire that arrives while a run is in flight is queued. A fire that arrives while **its own
schedule** already has a pending entry is dropped and recorded as `skipped: busy`.

The rejected alternative is coalescing — merging two due schedules into one run with the
union of their sources and some reconciliation of their `skip`, `limit` and `refresh`. That
produces a run **no operator wrote**, whose arguments are the resolution of a conflict
nobody was asked about, and files it in the journal under a schedule id that does not
describe it. Two back-to-back runs cost one extra pass of five skip predicates over a
current corpus; a fabricated run costs the record's honesty.

The cheapness of the second run rests on one invariant, which is already how `embed` is
written:

> **A stage with an empty work list must not load its model.** `embed` opens the vector
> table, computes the outstanding set, and returns before touching the model — the comment
> at `ops/embed.rs:133` says so explicitly. `transcribe` must hold the same property.

### 5.2 The lock is on disk, because the CLI is a second process

The queue is in-process and therefore useless against the operator typing `centinel run`
while the server's scheduler is mid-run. That is not an exotic case; it is Tuesday.

So: `<root>/run.lock`, written by **any** run from any surface, holding the pid, the start
timestamp, the trigger and the arguments. A second run refuses:

```
a run has been in flight since 03:00:12 (pid 4133, schedule tampa-daily) — 12 minutes
```

`--force` overrides for the case where the holder is genuinely dead in a way the pid check
missed. A scheduled fire that finds the lock held by another process is recorded as
`skipped: busy`, with the holder named — so "my cron never ran" has an answer in the
journal rather than in a hunch.

The lock is also the answer to "is a run happening right now", and it is on disk, so the
CLI and the server give the **same** answer to that question. §8.

### 5.3 The scheduler does not share the request path

A run must never make `search` slow to answer, and the reason is not politeness: a server
that stops answering for four hours every night is one an operator stops trusting, and the
whole point of §8 is that consumers can ask how fresh the corpus is *while* it is being
refreshed.

**The discipline already exists in the stage code.** `embed` runs its inference loop inside
`spawn_blocking` rather than on a runtime worker, and the comment at `ops/embed.rs:205`
gives exactly this reason — *"an HTTP caller's connection has to stay responsive across
hours"*. `search` does the same for scoring and for query embedding. Transcription and
document extraction are child processes (`tool.rs`), so from the runtime's point of view
they are I/O and not CPU at all.

So the request path is already protected — **by six call sites each remembering.** That is
the shape of problem `tool.rs` was written to end: *"seven call sites used to make those
choices separately, and all seven made none of them."*

#### The decision: a second runtime, on its own threads

`serve` builds a **separate multi-thread runtime** for the scheduler, and every run it
drives lives there. axum keeps the main one.

```text
  main runtime      axum handlers · search · read · list · MCP        latency work
  scheduler runtime the timer, the queue, and every run it fires      throughput work
```

*Why a runtime and not `tokio::spawn`:* a spawned task shares the workers. One stage that
forgets `spawn_blocking` — a future extractor, a future arm, an inner loop somebody
inlines — takes a worker for hours, and on a four-core machine that is a quarter of the
request capacity gone until the run ends. Runtime separation makes the read path's
correctness **independent of every stage's blocking discipline**, instead of contingent on
all of them staying right forever.

Sizing: `max(1, cores / 4)` by default, configurable. Collection is throughput work and
spends most of its wall clock asleep in a per-host pacer at one request per second; it does
not need the machine to make its deadline.

#### Priority, because separation is not isolation

State the limit plainly, because it is easy to oversell this:

> **Separate runtimes prevent starvation. They do not prevent contention.** A request never
> waits on a worker that will not yield for four hours. It can still be slower because the
> machine is genuinely busy embedding.

For contention the answer is the operating system's, not the runtime's: the scheduler's
threads start at **lower OS priority** (`nice +10` on unix, via tokio's `on_thread_start`
hook). Collection is throughput work, search is latency work, and that is precisely what
the nice value is for. It costs one hook and no design.

One contention this does *not* fix, and it is worth naming: memory. A search loads the
reranker and a query embedder while a run holds the embedding model. Two multi-gigabyte
residents on a machine chosen for neither. §12 flags it.

#### Cancellation: a token, never `abort()`

Shutdown (§9.4) and `--no-schedule` reload both need to stop a run in flight.

**`JoinHandle::abort()` is the wrong mechanism here.** It cancels at whatever await point
the task happens to be sitting on, and some of those are inside a log append or a blob
write. A partial JSONL line in `log/<source>/` is corruption of the one thing this project
calls truth — and it is corruption of a file whose format has no way to say "this line was
interrupted".

So a `CancellationToken` (tokio-util, already in the tree) travels with the invocation and
is checked at **item boundaries** — between addresses in `collect`, between blobs in
`extract`, between batches in `embed`. The same places `Progress` already reports from,
which is not a coincidence: an item boundary is exactly the point at which the record is
consistent and the work so far is durable.

Plumbing: it travels **beside** `Progress` in the invoke signature, not inside it.
`Progress` is deliberately one-directional and documents that a dropped receiver is not an
op's problem; making it the cancellation channel too would quietly change that contract for
every existing call site. The cost is one more parameter on `InvokeFn` and on the `#[op]`
expansion, paid once.

Cancelling loses nothing (§6.6): every stage is resumable by construction, and the next
fire subtracts what was already done.

---

## 6. The record — `runs/`

```text
<root>/
  blobs/ab/cd/…              TRUTH    immutable, content-addressed
  log/<source>/YYYY-MM.jsonl TRUTH    append-only, per Source
  runs/YYYY-MM.jsonl         TRUTH    append-only, one record per attempt   ← new
  run.lock                            the in-flight run, if any             ← new
  current/, centinel.db, vectors.lance/   DERIVED
```

`runs/` is truth, and it is the third thing that is. It is a record of **what this machine
did**, where `log/` is a record of **what the world served**.

### 6.1 Why it is not derived from the log

A quiet run — the common outcome, and the one a watchman exists to produce — writes nothing
to `log/`. So "the schedule fired at 03:00 and everything was current" is not recoverable
from the log, and is indistinguishable there from "the schedule has not fired since March".

That is the whole report, and it needs its own record.

### 6.2 Why not `centinel.db`

Same argument as §3.2: `rm centinel.db` is documented as safe, and it must not erase the
history of collection. `runs/` sits beside `log/`, in the same JSONL-per-month format, for
the same reason.

### 6.3 Why not under `log/<source>/`

A run spans sources and a quiet run touches none. Filing it per source would either
duplicate one record N times or lose it entirely.

### 6.4 One record per **attempt**

Not per run. A fire that was skipped because the lane was busy is a record; so is one that
was interrupted by shutdown. The three outcomes that produce no work are the three most
likely explanations for "why is this corpus stale", and dropping them leaves the question
unanswerable.

```jsonc
{
  "run_id":      "2026-08-06T07:00:11Z",   // the start instant; see §6.5
  "schedule":    "tampa-daily",            // null for a manual CLI run
  "trigger":     "schedule",               // schedule | manual | catch_up
  "due_at":      "2026-08-06T07:00:00Z",   // with jitter applied
  "started_at":  "2026-08-06T07:00:11Z",
  "finished_at": "2026-08-06T07:41:52Z",
  "outcome":     "ok",                     // ok | partial | failed | skipped | interrupted
  "added":       { … },                    // §7
  "subtracted":  { … },                    // §7
  "report":      { …RunReport… }           // verbatim
}
```

The whole `RunReport` is embedded verbatim — a few kilobytes, per-source and per-stage,
carrying `summary` and `error` separately as `StageRun` already does. *Why:* every surface
already knows how to render it. A summarised copy would be a second vocabulary for the same
facts, and the first thing anyone asks of a failed run is the detail a summary dropped.

### 6.5 The run id is its start instant

`2026-08-06T07:00:11Z`, and `history --run 2026-08-06T07` resolves it by prefix.

Two properties, deliberately: the lane is single, so start instants are unique within a
store and sort in the order the runs happened. And it obeys the rule the rest of the tool
obeys — **anything Centinel prints, Centinel takes back, by prefix** (CONTEXT.md, on
handles). It costs no dependency and no id scheme.

### 6.6 A crash leaves evidence

The record is appended at **finish**, so a killed process leaves no record — only a stale
`run.lock`. On startup the scheduler finds it, checks the pid, and converts a dead holder
into an `interrupted` record carrying whatever the lock knew: schedule, arguments, start
time. Then it proceeds.

Nothing is lost by the interruption itself. Every stage is resumable by construction, so
the next fire picks up exactly where the killed one stopped — which is the property the
pipeline already had, now visible in the journal.

---

## 7. Additions and subtractions

The two numbers a scheduled run exists to produce. They are **not symmetric**, and treating
them as if they were records something false.

### 7.1 Addition — bytes that entered the corpus

Straight off the `RunReport`, which already computes it: `new_documents`, `new_chunks`, and
`StageRun.new` per stage. A new address and a new **version** of a known address are both
additions, because every version is retained (SPEC §1.4). **A page changing is an addition,
never a subtraction** — the previous version is still there, still addressable, still
searchable.

### 7.2 Subtraction — and nothing is ever removed

> **A Centinel corpus never loses anything.** No blob is deleted, no Observation is
> retracted, no log line is rewritten.

A "subtraction" is therefore never a deletion. It is an address that **stopped appearing**,
or one that **started refusing** — and those are three different facts that the model
already distinguishes. Collapsing them into one number is how a live page gets recorded as
deleted:

| | What it is | Where it comes from |
|---|---|---|
| **Vanished** | Present in the previous DiscoveryRun, absent from this one. | The discovery delta — snapshot against snapshot (§4.3). The site stopped listing it. It may still be served. |
| **Gone** | Fetched, and the server said 404 or 410. | A `Liveness::Gone` transition on `ResourceStatus`. |
| **Blocked** | Refused in a way that is **not evidence of absence** — WAF 403, 429, robots denial, the YouTube bot wall. | A `Liveness::Blocked` transition. |

*Why it matters:* `Blocked` exists precisely because "a CloudFront 403 would otherwise be
indistinguishable from the page not changing, and recording it as `Gone` would log a live
page as deleted" (CONTEXT.md). A report that sums the three re-introduces that mistake at
the exact moment an operator is scanning for whether anything broke. So the record carries
three counts, the renderer prints three columns, and nothing ever adds them.

A fourth line belongs beside them and is neither: **`Error`** — a transport fault, a
timeout, a hang that had to be killed. Evidence about this machine, not about the page.

### 7.3 The counts are in the record; the addresses are in the log

The run record carries counts and a bounded sample. The *set* of vanished addresses is
recoverable from the last two DiscoveryRuns via `Replay`, and the liveness transitions are
in `log/<source>/`.

*Why:* a run record that inlined every vanished URL would be a second copy of the record,
divergent the first time either is written by a different version. `history` reports the
number and names the source; the log answers *which*.

This does need one addition upstream: `discover` computes the delta as a count today
(`ops/discover.rs:239`). It must also record the vanished **set** — or at least its size and
a sample — into the `StageRun` figures, or the run record has nothing to carry.

---

## 8. The consumer surface — two read-only ops

```
GET  /ops/schedules      centinel schedules      MCP: schedules
GET  /ops/history        centinel history        MCP: history
```

Both are ops in the registry, not hand-written routes — "routes are the registry" is the
first line of `http.rs`, and honouring it means the operator gets a rendered CLI view and a
model gets a tool, from one definition, with no third code path.

### 8.1 `schedules` — what is configured, and when it last ran

| Column | Source |
|---|---|
| id, cadence in words, zone, enabled | the config |
| `refresh`, `skip`, `limit` | the config — the expensive settings, made visible |
| next fire (with jitter applied) | computed |
| last fire, last outcome, consecutive failures | `runs/` |
| running now | `run.lock` |

Because "running now" comes off the lockfile rather than out of the server's memory, the
CLI and the HTTP surface give the **same** answer — including when no server is running at
all.

### 8.2 `history` — the attempts, with their arithmetic

Filterable by `--schedule`, `--source`, `--since`, `--failed`; one record per attempt,
newest first; each carrying additions, the three subtractions, elapsed, and outcome. A run
id resolves by prefix (§6.5), and `history --run <id>` prints the embedded `RunReport` in
full.

### 8.3 Why it is `history` and not `runs`

`centinel runs` and `centinel run` differ by one keystroke, and one of them starts an hour
of network traffic against a city. That is not a naming preference; it is the kind of
collision that eventually costs somebody a 3am incident.

### 8.4 This is the useful half of the ask

An agent can now ask **when the corpus was last collected, whether the last attempt failed,
and how much came in** — and qualify its answer accordingly: *"the last collection of
`tampa-gov` was nine days ago and it was blocked."* That is the same honesty as reporting
`vectors_indexed` beside `total_chunks_indexed` (CONTEXT.md, on rank vs pool): an absent
stage is a different answer, not a slower one.

What it cannot do is send the watchman out. **It can ask what he saw and when he last
walked.**

---

## 9. Server lifecycle

### 9.1 Startup: an invalid schedule refuses to start

`centinel serve` validates every `[[schedule]]` before binding: cron parses, `tz` resolves,
every id in `sources` names a real `[[source]]`, `skip` names real stages, ids are unique.
**Any failure is fatal to `serve`.**

This matches the config loader's existing stance — unknown keys are rejected rather than
ignored, because `[[sources]]` typed by reflex would parse cleanly and collect nothing
(SPEC §8). A server that starts happily with a broken schedule collects nothing and says
so nowhere; the operator finds out in a month, from an empty search result.

The failure is loud at the one moment it is cheap: while somebody is watching it start.
`centinel schedules --check` gives the same validation without starting anything.

`--no-schedule` runs the read/query API with no scheduler — the right mode for a machine
that serves a corpus somebody else collects.

### 9.2 Reload

SIGHUP re-reads and re-validates the config. **A reload that fails to validate keeps the
running schedule and logs the error** — the running configuration is known-good and a typo
must not disarm it. A restart is always correct and always sufficient; SIGHUP exists so
that `schedule set` on a live server is not a restart.

### 9.3 Catch-up: once, never a backlog

The laptop was closed. The server was down for a fortnight. On startup, for each schedule
whose last recorded attempt is older than one interval, fire **once**, immediately, with
jitter, recorded as `trigger: catch_up`.

Once, never N times. Six missed daily fires are one fire, because the pipeline is a
subtraction and not a queue of deltas — six catch-up runs would find the same work and do
it once, then do nothing five times, against a city's web server, in a burst. `catch_up =
false` opts out for a schedule where a missed window genuinely means "wait for the next
one".

### 9.4 Shutdown

SIGTERM stops accepting fires, cancels the in-flight run at the next item boundary, writes
an `interrupted` record, releases the lock, and exits. It does not wait for a four-hour
transcription to finish.

Losing work is not a concern, and the reason is the one from §1.2: everything is resumable
by construction. The next fire subtracts what was done from what is needed and continues.

---

## 10. Where the code lives — **two modules, no new crate**

Federation gets its own crate (FEDERATION §1.4). Scheduling does not, and the difference is
worth stating because the two decisions look inconsistent from a distance.

### 10.1 Most of it is already claimed by rules that exist

Work out where each piece has to go and there is very little left over:

| Piece | Where, and why it has no choice |
|---|---|
| The `[[schedule]]` block | `config.rs`. It is the file's schema, and unknown-key rejection lives there. |
| `runs/`, `run.lock` paths | `store.rs`. **"The layout is named in `store` and nowhere else"** — a path spelled out by a caller is a second, unenforced copy of that header (CONTEXT.md). |
| `Reach` | `op.rs`. It is a registry field. |
| `schedules`, `history` | `ops/`. Link-time `inventory` registration is what makes them appear on all three surfaces. |

What remains is genuinely new: cron parsing, next-fire in a zone, jitter, catch-up
arithmetic, the journal's reader, and the loop that drives them.

### 10.2 The split is "when" and "then do it"

```text
centinel-core/src/schedule.rs   pure. parse a cron, answer "when next", read the journal,
                                decide what is due. No timers, no tasks, no I/O beyond
                                the store. Unit-testable at any instant, on any date.

centinel/src/schedule.rs        the loop. sleep until due, take the lock, invoke `run`
                                through the registry, append the record. Beside http.rs
                                and mcp.rs.
```

The core half is not optional: `schedules` computes next-fire times and it is an op, so
that logic has to be in core regardless of where the loop goes. Given that, the loop
belongs with `http.rs` and `mcp.rs` because it is **the same kind of thing they are** — a
surface that drives the registry and owns no domain logic. The scheduler is the fourth
one, and the first that nobody has to authenticate.

Testing follows the split: everything about *when* is a pure function of `(cron, tz,
journal, now)` and needs no clock, which is the only way the DST rules of §4.2 and the
catch-up rule of §9.3 get tested at all.

### 10.3 Why federation earns a crate and this does not

Federation brings Iroh, QUIC, a key scheme and a wire protocol — a dependency surface that
must not be linked into a build that only wants a CLI. Scheduling brings a cron parser and
a timer.

A crate boundary exists to keep something *out*. This one would keep nothing out, and would
cost a `pub` on every core type it touches — turning internal detail into API surface to
buy an import path.

---

## 11. Accepted costs

| Cost | Why it is accepted |
|---|---|
| **A schedule can only fire `run`.** No "just re-embed at 2am" without a `skip` list that spells it. | The seam for a general job kind is hypothetical until there are two kinds. A federation pull (FEDERATION §6.4) is the likely second — that is when the seam gets drawn, not now. One adapter is a hypothetical seam; two is a real one. |
| **Two due schedules run back to back**, each paying a corpus-wide derive pass. | The second pass over a current corpus is five subtractions that find nothing, and §5.1's invariant keeps it off the model loader. Cheaper than a fabricated merged run. |
| **The cadence is fixed, not adaptive.** A source that publishes hourly and one that publishes yearly are both polled on whatever the operator wrote. | Adaptive cadence needs a change model, and the `fingerprint` normalization rules — which decide what "changed" even means — are still open in #7. Building adaptivity on an unsettled change signal builds it wrong. |
| **No backoff on repeated failure.** Ten failed nights keep the same cadence. | Backing off automatically turns a WAF block into a silently reduced cadence, which is the same class of error as recording `Blocked` as `Gone`. `consecutive_failures` is reported instead, and the operator decides. |
| **`runs/` grows forever.** | A record per attempt per schedule is kilobytes a day. Month-partitioned JSONL, like the log. Anything that prunes it is a policy decision about evidence, and this design does not make one. |
| **The scheduler is single-node.** | Federation is pull-only on the receiver's schedule (FEDERATION §6.4). Nothing here coordinates across peers, and nothing here forecloses it. |

---

## 12. Not yet specified

| | |
|---|---|
| ~~The cron parser~~ | **Settled: hand-rolled**, in `centinel-core/src/schedule.rs`. Every candidate crate is built on `chrono`, which is in the tree transitively but is not the workspace's time library, and adopting one would have put a second date library on a direct path to buy the *easy* half. The hard half — stepping through local time across a DST boundary — is jiff's either way. The grammar is five fields and fits on a page, including the rule that day-of-month and day-of-week are ORed when both are restricted, which is the one place cron does not intersect and the one place a hand-rolled parser gets it wrong. |
| **Whether `runs/` federates** | It is truth, and it sits beside `log/`, so FEDERATION §7's table has a new row to answer. The likely answer is **no**: the run journal is a record of what one machine did, not of what the world served, and a peer receiving a slice has no use for your 3am timings. But it is exactly the kind of row that gets answered by accident by an implementation that copies whole directories. |
| **Resident memory when a run and a search overlap** | §5.3 separates the runtimes and lowers the scheduler's priority, which settles CPU. It settles nothing about RAM: a `search` loads the reranker and a query embedder while a run holds the embedding model, and `transcribe` holds a whisper model beside both. The floor machine of [#13](https://github.com/bennyhodl/centinel/issues/13) is where this stops being theoretical, and #13 is where it should be answered — the options (refuse a run below a free-memory threshold, unload between stages, serialise the two) are hardware-profile decisions, not scheduling ones. |
| **The store lock's strength** | An advisory pid file is enough for two cooperating processes on one host. It is not enough for a store on a network filesystem, which is a configuration nothing else in Centinel supports yet either. |
| **Access control** | Still SPEC §8, still unspecified, and this design narrows it without closing it. The write surface is gone from HTTP; the read surface still hands the whole corpus to anyone who can reach the port. Loopback is still the default and the non-loopback warning still fires. |
| **When `Live` becomes `Gone`** | Still [#7](https://github.com/bennyhodl/centinel/issues/7). §7.2 reports the transitions; it does not decide how many consecutive refusals cause one. |
| **Vendor `LastModifiedUtc`** | Still #7. A schedule that could trust a vendor timestamp instead of a crawl would change what a fire costs — but not what a schedule *is*, so this design does not block on it. |

---

## 13. Out of scope

| | Why |
|---|---|
| **Notifications, webhooks, email on failure** | `history` is pollable and `schedules` reports `consecutive_failures`. An operator who wants a page has a monitoring system already; a project that grows its own alerting stack grows an SMTP configuration. |
| **A scheduling UI** | SPEC §9: no browsing surface. |
| **Distributed or multi-node scheduling** | §11. |
| **Per-source adaptive cadence** | §11, and blocked on #7's change model. |
| **Arbitrary command execution on a schedule** | The reason `open` is `Reach::Host`. A schedule that can run a command template is a remote shell one config write away. |
