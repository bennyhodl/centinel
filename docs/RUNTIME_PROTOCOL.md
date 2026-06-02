---
title: Centinel — Runtime Protocol (LOCKED)
status: 🔒 Locked v1
created: 2026-04-26
parent: README.md
---

# Centinel Runtime Protocol

How the agents communicate at runtime. Locked 2026-04-26 (plan checkpoint v7).

## Principles

1. **Filesystem is truth.** Single machine, centinel-server cron runtime. No message queue, no Redis, no broker. Folders of markdown + a SQLite DB + a git-tracked wiki are sufficient and inspectable.
2. **State changes ride on shared substrate, not messages.** New entity → wiki page + DB row. Other agents read from there next run. Don't double-write a "hey I made this page" memo.
3. **Messages are for requests and escalations.** "Please do X" or "you need to see Y" — anything that doesn't fit on a wiki/source/finding page.
4. **Synchronous within a run, async across runs.** When Investigator calls Archivist mid-crawl, it's a function call. When Investigator wants Archivist to OCR a 400-page budget book, it drops an inbox message and moves on.
5. **Operator queue is sacred.** Agents drop, never drain. Operator drains. Aging items get nudged in the daily huddle and weekly briefing.
6. **Status board is public.** It IS the transparency layer. The web app renders it live so anyone can see what the agents are doing right now.

## Folder layout

```
<wiki>/_runtime/
├── inbox/                           # work requests waiting for recipient
│   ├── archivist/
│   ├── cartographer/
│   ├── investigator/
│   ├── data-reporter/
│   └── watch-runner/
├── outbox/                          # processed messages, audit trail
│   └── <agent>/<YYYY-MM>/...        # rotated monthly
├── operator-queue/                  # human review queue, never auto-drained
│   ├── entity-merges/
│   ├── watch-tuning/
│   ├── findings-draft-aging/        # drafts >14 days old, auto-promoted here
│   └── broken-watches/
├── status/
│   ├── board.md                     # in-flight work, all agents (renders on web)
│   └── <agent>.md                   # per-agent last-known state
└── huddle/
    └── <YYYY-MM-DD>.md              # daily roll-up: each agent's did/will/blocked/threads
```

`<wiki>/_runtime/` is git-tracked. Outbox grows; rotate monthly. `inbox/` and `status/` rebalance as messages flow.

## Message file format

YAML frontmatter + markdown body. One file per message.

**Filename:** `<YYYY-MM-DD>-<HHMM>-<from>-<short-slug>.md`
(directory listing sorts chronologically without parsing frontmatter)

```yaml
---
id: 2026-04-26-0001-abc123              # sha256(from, to, type, references) truncated + date
from: investigator
to: archivist
type: request                            # request | response | notify | escalation
priority: normal                         # low | normal | high | critical
created: 2026-04-26T14:32:11-04:00
expires: 2026-04-29T00:00:00-04:00
correlation_id: null                     # set on response, matches request id
status: pending                          # pending | in_progress | done | rejected | expired
references:
  investigation: parks-contractors
  vault_paths: []
  wiki_pages: []
  db_rows: []
response_required: true
---

## Body

Vault these 5 URLs and return entity hints. Discovered crawling parks/contractors.

- https://www.tampa.gov/.../award-2024-031.pdf
- https://www.tampa.gov/.../rfp-2026-014.pdf
- ...

Reason: hit during depth-crawl from `parks-contractors` investigation, depth=2 from
seed `tampa.gov/procurement/awards`. Parser hint from sitemap entry: `tampa-budget-pdf`.
```

### Lifecycle

```
pending  ──(recipient claims)──►  in_progress  ──(work done)──►  done
                                         │
                                         ├──(can't / won't)──►  rejected
                                         │
                                         └──(deadline passed)──►  expired
```

