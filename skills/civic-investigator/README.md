# civic-investigator (Hermes skill)

The **Investigator** profile's only skill. Runs operator-defined civic investigations end-to-end against public `.gov` sources, emits cited evidence into the wiki, and proposes candidate connection findings for human review.

- **Profile:** `~/.hermes/profiles/investigator/`
- **Spotlight role:** Lead Reporter
- **Authority:** drafts only — never publishes, never contacts named subjects.

## Layout

```
civic-investigator/
├── SKILL.md                          # the operating manual the agent loads
├── README.md                         # this file
├── references/
│   ├── entity-extraction-rules.md   # contractor / org / project / person thresholds
│   └── finding-draft-format.md      # citation rule and draft schema
├── templates/
│   ├── investigation.md              # operator's starter for a new investigation
│   └── finding-draft.md              # agent's starter for an emitted draft finding
└── scripts/
    ├── parse_investigation_yaml.py   # validate + emit JSON of investigation frontmatter
    └── extract_pdf_links.py          # enumerate PDF/doc links from a markdown file
```

## Read these first

- `SKILL.md` — full procedural instructions for the LLM agent.
- `../../docs/RUNTIME_PROTOCOL.md` — inbox/outbox conventions across all agents.
- `../../docs/SCRAPER_AND_EXTRACTORS.md` — **use Hermes' built-in `web_extract` / `browser` tools first**; no custom Playwright wrapper in v0.1.
- `../../docs/AGENT_ROSTER.md` — where this profile sits in the org chart.
- `../../docs/EDITOR_PERSONA.md` — what the Editor delegates to the Investigator.

## Companion skills

- [`sitemap-builder`](../sitemap-builder.md) — Investigator drops `register` requests when it discovers off-sitemap URLs.
- [`civic-archivist`](../civic-archivist.md) — Investigator drops vault requests for every PDF/HTML capture.
- [`civic-data-reporter`](../civic-data-reporter.md) — Investigator flags near-duplicate entities for merge review here.
