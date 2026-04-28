# Finding-Draft Format

How the Investigator emits a candidate connection finding. **Drafts only. Operator promotes.** Every claim cites a source or it doesn't go in.

---

## Path

`<wiki>/Findings/draft/<investigation-slug>-<YYYY-MM-DD>.md`

If multiple findings come out of one run, suffix: `<slug>-<date>-01.md`, `<slug>-<date>-02.md`.

---

## Required frontmatter

```yaml
---
title: <one-line claim, neutral wording>
summary: <one-paragraph plain-language summary; no adjectives that imply guilt>
investigation: <slug>
generated_by: civic-investigator
generated_at: <ISO-8601>
status: draft                   # draft | review | published | archived | killed
drafted_at: <ISO-8601>
confidence: low | medium | high # the agent's self-rated confidence
sources:                        # MANDATORY — every URL/vault path used as evidence
  - https://www.tampa.gov/...
  - Vault/pdfs/...pdf
entities:                       # slugs of every entity the finding involves
  - acme-construction
  - jane-smith
tags: []
---
```

**Required fields:** `title`, `summary`, `investigation`, `generated_by`, `generated_at`, `status` (must be `draft`), `sources` (must be non-empty), `entities` (must be non-empty).

If you cannot satisfy all of these, **do not emit the draft.** Write the candidate into the investigation's `## Open Questions` section instead.

---

## Body structure

Three sections, in order. No deviation.

### 1. `## Claim`
One paragraph stating the connection in neutral, factual language. No "appears", no "suggests wrongdoing", no rhetorical flourish. State the pattern.

### 2. `## Evidence`
A numbered list. **Each item is one fact, with a citation.** Format:

```markdown
1. <Fact>. Source: [<short label>](<url>) · Vault: `<vault-path or pending>`
2. <Fact>. Source: [<short label>](<url>)
3. <Connection between facts 1 and 2>. Source: [<combined or supporting link>](<url>)
```

The connection itself is also cited — usually to the page where the two threads meet (a board roster, a campaign disclosure, a sunbiz filing). If the connection is *only* an inference, it does not belong here — it belongs in `## Open Questions` of the investigation, NOT in a draft finding.

**The rule: every claim cited or it doesn't go in.** If you find yourself wanting to write something without a citation, you've left the firewall and you are about to publish narrative without evidence. Stop.

### 3. `## Open questions`
What this finding does not yet establish. What a human reviewer or FOIA could clarify. What a follow-up crawl might surface.

### Optional 4. `## Counter-evidence`
If the crawl surfaced anything that complicates the finding, list it here with citations. Findings that omit counter-evidence are weaker, not stronger.

---

## Tone rules

- **Neutral verbs.** "Awarded", "voted", "registered", "listed". Not "rewarded", "rubber-stamped", "concealed".
- **No motive imputation.** State patterns; don't assign intent.
- **No named-subject contact attempts.** The Investigator never reaches out for comment. The operator decides whether to seek right-of-reply.
- **No publication.** `status: draft` only. The operator (Reviewer role) promotes to `Findings/published/` after review.

---

## Example (skeleton, not real)

```markdown
---
title: ACME Construction principal sits on board of NGO that received Parks funding
summary: ACME Construction's listed principal is also a board member of the Riverside Foundation, which received a $200K Parks-department grant in 2024 — six months after ACME won a $1.2M Parks contract. Both relationships are documented in public filings.
investigation: parks-contractors
generated_by: civic-investigator
generated_at: 2026-04-26T15:12:00-04:00
status: draft
drafted_at: 2026-04-26T15:12:00-04:00
confidence: medium
sources:
  - https://sunbiz.org/...acme...
  - https://www.tampa.gov/parks/grants/2024
  - https://riversidefoundation.org/board
  - Vault/pdfs/2024-03-15-acme-riverwalk-award.pdf
entities:
  - acme-construction
  - riverside-foundation
  - jane-smith
tags: [board-overlap, parks]
---

## Claim
ACME Construction's principal Jane Smith serves on the board of the Riverside Foundation. The Foundation received a $200K Parks-department grant in October 2024. ACME received a $1.2M Parks contract in March 2024.

## Evidence
1. Jane Smith is listed as principal of ACME Construction LLC. Source: [Sunbiz officer record](https://sunbiz.org/...) · Vault: `Vault/pdfs/sunbiz-acme-2024.pdf`
2. Jane Smith is listed as a board member of Riverside Foundation. Source: [Riverside Foundation board page](https://riversidefoundation.org/board)
3. Riverside Foundation received a $200K grant from the Parks department on 2024-10-15. Source: [Parks grants 2024](https://www.tampa.gov/parks/grants/2024) · Vault: `Vault/html/2024-parks-grants.html`
4. ACME Construction was awarded a $1.2M contract by the Parks department on 2024-03-15. Source: [Parks award notice](https://www.tampa.gov/...) · Vault: `Vault/pdfs/2024-03-15-acme-riverwalk-award.pdf`

## Open questions
- Was Smith on the Foundation board at the time of the ACME contract award?
- What was the Foundation grant for, and was Smith involved in its application?
- Are there other board members of Riverside Foundation with city contracts?

## Counter-evidence
- The Parks grant was awarded by a different program (community-grants) than the Parks contract (capital-improvements). Source: [Parks org chart](https://www.tampa.gov/parks/about).
```

---

## What NOT to put in a draft finding

- A claim with no citation. (→ goes in investigation's `## Open Questions`.)
- A finding with confidence "high" but only one source. ("High" requires ≥2 independent sources.)
- A motive ("ACME got favorable treatment because…"). Patterns only.
- A finding about a private individual who doesn't pass the `person` entity threshold.
- Anything you'd hesitate to show the named subject during a right-of-reply call.
