---
id: <YYYY-MM-DD>-<HHMM>-<from>-q-<short-slug>
from: editor
to: data-reporter
type: request
priority: normal                   # low | normal | high | critical
created: <ISO8601>
expires: <ISO8601 + 3d>
correlation_id: null
status: pending
references:
  investigation: <slug-or-null>
  finding_draft: <slug-or-null>
response_required: true
# Query metadata picked up by run_query.py:
label: <kebab-case-stable-handle>   # cited later as M-<id>; must be unique-ish
---

## Rationale

One paragraph: what question is this query answering, what finding/briefing will use it, what's the intended caveat list.

## SQL

```sql
SELECT ...
FROM ...
WHERE ...
;
```

## Caveats / known limits (optional)

- e.g., excludes Q4 2025 ingestion still in archivist queue
- e.g., relies on `parks-department` alias map current as of 2026-04-25
