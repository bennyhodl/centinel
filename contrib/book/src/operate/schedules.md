# Schedules

A schedule is a **saved `run` and a cadence** — nothing more. It has no arguments of its
own that `run` does not have. If you can type it, you can schedule it.

```bash
centinel schedule set tampa-daily --cron "0 3 * * *" --source tampa
centinel schedules
centinel history
centinel serve
```

`centinel serve` is what fires them. `--no-schedule` serves the read API without firing
anything, which is what a machine that serves a corpus somebody else collects wants.

## Writing one

```bash
centinel schedule set <id> [flags]
centinel schedule rm  <id>
```

| Flag | Meaning |
|---|---|
| `--cron EXPR` | 5-field cron expression, or a shorthand like `@daily` |
| `--tz ZONE` | IANA zone name. Defaults to the host's. |
| `--source ID` | source to run. Repeatable. Omit for every enabled source. |
| `--skip STAGE` | stage to skip. Repeatable. |
| `--limit N` | stop collection after this many addresses, per source |
| `--refresh` | re-fetch and re-derive everything at every fire. Expensive, and deliberate. |
| `--jitter-secs N` | seconds of jitter. Zero fires exactly on the minute. |
| `--disabled` | write the block but leave it disarmed |
| `--no-catch-up` | do not fire on startup when overdue |
| `--replace` | replace an existing schedule with this id |

On a terminal with no arguments, the CLI asks. Everywhere else the id and cadence are
required — the op itself never prompts, because an op that blocks on input cannot be
called over HTTP.

Whichever way you write one, `schedule set` prints **the next few fire times in the
schedule's own zone**. Three dates settle whether `0 3 * * 1` meant Mondays or the 1st,
and that is the last cheap moment to notice it did not.

## Why cron, and why a zone

Cron, because a civic record has a shape a fixed interval cannot express: *before the
Tuesday meeting*, *overnight*, *not during business hours*.

The zone is part of the schedule rather than the host, because a corpus outlives the
machine that collects it, and "3am" means a different instant after a daylight-saving
change. Storing the zone keeps the intent rather than the offset.

Jitter exists because forks are the point. If a hundred cities each run the same default
cadence, they all hit their upstreams on the same minute.

## Schedules live in the config

`centinel.toml`, as `[[schedule]]` blocks, beside the `[[source]]` blocks they name.
Not in the store, because the store is *fact* — what was collected — and a cadence is
*intent*. The same distinction that makes `source list` report a union.

Creation is a command rather than hand-edited TOML because a hand-written block invites
two silent mistakes: a cron expression that parses and means something else, and a source
id that does not exist. `centinel schedules --check` runs exactly the validation `serve`
performs before binding, so you can find out after editing rather than at the next
restart.

An invalid schedule **refuses to start the server**. A scheduler that silently drops the
one broken entry is a scheduler you cannot trust the other twenty of.

## One lane

Runs do not overlap. One at a time, per store, with the lock on disk — because the CLI is
a second process, and `centinel run` typed by hand while the server is mid-run is the
ordinary case, not an edge case.

The queue is depth one per schedule, FIFO, with no merging. A fire that arrives while
something is running waits; a second one that arrives while one is already waiting is
dropped, because two identical pending runs do the work of one.

Catch-up fires **once** on startup when a schedule is overdue, never a backlog. A machine
that was off for a week does not wake up owing seven runs.

## Reading what happened

```bash
centinel schedules                  # what is configured, when each next fires, how the last went
centinel history                    # every attempt, newest first
centinel history --failed
centinel history --schedule tampa-daily
centinel history --source tampa
centinel history --since 2026-08-01T00:00:00Z
centinel history --run 8f3c         # one run, by id or unambiguous prefix — the whole report
```

`history` covers **manual runs too**, not only scheduled ones. It is a record of attempts.

One record per **attempt**, not per success. A crash leaves evidence: the record is
written when the run starts, so a run that died is a row that never completed rather than
a gap you have to infer.

The run id is its start instant, which is why a prefix is enough to name one.

## Additions and subtractions

The history record carries the arithmetic of what each run changed — bytes that entered
the corpus, and what the discovery delta was. Nothing is ever removed: a Resource that
vanished from a sitemap is a *subtraction from the snapshot*, not a deletion from the
archive. The bytes stay, and the liveness changes.

The counts are in the record; the addresses are in the log. That split is deliberate — a
run record that listed every address would be a second copy of the log, and the second
copy is the one that goes stale.

Next: [Models](models.md).
