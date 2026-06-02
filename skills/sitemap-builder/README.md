# sitemap-builder

Centinel Cartographer skill — builds and maintains a labeled sitemap of a city's `.gov` web surface.

This is a Centinel skill, loaded into the **editor role inside centinel-server** alongside the Editor persona (see `docs/AGENT_ROSTER.md` in the parent repo). Same agent, two hats: Editor for chat / synthesis, Cartographer for sitemap upkeep.

## Start here

→ [`SKILL.md`](./SKILL.md) — full procedure, four modes (`bootstrap`, `lint`, `subtree`, `register`), schema, prompts, pitfalls.

## Layout

```
sitemap-builder/
├── SKILL.md                       # the skill body — procedure for the LLM
├── README.md                      # this file
├── references/
│   ├── portal-vendors.md          # Granicus / Legistar / CivicPlus / etc. URL fingerprints
│   └── exclude-patterns.md        # default exclude regexes with reasoning
├── templates/
│   ├── sitemap-entry.yaml         # fully-populated entry, schema reference
│   └── sitemap-index.md           # example structure for <wiki>/Sitemap/index.md
└── scripts/
    ├── normalize_url.py           # URL canonicalizer (stdlib only)
    └── check_robots.py            # robots.txt allow/deny check (stdlib only)
```

## v0.1 scope

- Lean on the role's web_fetch tool (TODO: stub) (`web_extract`, `browser_navigate`, `web_search`). No custom Playwright wrapper.
- Iterate against tampa.gov first. Patterns and exclude lists will sharpen after the first real bootstrap.
- See `docs/SCRAPER_AND_EXTRACTORS.md` and `docs/RUNTIME_PROTOCOL.md` in the parent repo for the surrounding contracts.
