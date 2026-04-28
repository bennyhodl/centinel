# Vault Layout & Manifest Schema

The vault is the evidence base. Every claim in Centinel — every wiki entity page, every DB row, every published finding, every Editor chat answer — cites a vault path. Operator-in-Chief and external readers verify by clicking through to the original artifact at `/vault/[...path]` per `docs/WEB_APP_DESIGN.md`.

**Three invariants the Archivist must never break:**
1. **Append-only.** Files land; files never leave. Manifest grows forever.
2. **Immutable originals.** A vaulted file's bytes never change. New version of the same URL = new vault entry, never overwrite.
3. **Hash-addressed dedup.** Same SHA256 = same vault entry, even if discovered via three different URLs.

---

## Directory tree

```
<wiki>/Vault/
├── pdfs/
│   ├── <YYYY-MM-DD>-<sha8>-<slug>.pdf            # original, 0444, +i
│   └── sidecar/
│       └── <YYYY-MM-DD>-<sha8>-<slug>.pdf.md
├── html/
│   ├── <YYYY-MM-DD>-<sha8>-<slug>.html
│   └── sidecar/<YYYY-MM-DD>-<sha8>-<slug>.html.md
├── data/                                          # csv, xlsx, xls, tsv, json
│   └── sidecar/...
├── transcripts/                                   # audio/video originals + .txt transcripts
│   ├── <YYYY-MM-DD>-<sha8>-<slug>.mp3
│   ├── <YYYY-MM-DD>-<sha8>-<slug>.txt             # whisper output, ALSO 0444
│   └── sidecar/<YYYY-MM-DD>-<sha8>-<slug>.mp3.md
├── images/
│   └── sidecar/...
├── _inbox/                                        # operator manual drops, processed -> moved to vault
│   └── _processed/<YYYY-MM-DD>/                   # 90-day retention before pruning
├── _tmp/                                          # workspace; never linked from manifest
└── manifest.jsonl                                 # append-only, the index of record
```

## Naming convention

`<YYYY-MM-DD>-<sha8>-<slug>.<ext>` where:
- `<YYYY-MM-DD>` — the date the Archivist first vaulted this content (its `fetched_at`, in operator-local time).
- `<sha8>` — first 8 hex chars of the SHA256 of the raw bytes.
- `<slug>` — kebab-case slug, ≤64 chars. For URLs: derived from the last meaningful path segment. For HTML: from `<title>`. For operator drops: from the original filename.
- `<ext>` — lowercased actual extension matching the magic-byte mime, NOT necessarily the URL extension.

Sidecars live in a parallel `sidecar/` subdir and are named `<filename>.md` (so the original `.pdf` and its sidecar `.pdf.md` are easily zipped or paired).

## Immutability

After moving from `_tmp/` to vault path:
1. `chmod 0444 <vault_path>` — required, always.
2. `chattr +i <vault_path>` — best-effort. Logs warning on failure (non-ext FS, non-root container). Do not fail the vault op.

The web app serves these as static files with strong ETags (`etag = sha256`). Browser cache is fine because bytes never change.

## Manifest entry schema

`<wiki>/Vault/manifest.jsonl` — JSON Lines, one entry per line, append-only. Two `op` types:

### `op: "vault"` — initial entry

```json
{
  "op": "vault",
  "vault_path": "pdfs/2026-04-26-a1b2c3d4-fy2025-parks-awards.pdf",
  "sidecar_path": "pdfs/sidecar/2026-04-26-a1b2c3d4-fy2025-parks-awards.pdf.md",
  "sha256": "a1b2c3d4e5f6...64chars",
  "size_bytes": 482113,
  "mime_type": "application/pdf",
  "document_kind": "pdf",
  "fetched_at": "2026-04-26T14:32:11-04:00",
  "source_url": "https://www.tampa.gov/sites/default/files/parks-awards-fy2025.pdf",
  "seen_at": [
    {
      "url": "https://www.tampa.gov/sites/default/files/parks-awards-fy2025.pdf",
      "at": "2026-04-26T14:32:11-04:00",
      "discovered_via": {
        "investigation": "parks-contractors",
        "caller_url": "https://www.tampa.gov/procurement/awards",
        "link_text": "FY2025 Parks Capital Awards (PDF)",
        "from_agent": "investigator"
      }
    }
  ],
  "extractor": "web_extract",
  "ocr_engine": null,
  "page_count": 24,
  "extraction_status": "ok",
  "tags": []
}
```

### `op: "seen_at_append"` — additional discovery context for an already-vaulted sha

```json
{
  "op": "seen_at_append",
  "target_sha256": "a1b2c3d4...",
  "at": "2026-04-28T09:11:00-04:00",
  "url": "https://www.tampa.gov/procurement/uploads/parks-awards-fy2025.pdf",
  "discovered_via": {
    "investigation": "parks-contractors",
    "caller_url": "https://www.tampa.gov/council/agenda-2026-04-28",
    "link_text": "Parks Award Schedule",
    "from_agent": "watch-runner"
  }
}
```

Readers (data-reporter, web app, Editor) **fold seen_at_append lines forward** when they materialize a view of the manifest — typically by `LEFT JOIN`ing them onto their `target_sha256`'s vault entry.

## Reader's algorithm (for downstream agents)

```python
def load_manifest(path):
    by_sha = {}
    for line in open(path):
        e = json.loads(line)
        if e["op"] == "vault":
            by_sha[e["sha256"]] = e
        elif e["op"] == "seen_at_append":
            tgt = by_sha.get(e["target_sha256"])
            if tgt:
                tgt["seen_at"].append({
                    "url": e["url"],
                    "at": e["at"],
                    "discovered_via": e["discovered_via"],
                })
    return by_sha
```

This is intentionally trivial — the manifest is small enough (thousands → tens of thousands of entries over the project's life) that an in-memory fold is fine.

## /vault/[...path] route

Per `docs/WEB_APP_DESIGN.md`, the Next.js app exposes every vault file at `https://<host>/vault/<vault_path>`. The Archivist's job is to make sure that:
- `vault_path` in the manifest matches the relative path under `<wiki>/Vault/`.
- Files are world-readable (0444) so the Node process can serve them without escalation.
- Mime type is correct (web app reads from manifest's `mime_type`, falls back to `file --mime-type`).

## Out of scope for v0.1 (tracked as follow-ups)

- **Manifest health cron** (orphan vault files, orphan manifest entries, sha mismatch alerts) — single-file spec listed this; deferring to v0.2 once we have actual vault traffic.
- **Source summary pages** at `<wiki>/Sources/<slug>.md` — the Editor / data-reporter generates these from sidecar + manifest; Archivist only writes the sidecar.
- **Recursive vaulting of embedded PDF attachments** — flag in sidecar, don't recurse.
- **Diarization for multi-speaker transcripts** — accept single-track Whisper output for now.