When a message reaches a terminal state (done | rejected | expired):
- Frontmatter `status` updated
- File moved from `inbox/<agent>/` to `outbox/<from>/<YYYY-MM>/`
- If `response_required: true` and terminal state is `done` or `rejected`, recipient writes a response file with `correlation_id: <original-id>` directly into `outbox/<recipient>/<YYYY-MM>/` (also sends a copy to the original sender's inbox if the sender needs to react)

### Idempotency

The `id` is `sha256(from + to + type + references)` truncated + date. Re-issuing the same request produces the same id; recipient checks for prior id in outbox and skips duplicates.

### Expiry

Each agent's run starts by sweeping its own inbox: any `expires` past current time → move to `outbox/_expired/`. Expired = recipient never picked up; sender can re-issue if still needed.

## Status board (`status/board.md`) — the public artifact

The single source of truth for "what's happening right now." Each agent updates its section at start and end of run. The web app renders this live at `/status` for public viewing — agents working in the open.

```markdown
# Centinel Runtime Status — 2026-04-26 14:32 EDT

## In flight
- [Investigator] parks-contractors run, page 23/47, ETA 2026-04-26 16:00
- [Archivist] OCR queue: 12 PDFs (FY2026 budget book in front)
- [Watch Runner] paused: errant-spending overflowed last night, tuning request open

## Blocked
- [Investigator] waiting on tampa-budget-pdf parser update (Cartographer suggested for 6 URLs)

## Operator queue (3 items)
- 1 entity merge awaiting confirmation (Acme Construction LLC ↔ ACME Construction Co)
- 1 watch tuning request (errant-spending fired 73 times)
- 1 finding aging in draft (parks-no-bid-pattern, 18 days old)

## Last 24h activity (compact)
- Cartographer: lint run, +3 new URLs, 1 broken
- Investigator: parks-contractors, 47 pages crawled
- Archivist: 11 docs vaulted, 6 summaries written
- Data Reporter: 4 alias reconciliations, 2 in operator queue
- Watch Runner: 73 hits → auto-paused

_Last updated: 2026-04-26 14:32 EDT — by Watch Runner_
```

### Update protocol

- Each agent's run **must** end by editing `status/board.md`:
  - Remove its `In flight` entry
  - Add `Last 24h activity` line if anything notable happened
  - Update `Blocked` and `Operator queue` counts if changed
  - Bump the timestamp + signature line at the bottom
- Each agent's run **must** start by editing `status/board.md`:
  - Add an `In flight` entry with ETA
  - Same timestamp bump
- `status/board.md` is rendered live by the web app at `/status` — public.
- A nightly compact-and-render cron rebuilds the page, prunes 24h-aged items, and produces a clean snapshot.

### Concurrency

Two agents can run simultaneously. Edits to `status/board.md` use a file lock (`flock` on `status/.board.lock`); contention is rare (board edits are sub-second). If lock contention exceeds 5s, agent logs a warning and falls back to a stamped append in `status/_pending/<timestamp>-<agent>.md` for the next compact pass to merge.

### Per-agent state files (`status/<agent>.md`)

Optional, advisory. Each agent's longer-form "what I'm in the middle of, restart-safe" state — for resuming after a crash. NOT the truth board. Just an agent's private scratchpad that the agent itself reads next run to remember what it was doing.

## Daily huddle (`huddle/<YYYY-MM-DD>.md`)

After the last agent of the day finishes (or at a fixed nightly time), a small `huddle-roll-up` cron job concatenates each agent's run-log into one digest. Mirrors the Spotlight 4 prompts.

```markdown
# Daily Huddle — 2026-04-26

## Cartographer
- **Did:** lint run, +3 new URLs (`needs_review`), 1 broken
- **Will:** nothing scheduled tomorrow
- **Blocked:** none
- **New threads:** new URL `tampa.gov/procurement/awards-fy2026` looks like a fresh portal

## Investigator (parks-contractors)
- **Did:** crawled 47 pages, vaulted 6, found 3 new contractors
- **Will:** re-run weekly Sun
- **Blocked:** parser hint missing on 6 URLs (filed inbox/cartographer)
- **New threads:** ACME shows up 4× in 2 months — worth its own follow

## Archivist
- **Did:** vaulted 11 docs, OCR queue 12 deep
- **Will:** OCR backlog over weekend
- **Blocked:** none
- **New threads:** big budget book FY2026 came in, will summarize Sunday

## Data Reporter
- **Did:** reconciled 4 alias merges, 2 sent to operator queue
- **Will:** weekly backup tonight
- **Blocked:** none

## Watch Runner
- **Did:** ran nightly, errant-spending overflowed and auto-paused
- **Will:** wait on operator tuning
- **Blocked:** errant-spending paused
- **New threads:** none (paused)

## Operator agenda
1. ⚠ Decide on errant-spending tuning (Watch Runner blocked until done)
2. Approve 2 entity merges (Data Reporter queue)
3. Review 1 aging draft finding (>14 days)
```

This file is what the operator reads with coffee. The Briefings Writer pulls from these huddles for the weekly digest.

## Operator queue

The most important channel — where agents hand work to the human. Each subdirectory has its own conventions but all entries are markdown with frontmatter:

```yaml
---
id: 2026-04-26-merge-001
type: entity-merge
from: data-reporter
created: 2026-04-26
priority: normal
status: open                  # open | resolved | dismissed
references:
  entities: [acme-construction-llc, acme-construction-co]
  confidence: 0.87
---

## Decision needed
Two entities look like the same contractor:
- ACME Construction LLC (id 142, slug acme-construction-llc, first seen 2024-03)
- ACME Construction Co. (id 488, slug acme-construction-co, first seen 2026-01)

Same address (123 Main St, Tampa). Different EIN per SunBiz (one expired).
Confidence 0.87 — too low for auto-merge.

**Options:**
- Confirm merge → keeper id 142, alias added
- Reject → both stay separate
- Defer → leave open, gather more evidence
```

Operator drains queue interactively. Agents respect `status: resolved | dismissed` next run.

## Rules of the road (locked)

1. **State changes don't go in messages.** Wiki page or DB row.
2. **Messages = requests and escalations only.**
3. **Synchronous inline call** if the recipient runs in this same run and the work is bounded. **Async inbox message** if the work crosses runs.
4. **Operator queue is sacred.** Drop only.
5. **No broadcast.** Every message has exactly one `to`. Multicast = N messages.
6. **Outbox is forever** (rotated monthly).
7. **Expiry catches cobwebs.** Sweep on every run start.
8. **Idempotent IDs.** Same request → same id → dedup at recipient.
9. **One source of truth for "what's happening": `status/board.md`.** Public-facing.
10. **Per-agent state files are private scratch.** Don't read another agent's state file.

## What this is NOT

- Not a message queue. Filesystem ordering + monthly outbox rotation handles civic-data volumes (10s–100s of messages/day).
- Not a chat. Agents don't converse. Request → response, single round-trip per message.
- Not a substitute for the wiki. The wiki is the brain; messages are the to-do lists.
- Not a real-time alerting system. Daily huddle + operator queue + briefings handle the operator's attention budget intentionally.

## Web app rendering

`/status` page serves a live render of `status/board.md` plus an "Activity" feed pulled from outbox/ over the last 7 days (sender, recipient, type, summary — vault paths and DB row IDs linkified). This is the public transparency layer:

- Anyone can see which investigations are running
- Anyone can see what watches fired and when
- Anyone can see how long items sit in the operator queue (accountability for the human)
- Anyone can see what the agents are working on right now

The `/status` page is the project's "show your work" surface — civic-data done in the open, not in a black box.

## Spotlight model mapping

| Spotlight | Centinel runtime |
|---|---|
| Morning huddle (15–20 min, 4 prompts) | `huddle/<date>.md` rolled up nightly |
| Status board (kanban in the locked room) | `status/board.md` (public) |
| Interview memo / document summary | `Sources/<slug>.md` (already specced in `civic-archivist`) |
| Master story memo | Investigation page run-log (already specced in `civic-investigator`) |
| Editor flagging follow-ups | Inbox messages |
| Pre-publication review | Operator queue + Reviewer human role |
| End-of-day lockdown | Each agent appends huddle entry, updates status board |

## Verification (acceptance criteria)

- ✅ Investigator filing an inbox message to Archivist appears in `inbox/archivist/` with valid frontmatter
- ✅ Re-issuing the same request produces no duplicate (idempotent id check)
- ✅ Archivist completing the work moves the file to `outbox/investigator/<YYYY-MM>/` with `status: done` and writes a response with `correlation_id`
- ✅ Two agents updating `status/board.md` simultaneously don't corrupt the file (flock test)
- ✅ Web app `/status` renders the latest board within 30 seconds of an update
- ✅ Daily huddle file is generated nightly even if no agents ran (empty sections + a "no activity" note)
- ✅ A message past its `expires` is moved to `outbox/_expired/` on the next run start
- ✅ Operator queue items aged >7 days surface on the daily huddle as a nudge

## Open questions (for the operator)

1. Should operator-queue notifications hit Discord/Telegram, or stay pull-only (operator visits `/status` or daily huddle)? Default per parent README: pull-only in v0.1, push-via-briefings only.
2. Should the public `/status` page redact in-flight investigation names while they're active? Default proposal: show by default — transparency wins. Operator can flag specific investigations as `confidential: true` in YAML to suppress until publication.
3. How many days back of outbox activity should `/status` show? Default proposal: 7 days, configurable.
