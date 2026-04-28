# Default exclude patterns

These are the v0.1 default `exclude_patterns` for civic `.gov` crawls. Each is a regex (Python flavor) tested against the full URL. If a URL matches any pattern, the Cartographer **does not crawl it** and **does not register it** in the sitemap (or, if previously registered, marks it `status: excluded`).

Operator can override per-run via `config.exclude_patterns`. After the first real `bootstrap`, expand this list with city-specific patterns.

## The list

```yaml
exclude_patterns:
  # --- Binary / vault-bound ---
  - "\\.pdf(\\?|$)"          # PDFs go to the vault via Archivist, not the sitemap
  - "\\.docx?(\\?|$)"        # Word docs → vault
  - "\\.xlsx?(\\?|$)"        # Excel → vault (data-reporter may ingest separately)
  - "\\.zip(\\?|$)"          # archives → vault on demand
  - "\\.(jpg|jpeg|png|gif|webp|svg)(\\?|$)"  # images → not catalogued individually

  # --- Calendar / search infinite loops ---
  - "[?&]date=\\d{4}"        # ?date=2099-12-31 style — unbounded
  - "/calendar/print"
  - "/calendar/(month|week|day)\\?"
  - "/search\\?"             # search result pages — infinite query space
  - "/search$"
  - "[?&]q="                 # generic search-query parameter

  # --- Login / auth / session-bound ---
  - "/login"
  - "/signin"
  - "/logout"
  - "/auth/"
  - "/account/"
  - "[?&](jsessionid|phpsessid|csrf|csrftoken|sessionid)="

  # --- Pagination noise (keep first page only at v0.1) ---
  - "[?&]page=([2-9]|[1-9]\\d+)"   # page=2 onwards; revisit if real listings need deep paging

  # --- Print / share / utility views ---
  - "/print$"
  - "[?&]print=1"
  - "/share$"
  - "/email\\?"

  # --- Tracking / analytics ---
  - "[?&]utm_"
  - "[?&]fbclid="
  - "[?&]gclid="
```

## Reasoning, line-by-line

- **PDFs and other binaries.** The Vault (`civic-archivist`'s domain) holds documents; the sitemap holds *pages that link to documents*. Letting PDFs into the sitemap inflates it 10–100x and duplicates Archivist's job.
- **Calendar query strings.** A `?date=2099-12-31` URL is valid forever and the calendar page renders identically; without an exclude, the crawler walks until `max_pages`.
- **Search forms.** Same problem — every `?q=...` permutation is a "new" URL. Skip search forms entirely; the sitemap describes the *form*, not its results.
- **Session tokens.** `jsessionid` etc. embed in URLs on some ASP.NET / J2EE portals. The same page registers as N entries unless tokens are stripped (`scripts/normalize_url.py` does this) and excluded.
- **Pagination beyond page 1.** v0.1 trade-off: a paginated listing's *first* page is what the sitemap describes ("lists awarded contracts, paginated"). Investigators walk pagination depth-first when they need it. Revisit once we see real listings.
- **Print / share views.** Duplicate content of a canonical page; no informational value.
- **UTM / tracking.** Should never be canonical anyway — `normalize_url.py` strips them, but pattern-excluding is a belt-and-braces guard.

## When to override

The operator (or you, as Editor, with operator approval) should consider relaxing or tightening:

- **Tighten:** add `/admin/`, `/cms/`, vendor-specific noise paths once they surface.
- **Relax:** if a city posts agendas only as PDFs with no HTML index, you may need to register the PDFs themselves *or* (better) ask the Archivist to maintain a "PDF-only sources" index that the Cartographer references. Decide before relaxing the `\\.pdf` rule globally.
