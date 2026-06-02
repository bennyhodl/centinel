---
title: Centinel — Skill Specs
status: 🧠 Specced (not yet built)
created: 2026-04-26
parent: ../README.md
---

# Skill Specs

Skill specs for the five Centinel roles. Each one ships as a `SKILL.md` in its own directory under `skills/`; centinel-server's role loader picks the right one for the active role at runtime via pi-agent's `DefaultResourceLoader` skills override. See [`docs/PI_MIGRATION_PLAN.md`](../docs/PI_MIGRATION_PLAN.md).

| Skill | Loaded into role | Spec |
|---|---|---|
| `sitemap-builder` | `editor` | [sitemap-builder/SKILL.md](./sitemap-builder/SKILL.md) |
| `civic-investigator` | `investigator` | [civic-investigator/SKILL.md](./civic-investigator/SKILL.md) |
| `civic-archivist` | `archivist` | [civic-archivist/SKILL.md](./civic-archivist/SKILL.md) |
| `civic-data-reporter` | `data-reporter` | [civic-data-reporter/SKILL.md](./civic-data-reporter/SKILL.md) |
| `civic-watch-runner` | `watch-runner` | [civic-watch-runner/SKILL.md](./civic-watch-runner/SKILL.md) |

## Conventions across all five specs

- **Wiki path:** `<wiki>` resolves to `~/wiki/<City>/` per operator. Centinel = `~/wiki/Tampa/`.
- **Vault path:** `<wiki>/Vault/` per Document Vault rules in parent README.
- **Vault rule:** every external resource (PDF, HTML, transcript, image) MUST be vaulted before it's used. No skill quotes from a URL it didn't vault first.
- **Source attribution:** every wiki claim cites a vault path, not a live URL.
- **No outbound comms:** no skill calls/emails/messages anyone. No FOIA filing. No right-of-reply contact. Those are human jobs.
- **Frontmatter on everything:** all wiki pages get YAML frontmatter per `llm-wiki` conventions plus Centinel-specific fields (sitemap_entry, vault_paths, investigation, watch_hits).
- **Cron-runnable:** every skill must work both interactively (operator triggers) and from cron (no human in the loop). Specs note the cron entry point explicitly.
- **Logging:** every run appends to `<wiki>/log.md` with date, skill, summary, files touched.
