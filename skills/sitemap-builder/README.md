# sitemap-builder

Centinel Cartographer skill — builds and maintains a labeled sitemap of a city's `.gov` web surface.

This is a Hermes skill, loaded into the **default Hermes profile** alongside the Editor persona (see `docs/AGENT_ROSTER.md` in the parent repo). Same agent, two hats: Editor for chat / synthesis, Cartographer for sitemap upkeep.

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

- Use **Tavily Crawl** for bulk domain/subtree crawling (the v0.1 locked decision — see `docs/SCRAPER_AND_EXTRACTORS.md`). The skill ships a thin wrapper at `scripts/crawl.py`; operator sets `TAVILY_API_KEY` in `.env`.
- Use Hermes' built-in web tools (`web_extract`, `browser_navigate`) for per-URL detail and JS rendering. No custom Playwright wrapper.
- Iterate against tampa.gov first. Patterns and exclude lists will sharpen after the first real bootstrap.
- See `docs/SCRAPER_AND_EXTRACTORS.md` and `docs/RUNTIME_PROTOCOL.md` in the parent repo for the surrounding contracts.
