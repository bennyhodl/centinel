---
# REQUIRED FIELDS — do not omit
title: <one-line claim in neutral language>
summary: <one paragraph; no adjectives that imply guilt>
investigation: <slug-of-parent-investigation>
generated_by: civic-investigator
generated_at: <ISO-8601 timestamp>
status: draft                       # MUST be "draft" — agent never publishes
drafted_at: <ISO-8601 timestamp>
confidence: low                     # low | medium | high (high requires ≥2 independent sources)
sources:                            # MANDATORY — must be non-empty
  - https://www.tampa.gov/...
  - Vault/pdfs/...pdf
entities:                           # MANDATORY — slugs of every entity involved
  - <slug-a>
  - <slug-b>

# OPTIONAL
tags: []
---

# <Investigation title> — <claim>

## Claim
<One paragraph stating the connection in neutral, factual language.
No "appears", "suggests wrongdoing", "rubber-stamped". State the pattern.>

## Evidence
1. <Fact A>. Source: [<short label>](<url>) · Vault: `<vault-path or pending>`
2. <Fact B>. Source: [<short label>](<url>) · Vault: `<vault-path or pending>`
3. <The connection between A and B>. Source: [<link>](<url>)

<!--
RULE: every claim cites a source or it doesn't go in.
If you can't cite it, write it under ## Open questions, not here.
-->

## Open questions
- <What this finding does not yet establish>
- <What a follow-up crawl or FOIA might clarify>

## Counter-evidence
<!-- Optional but encouraged. If the crawl surfaced anything that complicates the
     claim, list it here with citations. Findings that omit counter-evidence are
     weaker, not stronger. -->
