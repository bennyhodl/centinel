# Portal vendor cheat-sheet

Common `.gov` portal vendors you'll encounter when cataloging a city's web surface. Use this during the description pass to populate `parser_suggestion` and to decide whether a page needs JS rendering.

If a URL matches one of these fingerprints, set `parser_suggestion` to the suggested name (even though the parser may not exist yet — it's a hint for `civic-investigator` / `civic-archivist`). Most of these vendors serve SPAs and need `browser_navigate`, not `web_extract`.

| Vendor | Telltale URL patterns | Hosts | JS? | `parser_suggestion` |
|---|---|---|---|---|
| **Granicus** | `*.granicus.com`, `*/MediaPlayer.php`, `*/ViewPublisher.php`, `legistar.granicus.com` | Council meeting video, agendas, minutes, archived stream | Yes | `granicus-meeting` |
| **Legistar** | `*.legistar.com`, `legistar.granicus.com`, `*/Calendar.aspx`, `*/MeetingDetail.aspx` | Council agendas, ordinances, voting records, file detail pages | Yes (heavy) | `legistar-agenda` |
| **CivicPlus** | `*.civicplus.com`, `civicengage.com`, `civicclerk.com`, `civicrec.com` | Generic CMS for cities, agenda center, parks/rec, bid postings | Mixed | `civicplus-page` |
| **OpenGov** | `*.opengov.com`, `checkbook.opengov.com`, `*/transparency`, `*/budget` | Budget dashboards, checkbook, financial transparency | Yes (SPA, dashboards) | `opengov-budget` |
| **Bonfire** | `*.bonfirehub.com`, `bonfire.<city>.gov` | RFP / RFQ / sealed-bid postings | Yes | `bonfire-rfp` |
| **eTRAKiT** | `etrakit.*`, `*/etrakit3/`, `etrakit.flagstaffaz.gov` style | Permits, code enforcement, contractor licenses | Mostly server-rendered | `etrakit-permit` |
| **Accela** | `aca.*`, `*/CitizenAccess/`, `*/Accela/` | Permits, planning, code enforcement, business licenses | Server-rendered (ASP.NET) | `accela-permit` |
| **NovusAGENDA** | `*.novusagenda.com`, `agenda.novusagenda.com` | Council agendas, meeting packets | Mixed | `novus-agenda` |
| **PrimeGov** | `*.primegov.com`, `*/Public/` | Newer agenda/meeting management | Yes | `primegov-meeting` |
| **BoardDocs** | `*.boarddocs.com`, `go.boarddocs.com` | School-board / agency board agendas | Yes | `boarddocs-agenda` |
| **Tyler Technologies** | `*.tylertech.com`, `*/MunisSelfService/`, `munis.*` | Finance, ERP, citizen self-service | Mixed | `tyler-munis` |
| **Socrata** | `*.socrata.com`, `data.<city>.gov` (Tyler Data & Insights) | Open-data catalog, dataset pages | API + HTML | `socrata-dataset` |
| **CKAN** | `data.*` paths with `/dataset/`, `/group/` | Open-data catalog | Server-rendered | `ckan-dataset` |

## What "JS?" means in practice

- **Yes** → `web_extract` will return a near-empty body / a "please enable JavaScript" message. Retry with `browser_navigate`.
- **Mixed** → try `web_extract` first; if body length < ~500 chars or contains `enable JavaScript`, fall back to `browser_navigate`.
- **Server-rendered** → `web_extract` is fine.

## Notes for Tampa specifically (to fill in after bootstrap)

This section is intentionally empty in v0.1. After the first real `bootstrap` against tampa.gov, document which vendors Tampa uses for which subsystems (e.g. "Tampa uses Granicus for council video, Bonfire for procurement, Accela for permits") so future runs can short-circuit.
