# civic-watch-runner

Centinel skill for the **Watch Runner** role in the Centinel civic-data system.

Continuous matchers over sitemap diffs and new wiki content. Hits classified into two lanes:

- **Raw** (`<wiki>/Findings/raw/`) — one concrete fact + citation. Auto-published.
- **Narrative** (`<wiki>/Findings/draft/`) — connection/pattern claims. Always human-reviewed.

Runs every 4h after `sitemap-builder` posts a lint diff. See `SKILL.md` for the full procedure.

## Layout

```
civic-watch-runner/
├── SKILL.md                                 # the skill
├── README.md                                # this file
├── references/
│   ├── preset-watches.md                    # errant-spending, corruption-signals, policy-drift
│   ├── match-dsl.md                         # match-criteria grammar
│   └── finding-classification.md            # raw vs draft rubric
├── templates/
│   ├── watch.yaml                           # annotated watch template
│   └── finding-raw.md                       # auto-published finding template
└── scripts/
    ├── list_watches.py                      # enumerate + validate watches
    ├── dedup_hits.py                        # filter against seen log
    └── watch_lock.sh                        # flock wrapper
```

## Related

- `sitemap-builder` — produces the sitemap diff this skill consumes.
- `civic-data-reporter` — owns `tampa.db`; data watches query through it (or directly read-only).
- `civic-investigator` — produces wiki pages that watches scan.

See `~/code/centinel/docs/AGENT_ROSTER.md` for how this role fits in the whole system.
