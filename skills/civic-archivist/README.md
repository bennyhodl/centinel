# civic-archivist

Centinel skill. The Archivist — document intake, vault, OCR, sidecar generation, manifest maintenance.

## Layout

```
civic-archivist/
├── SKILL.md                       # main playbook (load this)
├── README.md                      # you are here
├── references/
│   ├── document-kinds.md          # per-mime extraction playbook
│   └── vault-layout.md            # vault dirs, naming, manifest schema
├── templates/
│   └── sidecar.md                 # canonical sidecar template
└── scripts/
    ├── sha256_file.py             # stdlib SHA256
    ├── check_dupe.py              # manifest dedup lookup
    ├── append_manifest.py         # flock-protected append
    └── extract_text.sh            # type-dispatch text extractor
```

## Wiki paths

- Vault root: `<wiki>/Vault/`
- Manifest: `<wiki>/Vault/manifest.jsonl` (append-only, JSON Lines)
- Inbox: `<wiki>/_runtime/inbox/archivist/`
- Outbox: `<wiki>/_runtime/outbox/archivist/<YYYY-MM>/`
- Status: `<wiki>/_runtime/status/archivist.md`
- Discrepancy reports out to: `<wiki>/_runtime/inbox/data-reporter/`

## Related

- `civic-investigator` — calls the Archivist inline when it encounters URLs.
- `civic-watch-runner` — drops vault requests when watches fire.
- `civic-data-reporter` — receives discrepancy flags from the Archivist.
- See `docs/RUNTIME_PROTOCOL.md`, `docs/SCRAPER_AND_EXTRACTORS.md`, `docs/AGENT_ROSTER.md`, `docs/WEB_APP_DESIGN.md`.

## Origin

Converted from the single-file spec at `skills/civic-archivist.md` (deleted) on 2026-04-27.
