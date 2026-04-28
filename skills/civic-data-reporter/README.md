# civic-data-reporter

Hermes skill for Tampa-DOGE's Data Reporter agent. Sole writer to `<wiki>/_data/tampa.db`.

See [SKILL.md](./SKILL.md) for the operational playbook.

## Layout

```
civic-data-reporter/
├── SKILL.md
├── README.md
├── references/
│   ├── db-schema.md            # canonical schema + rationale
│   ├── name-normalization.md   # canonicalization rules + merge threshold
│   └── public-views.md         # what /db filters and why
├── scripts/
│   ├── init_db.py              # idempotent schema creator + migration runner
│   ├── normalize_name.py       # name canonicalizer (--selftest)
│   ├── run_query.py            # readonly SELECT + methodology row write
│   ├── backup_db.sh            # atomic .backup + gzip + integrity check
│   └── init_public_views.sh    # load public-views.sql into tampa.db
└── templates/
    ├── operator-query.md       # Editor → Data Reporter request envelope
    └── methodology-entry.md    # markdown rendering of a methodology row
```

## Invariants

- **Sole writer.** Every other agent reads via Datasette or asks Data Reporter to run a query.
- **Never auto-merge.** All merge candidates land in `<wiki>/_runtime/operator-queue/entity-merges/`.
- **Methodology is public.** Every published claim cites an `M-<id>` from the methodology table.
- **Confidence floor for `/db`.** Public views filter `confidence < 0.7`.
- **Stdlib only.** No SQLAlchemy, no ORM. `sqlite3` is plenty.

## Related skills

- `civic-archivist` — produces vault sidecars + sends discrepancy flags here.
- `civic-investigator` — sends entity-merge candidates and entity/transaction upserts; reads from Datasette; writes wiki entity pages.
- `civic-watch-runner` — sends criteria queries.

## License

MIT — see repo root.
