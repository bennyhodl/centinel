---
title: Centinel — Skill Specs
status: 🧠 Specced (not yet built)
created: 2026-04-26
parent: ../README.md
---

# Skill Specs

Design specs for the five new skills required by Centinel. **These are NOT in `~/.hermes/skills/`** — they live here in the plan directory as authored specs. When the project graduates from 🧠 Thinking → 🔬 Investigating → 🛠️ Building, each spec gets translated into a real Hermes skill via `skill_manage(action='create')`.

| Skill | Owner agent | Spec |
|---|---|---|
| `sitemap-builder` | Cartographer | [sitemap-builder.md](./sitemap-builder.md) |
| `civic-investigator` | Investigator | [civic-investigator.md](./civic-investigator.md) |
| `civic-archivist` | Archivist | [civic-archivist.md](./civic-archivist.md) |
| `civic-data-reporter` | Data Reporter | [civic-data-reporter.md](./civic-data-reporter.md) |
| `civic-watch-runner` | Watch Runner | [civic-watch-runner.md](./civic-watch-runner.md) |

Reused (not specced here, already exist):
- `humanized-writing` — Briefings Writer
- `llm-wiki` — Librarian (lint mode)

## Conventions across all five specs

- **Wiki path:** `<wiki>` resolves to `~/wiki/<City>/` per operator. Centinel = `~/wiki/Tampa/`.
- **Vault path:** `<wiki>/Vault/` per Document Vault rules in parent README.
- **Vault rule:** every external resource (PDF, HTML, transcript, image) MUST be vaulted before it's used. No skill quotes from a URL it didn't vault first.
- **Source attribution:** every wiki claim cites a vault path, not a live URL.
- **No outbound comms:** no skill calls/emails/messages anyone. No FOIA filing. No right-of-reply contact. Those are human jobs.
- **Frontmatter on everything:** all wiki pages get YAML frontmatter per `llm-wiki` conventions plus Centinel-specific fields (sitemap_entry, vault_paths, investigation, watch_hits).
- **Cron-runnable:** every skill must work both interactively (operator triggers) and from cron (no human in the loop). Specs note the cron entry point explicitly.
- **Logging:** every run appends to `<wiki>/log.md` with date, skill, summary, files touched.
