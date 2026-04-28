---
title: <Investigation title — operator fills in>
goal: |
  <One paragraph. What question does this investigation answer? What would
  a successful end-state look like? Be specific — vague goals produce vague crawls.>
seeds:
  - https://www.tampa.gov/<seed-url-1>
  - https://www.tampa.gov/<seed-url-2>
status: active                  # active | paused | done | archived
depth: 2                        # max hops from seeds; 1–5
schedule: weekly                # daily | weekly | monthly | manual
date_range:
  from: 2021-01-01
  to: null                      # null = present
focus_entities: []              # bias extraction toward these slugs
exclude_urls:
  - /calendar/
  - /events/
created: <YYYY-MM-DD>
updated: <YYYY-MM-DD>
auto_complete: false            # if true, agent may flip status to done when goal-shape met
confidential: false             # if true, suppress in public /status renders
---

# <Investigation title>

## Goal
<Restate the goal in one paragraph. The Investigator re-anchors on this every synthesis pass. If you change the goal, the agent will follow.>

## Seeds
- <url1> — why this seed: <one sentence>
- <url2> — why this seed: <one sentence>

## Methodology
<Operator's hand-written notes on how this investigation should be approached.
Hypotheses to test. What "done" looks like. Sources that should be prioritized.
The agent reads this section but never edits it.>

## Notes
<Operator's running notes. Free-form. The agent never touches this section.>

## Findings (auto-appended)
<!-- agent appends one bullet per draft finding emitted, with link -->

## Open Questions
<!-- both operator and agent append; never delete -->

## Run log
<!-- agent owns; append-only; one ### Run YYYY-MM-DD HH:MM block per run -->
