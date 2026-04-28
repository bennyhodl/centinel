# Entity Extraction Rules

Detailed rules the Investigator follows when promoting a name encountered during a crawl into a wiki entity page. Four types: **contractor**, **person**, **org**, **project**. Permissive for the first three; **guarded** for `person`.

---

## Common rules

### Slug & title
- **Title:** the official legal name as it appears on the source. Preserve `LLC`, `Inc.`, `Co.`, accents, etc.
- **Slug:** lowercase, ASCII, hyphen-separated, legal suffixes stripped. e.g. `ACME Construction LLC` → `acme-construction`.
- If two distinct entities slugify to the same string, append a disambiguator: `acme-construction-fl`, `acme-construction-tx`.

### Required frontmatter (all types)

```yaml
---
title: <Official Name>
type: contractor | person | org | project
slug: <slug>
created: YYYY-MM-DD
updated: YYYY-MM-DD
sources:                        # URLs and vault paths that mention this entity
  - https://www.tampa.gov/...
  - Vault/pdfs/...pdf            # pending until Archivist responds
investigations:                  # slugs that touched this entity
  - parks-contractors
aliases: []                      # other names this entity has been seen under
confidence: high                 # high | medium | low — how confident the entity is real and distinct
---
```

### Body skeleton

```markdown
# <Official Name>

## Overview
[2–3 factual sentences. No narrative. No "appears to" / "seems to". Cite each claim.]

## Mentions
- YYYY-MM-DD — [short factual claim]. Source: [link](url) · Vault: `path or pending`

## Related entities
- [[other-slug]] — relationship (e.g. "principal", "subsidiary", "awarded by")

## Open questions
[append-only; never delete]
```

### Citation rule
Every line under `## Mentions` ends in `Source: ...` or it doesn't exist. Same for any factual sentence in `## Overview`. **No citation, no claim.** This is the editorial firewall.

### Update vs. create
- If a page with the same slug exists → **update**. Append new mentions; do not rewrite history.
- If a slug-collision but distinct entity → disambiguate (see above).
- If near-duplicate suspected (Levenshtein < 3 OR shared address/EIN OR aliases overlap) → **flag for Data Reporter merge review** at `<wiki>/_runtime/operator-queue/entity-merges/`. **Never auto-merge.**

### Atomic writes
Always write `<page>.md.tmp` then `mv` to `<page>.md`. Crashed mid-write leaves no corruption.

---

## `contractor`

**When to create:** any named legal entity that appears as a vendor, awardee, RFP responder, or contracted party on a `.gov` source.

**Required extras in frontmatter:**
```yaml
legal_form: LLC | Inc | Corp | Sole Prop | Unknown
ein: <if found>
addresses: []
principals: []                  # slugs of person entities (only those that pass the person threshold)
```

**Body adds:**
```markdown
## City contracts
| Date | Department | Project | Amount | Source |
|---|---|---|---|---|
| 2024-03-15 | Parks | Riverwalk maintenance | $1,200,000 | [link](url) |
```

Threshold: **first sighting**. Permissive.

---

## `org`

**When to create:** NGOs, advisory boards, commissions, neighborhood associations, foundations, PACs visible on `.gov` pages.

**Required extras:**
```yaml
org_kind: nonprofit | board | commission | advisory | pac | foundation | other
parent_org: <slug if subsidiary>
```

Threshold: **first sighting**. Permissive.

---

## `project`

**When to create:** any named project, RFP, capital improvement, grant program, named initiative.

**Required extras:**
```yaml
project_kind: rfp | capital | grant | initiative | program
status: open | closed | awarded | cancelled | unknown
budget: <amount or null>
department: <if found>
```

Threshold: **first sighting**. Permissive. Each named project gets its own page even if small.

---

## `person` (GUARDED)

**Create only if** at least one of:
1. **Leader / official by title** — mayor, councilor, commissioner, director, board chair, registered lobbyist, principal/officer of a contractor entity, named appointee.
2. **Mentioned in 3+ independent source pages** discovered across this or prior investigations.
3. **Explicitly listed in the investigation's `focus_entities`.**

Otherwise, mention them on the relevant org/contractor page **without** creating a person page. Privacy + signal-to-noise.

**Required extras:**
```yaml
roles:                          # current and prior public roles
  - title: Council Member
    org: city-of-tampa
    from: 2022-01-01
    to: null
relationships: []               # slugs of related people/orgs (e.g. spouse, business partner) — only if disclosed in a public source
```

### Hard rules for `person` pages
- **No private-life details.** Roles, public statements, votes, public filings only.
- **No claims about relatives** unless the relationship is in a public source (campaign disclosure, sunbiz officer listing, official bio).
- Near-duplicate detection is even more cautious. "J. Smith" vs "John A. Smith" → always flag for merge review, **never** create a second page silently and **never** merge silently.

---

## When to flag for Data Reporter merge review

Drop a file in `<wiki>/_runtime/operator-queue/entity-merges/<YYYY-MM-DD>-<slug-a>-vs-<slug-b>.md`:

```yaml
---
id: <hash>
type: entity-merge
from: investigator
created: <ISO>
priority: normal
status: open
references:
  entities: [<slug-a>, <slug-b>]
  confidence: 0.0–1.0
---

## Decision needed
Two entities may be the same:
- [[<slug-a>]] — first seen <date>, source <url>
- [[<slug-b>]] — first seen <date>, source <url>

Shared signals: <address / EIN / aliases / principals overlap>.
Differing signals: <whatever differs>.

Recommend: confirm | reject | defer.
```

The Data Reporter owns the merge decision; you're flagging candidates.
