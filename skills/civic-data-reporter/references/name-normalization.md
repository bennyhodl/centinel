# Name normalization

Canonicalization rules for `entities.canonical_name`. The original input is always preserved verbatim in `aliases.alias`. The script `scripts/normalize_name.py` is the source of truth — these rules and that script must agree.

## Goals

- Stable canonical form so `INSERT ... ON CONFLICT(slug)` actually catches duplicates.
- Loseless: anything we strip goes into an alias.
- Predictable: same input → same output, every time, regardless of caller.

## Rules (persons)

1. **Lowercase for processing**, then re-titlecase for output (`John A Smith`, not `JOHN SMITH`).
2. **Strip surrounding whitespace and quotes.**
3. **Drop honorifics** (case-insensitive, with or without trailing dot): `Mr Mrs Ms Mx Dr Hon Rev Prof Sir Dame The Honorable`.
4. **Reorder `Last, First [M.]` → `First M Last`.** Detect by exactly one comma and a space after it; preserve the `aliases` entry with the original.
5. **Suffixes** (`Jr Sr II III IV V Esq PhD MD`): strip from canonical, append at end as a normalized suffix without punctuation: `John Smith Jr` (no comma).
6. **Middle initial**: collapse `John A. Smith` → `John A Smith` (drop dot, single letter).
7. **Particles** (`van de la von del der bin ibn da das do dos`): keep, lowercase, no special-casing of position. `Maria de la Cruz` stays as-is.
8. **Hyphens and apostrophes** in surnames: preserve. `O'Brien`, `Jones-Smith`, `D'Angelo` are unchanged.
9. **Diacritics**: preserve in canonical; also store an ASCII-folded alias for matching (`José` → canonical `José`, alias `Jose`).
10. **Whitespace collapse**: any run of internal whitespace → single space.

## Rules (orgs / contractors)

1. **Strip corporate suffixes** for matching: `LLC L.L.C. Inc Inc. Incorporated Corp Corp. Corporation Co Co. Company Ltd Limited LP LLP PLLC PA P.A. NA N.A.`. Keep the most-recent observed form as canonical (so `ACME Construction LLC` stays `Acme Construction LLC` if that was the original spelling), but the *match key* uses the stripped form.
2. **Punctuation**: drop commas, dots in suffixes; keep `&` (literal), keep hyphens.
3. **Casing**: title-case unless the source uses an explicit acronym (`IBM`, `NAACP`) — detect by all-caps tokens of length 2–6 with no vowels-only pattern.
4. **`The` prefix**: drop for matching (`The Tampa Tribune` matches `Tampa Tribune`); preserve in canonical if observed.
5. **DBA / aka**: split on `dba|d/b/a|a/k/a`; the primary becomes canonical, the other becomes an alias.

## Match-candidate threshold

Two normalized names are flagged as a *merge candidate* (and dropped to operator queue) when **all** of:

- Token Jaccard similarity ≥ 0.6 (after stop-token removal of corporate suffixes).
- Levenshtein distance on the joined match-key ≤ 3, OR Levenshtein ratio ≥ 0.85.
- For persons: same first-token initial AND same last token (after suffix stripping).
- For orgs: not in the explicit `denylist_collisions` (e.g., `Acme Inc` vs `Acme LLC` are commonly two distinct sibling entities — flag, don't merge).

Below threshold → separate entities, no flag. Above threshold + any disambiguating mismatch (different EIN, different address) → flag with confidence ≤ 0.85. Above threshold + corroborating signal (same EIN, same address) → flag with confidence ≥ 0.9.

**Never auto-merge regardless of score.** All flags go to `<wiki>/_runtime/operator-queue/entity-merges/`.

## Examples

| Input | Canonical | Aliases written |
|---|---|---|
| `Smith, John A.` | `John A Smith` | `Smith, John A.` |
| `JOHN SMITH JR` | `John Smith Jr` | `JOHN SMITH JR` |
| `Hon. Maria de la Cruz` | `Maria de la Cruz` | `Hon. Maria de la Cruz`, `Maria de la Cruz` (ASCII fold if needed) |
| `ACME Construction, LLC` | `Acme Construction LLC` | `ACME Construction, LLC`; match key `acme construction` |
| `The Tampa Tribune` | `Tampa Tribune` | `The Tampa Tribune` |
| `O'Brien & Sons Co.` | `O'Brien & Sons Co` | `O'Brien & Sons Co.` |

## Edge cases logged but not auto-resolved

- Single-name ambiguities (`John Smith` with no middle, no role context): always create separate entities, never merge without operator review.
- Initials-only names (`J. A. Smith`): preserve as-is; only matches against another initials-only form, not against `John A Smith`.
- Cyrillic / non-Latin scripts: preserve canonical; ASCII-folded alias added; matching across scripts is a follow-up, not v0.1.
