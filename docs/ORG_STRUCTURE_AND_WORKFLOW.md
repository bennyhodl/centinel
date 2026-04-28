# The Spotlight-Model Investigative Team: Operations Manual

A practical reference for fiction set inside a small, in-house investigative unit modeled on the Boston Globe's Spotlight team. Focused on roles, daily tasks, workflow, and the open-source tools the team would actually use.

---

## Part 1: Organizational structure

### The core team (the four-person unit in the locked room)

```
                    EXECUTIVE EDITOR
                          │
                  DEPUTY MANAGING EDITOR
                  (Projects oversight)
                          │
                  INVESTIGATIONS EDITOR ◄──── (player-coach;
                  ┌───────┼───────┐            reports + edits)
                  │       │       │
            LEAD REPORTER │   DATA/DATABASE REPORTER
                          │
                    REPORTER (2nd seat)
```

### The extended team (called in as needed; not in the locked room day-to-day)

```
INVESTIGATIONS EDITOR ──── NEWS RESEARCHER (records, archives, FOIA)
                      ──── IN-HOUSE COUNSEL (legal review)
                      ──── COPY/STANDARDS EDITOR (final pass)
                      ──── VISUAL JOURNALIST (graphics, photo)
                      ──── WEB PRODUCER (publishing infrastructure)
```

The Spotlight model is deliberately small. Four people in the locked room, a chain of command of five total above them, and a handful of specialists pulled in only at specific phases. The whole organism rarely exceeds a dozen people who know what's being investigated. Most colleagues in the larger newsroom learn about the story when readers do.

---

## Part 2: Roles, responsibilities, and daily tasks

### Investigations Editor (the "player-coach")
**Reports to:** Deputy Managing Editor.
**Manages:** Lead Reporter, second Reporter, Data Reporter.
**Defining trait:** Edits *and* reports. Sits in the same locked room as the team.

**Daily tasks**
- Opens the room each morning; reviews the team's status board (a shared task tracker) before anyone arrives.
- Runs a morning huddle (15–20 minutes) where each reporter states what they are doing today and what they are blocked on.
- Reads every interview memo and every document summary the reporters file the previous day; flags follow-ups in the margins.
- Conducts at least one on-the-record or background interview personally each day to stay close to the reporting.
- Vets every outgoing communication that names the target (calls to subjects, emails to lawyers, FOIA letters) before it leaves.
- Maintains the master story memo — a living document that reframes the investigation in 1,000 words after each new development. Rewrites it weekly.
- Handles upward communication to the Deputy Managing Editor; insulates the team from internal newsroom politics.
- Locks the room at end of day; confirms the encrypted laptop is sealed and the safe is secured.

**Weekly tasks**
- Holds a closed-door, no-laptops-allowed meeting once a week to step back and ask "what is the actual story?"
- Updates the Deputy Managing Editor in a one-on-one.
- Reviews the data reporter's database once a week with fresh eyes, looking for patterns the reporters missed.

---

### Lead Reporter
**Reports to:** Investigations Editor.
**Owns:** The principal source relationships and the central narrative spine.
**Defining trait:** Holds the story in their head. If hit by a bus, the investigation collapses for at least two weeks.

**Daily tasks**
- Arrives early; reviews overnight Signal messages from sources from a separate device (phone left in a Faraday pouch in the room).
- Triages 5–15 source contacts per day: which to call back today, which to push to next week, which to drop in person.
- Conducts 2–4 substantive interviews per day, by phone, in person, or via secure messenger. In-person interviews require travel; reporters often disappear from the office for half-days.
- Files an interview memo within 24 hours of every conversation: who, when, where, what was said, what was promised, source's reliability rating.
- Updates the source ledger (a separate, restricted-access file): every source listed by code name only, with the editor holding the cross-reference key.
- Drafts story sections in 500–1,500 word increments; never works on the master draft until the section is reviewed.
- Coordinates with the Data Reporter: brings names and dates from interviews to be matched against the database; takes patterns from the database back into the next round of interviews.

**Weekly tasks**
- Maintains the timeline of confirmed events, updated weekly as facts harden.
- Conducts at least one in-person, no-electronics meeting with a key source per week.
- Reviews the right-of-reply list — the people who must be contacted before publication — and works backward from the planned publish date.

---

### Reporter (second seat)
**Reports to:** Investigations Editor.
**Owns:** Secondary sources, document review, parallel reporting tracks.
**Defining trait:** Often comes from a beat (legal, business, health) and brings subject-matter expertise the lead reporter lacks.

**Daily tasks**
- Manages the document intake queue: every new file (FOIA response, leaked PDF, court filing, tip submission) gets logged, hashed, indexed, and tagged.
- Reads documents systematically — typically a target of 50–200 pages per day depending on density. Annotates wi[118;1:3uthin the document management system.
- Cross-checks names, dates, and dollar figures from documents against the team's database; flags discrepancies for the Data Reporter.
- Conducts the "second-tier" interviews: peripheral witnesses, former employees, regulators, academic experts. Frees the lead to focus on principal sources.
- Drafts memos summarizing each cluster of documents (e.g., "the 2014–2017 board minutes, summarized" — 3 pages from 400 pages of source material).
- Files FOIA, public records, and court records requests; tracks pending requests.
- Verifies factual claims the lead reporter is including in drafts: every name spelled correctly, every date confirmed against two sources, every dollar figure traceable to a primary document.

**Weekly tasks**
- Audits the document repository for organization: are files tagged correctly, is OCR working, are indexes complete?
- Reviews the right-of-reply file with the Lead Reporter: who has been contacted, who is still owed a chance to respond.
- Builds at least one new exhibit per week — a side-by-side comparison, a redacted version of a key document for publication, an annotated timeline.

---

### Data Reporter (Database Reporter)
**Reports to:** Investigations Editor.
**Owns:** The master spreadsheet/database, all structured data, all analysis.
**Defining trait:** Is a reporter, not a "data monkey." Files FOIAs, attends interviews, writes bylines.

**Daily tasks**
- Maintains the master database — typically a relational schema linking persons, organizations, transactions, dates, locations.
- Imports new records as the team acquires them: scraping public registries, parsing PDFs into structured rows, normalizing names ("J. Smith" / "John Smith" / "John A. Smith" reconciled to one entity).
- Runs the day's queries based on requests from reporters: "show me every transaction over $50,000 between these two entities between 2018 and 2021."
- Cleans data in the morning (the unglamorous 60% of the job): deduplication, format standardization, error correction.
- Builds a new visualization or summary statistic each day that the reporters can use as a reporting prompt — e.g., a count of incidents per region, a network graph of co-occurring names.
- Documents methodology: every analysis is reproducible. A separate methodology file grows alongside the story so the in-house counsel and standards editor can audit any number that appears in print.

**Weekly tasks**
- Backs up the entire database to two encrypted destinations.
- Audits data quality: spot-checks 20 random rows against original source documents to confirm fidelity.
- Produces a "data state" memo summarizing what has been collected, what is still missing, what cannot be obtained.
- Pairs with the Reporter for a half-day to walk through what new documents need to be entered.

---

### Deputy Managing Editor (Projects)
**Reports to:** Executive Editor.
**Manages:** Investigations Editor (and other project teams).
**Defining trait:** The story's institutional protector. Often a former investigative reporter.

**Daily tasks**
- Reads the team's daily status update from the Investigations Editor.
- Handles requests for resources (extra reporting time, travel budget, freelance translators, outside data licenses).
- Runs interference within the wider newsroom when colleagues notice the team is missing from regular coverage.
- Does *not* attend the daily huddle. Maintains professional distance so they can edit fresh later.

**Weekly tasks**
- Meets the Investigations Editor for one hour, closed-door.
- Reads a representative chunk of the latest draft material to maintain editorial feel for the story.
- Briefs the Executive Editor on progress at a fixed weekly slot.

---

### Executive Editor
**Reports to:** Publisher / board.
**Manages:** All editorial.
**Defining trait:** Approves the investigation, owns the legal exposure, signs off on publication.

**Daily tasks (during an active investigation)**
- None directly. Trusts the chain.

**Weekly tasks**
- Receives a verbal briefing from the Deputy Managing Editor.
- Reads major drafts at three points: at the proposal stage, at the midpoint review, and at the pre-publication final.

**Pre-publication**
- Convenes the legal/standards review meeting.
- Has the final word on whether to publish, hold, or kill.

---

### News Researcher (Research Librarian)
**Reports to:** Investigations Editor (project-by-project) and a separate research desk head institutionally.
**Defining trait:** A specialist in finding things, not interpreting them.

**Daily tasks (when assigned)**
- Pulls court records, corporate filings, property records, regulatory filings on demand. A reporter walks in with a name; the researcher returns within hours with everything publicly known.
- Maintains subscriptions to commercial databases (PACER for federal courts, state court systems, corporate registries, news archives).
- Drafts and tracks FOIA / public records requests; logs deadlines; escalates non-responsive agencies.
- Builds a "background file" on every named person of interest: birthdate, prior employers, prior litigation, public statements, family relationships, political donations.
- Handles requests for old newspaper clips, archived web pages, and historical context.

**Weekly tasks**
- Updates a master FOIA tracking sheet shared with the team.
- Briefs the team on any new public records or filings related to known subjects (a case docketed, a corporate registration, a property transfer).

---

### In-House Counsel (Newsroom Lawyer)
**Reports to:** General Counsel of the parent organization; functionally serves the Executive Editor.
**Defining trait:** Reads alongside reporters; does not "approve" stories but flags risk.

**Daily tasks (after the team brings them in)**
- Available on call; not embedded daily.
- Reviews key documents and interview memos for legal exposure (libel, privacy, source-shield issues).
- Drafts and reviews motions to unseal court records.
- Reviews subpoena responses and source-protection strategy.

**At pre-publication**
- Reads the full draft alongside the standards review.
- Asks for line-level changes: softening adjectives, attributing claims, adding qualifiers where warranted.
- Reviews the right-of-reply correspondence for fairness.
- Drafts the response letter the organization will send if a target's lawyers complain.
- Sometimes prepares a "second story" from purely public sources as a defensive backup.

---

### Copy/Standards Editor
**Reports to:** Executive Editor or a standards desk.
**Defining trait:** Reads only at the very end. The fresh eyes.

**Pre-publication tasks**
- Reads the full draft against the back-up file (every assertion footnoted to a source/document).
- Confirms every name is spelled the same way every time.
- Confirms every date is consistent.
- Confirms every quote matches the interview memo.
- Flags any sentence that asserts a fact without an evident source in the back-up file.
- Reviews redactions in published documents.

---

### Visual Journalist (called in 2–4 weeks before publication)
**Defining trait:** Builds the graphics, the document images, the network diagrams that appear with the story.

**Tasks**
- Designs the story's visual spine: timeline, network diagram, location maps.
- Coordinates with the Data Reporter on data accuracy in any chart.
- Prepares redacted PDFs of key documents for publication.
- Coordinates with the Web Producer on interactive elements.

---

### Web Producer (called in 1–2 weeks before publication)
**Defining trait:** Owns the publishing infrastructure; also runs the SecureDrop instance year-round.

**Tasks**
- Builds the story's landing page.
- Sets up document hosting (typically a self-hosted document viewer or DocumentCloud).
- Configures any interactive databases for public release.
- Coordinates the embargo: schedules the publication time, manages partner outlets if any, prepares social media.
- Maintains the SecureDrop server full-time as a separate ongoing duty.

---

## Part 3: The team's day, hour by hour

The four-person core plus the editor share a locked room. The day has a recognizable rhythm. The specifics vary by team, but the following is a typical pattern during the active middle phase of an investigation (months 3 through 9).

**07:30 – 09:00 — Arrivals and prep**
The Investigations Editor arrives first. Unlocks the room. Powers on the air-gapped workstation. Checks the overnight intake queue (any SecureDrop submissions, any scheduled FOIA responses arrived by post). Reviews the previous evening's status board.

The Data Reporter typically arrives next; runs morning data import jobs and starts cleaning routines that take an hour.

**09:00 – 09:20 — Morning huddle**
All four core members in the room. No phones (in Faraday pouches by the door). Each person states:
1. What they finished yesterday.
2. What they will do today.
3. What they are blocked on.
4. Any new thread that emerged overnight.

The editor takes notes in a single shared document. No PowerPoint, no formal agenda. Roughly 15–20 minutes.

**09:20 – 12:00 — Block of focused work**
- **Lead Reporter:** outbound calls and in-person interviews. Often leaves the building. Calls are made from a desk phone or burner, never from a personal cell.
- **Reporter:** document review at a workstation. Headphones on. Annotating in the document system.
- **Data Reporter:** running the morning's analysis queries. Producing a fresh visualization.
- **Editor:** reads and reacts to interview memos filed yesterday; conducts their own interview.

**12:00 – 13:00 — Working lunch**
Often eaten at desks. Sometimes the editor and one reporter walk to lunch outside the building specifically to discuss a sensitive thread that shouldn't be aired in the room (which also protects against any audio surveillance — a paranoid habit but not unreasonable).

**13:00 – 16:00 — Second focused block**
Cross-pollination starts. The Data Reporter brings findings to the Reporter ("these 12 names recur with this entity"). The Reporter takes the names back to documents to confirm. The Lead Reporter, returning from interviews, dictates an interview memo and asks for follow-ups.

**16:00 – 16:30 — Status check**
Brief, informal. The editor walks through what each person needs to file or finish before leaving. Sometimes the meeting is just "anything I need to know."

**16:30 – 18:30 — Drafting and filing**
Reporters file interview memos, document summaries, and updated section drafts. Data Reporter commits the day's database updates. Editor reads the day's filings.

**18:30 – 19:00 — Lockdown**
Workstations sealed. Sensitive paper documents returned to the safe. Burner phones placed in their drawer. Encrypted disks unmounted. The keycarded door locks behind the last person.

### The weekly rhythm

| Day | Standing event |
|---|---|
| Monday | Editor + Deputy Managing Editor 1:1 (08:00) |
| Tuesday | Full-team "step back" meeting, no laptops, 90 minutes |
| Wednesday | Data review session (Data Reporter walks team through database) |
| Thursday | News Researcher delivers weekly background updates |
| Friday | Right-of-reply / publication path review |

### The investigation's life cycle

| Phase | Duration | Headcount in room | Defining tasks |
|---|---|---|---|
| **Tip and triage** | 2–6 weeks | Editor + 1 reporter | Decide if the story exists. Initial sourcing. No locked room yet. |
| **Discovery** | 2–4 months | 4 core | Source mapping. Document collection. Database build-out. |
| **Hardening** | 2–6 months | 4 core | Confirming everything. Closing gaps. Identifying the unknowns. |
| **Right-of-reply** | 2–6 weeks | 4 core + lawyer | Subjects contacted; team braces for legal threats and counter-PR. |
| **Pre-publication** | 1–3 weeks | 4 core + lawyer + standards + visuals + web | Final draft cycles. Legal review. Production. |
| **Publication and aftermath** | Weeks to months | 4 core + reinforcements | Initial story drops. Follow-ups daily. Tip line floods. New sources surface. |

---

## Part 4: How information moves through the team

### Document intake (every new file)

1. **Logged.** A receipt entry with timestamp, source (or "anonymous"), method of receipt (SecureDrop, mail, in-person), file size, file hashes (SHA-256).
2. **Triaged for sensitivity.** High-sensitivity (leaks, anonymous source) goes to the air-gapped workstation only. Low-sensitivity (court filings, FOIA returns) can go to networked workstations.
3. **Hashed and stored.** Original file written once to encrypted storage. The hash becomes the document's permanent ID.
4. **OCRed.** Image PDFs converted to searchable text.
5. **Indexed.** Full-text search built so the document is discoverable by keyword.
6. **Tagged.** Persons, organizations, dates, places extracted (manually or with named-entity recognition) and entered into the master database.
7. **Summarized.** A reporter writes a 1–3 paragraph summary stored alongside the document.

### Daily data flow

```
Interview ──► Memo ──► Names/dates/figures extracted ──► Database
                              │
Document ──► OCR ──► Index ──► Names/dates/figures extracted ──► Database
                              │
                              ▼
                    Database queries surface patterns
                              │
                              ▼
                    Patterns sent back to reporters
                              │
                              ▼
                    Next round of interviews and document requests
```

### The team's working files

| File | Owner | Update frequency | Access |
|---|---|---|---|
| Master story memo | Editor | Weekly | All four |
| Source ledger (codenames + cross-reference key) | Editor | As needed | Editor only; cross-reference key in safe |
| Interview memos | Each reporter | Within 24h of interview | All four |
| Master timeline | Lead Reporter | Daily | All four |
| Master database | Data Reporter | Daily | All four |
| Document repository | Reporter | Continuous | All four |
| FOIA tracker | News Researcher | Weekly | All four + researcher |
| Right-of-reply log | Reporter | Weekly | All four + lawyer |
| Methodology document | Data Reporter | Weekly | All four + lawyer + standards |
| Back-up file (footnotes for every claim) | Editor + reporters | Continuous in final weeks | All four + lawyer + standards |

---

## Part 5: Open-source tools by function

A small team can run a serious investigation entirely on free, open-source software. The list below favors tools that are genuinely open source (FOSS), self-hostable, and used in real newsrooms.

### Document storage, OCR, and search

- **DocumentCloud** — open source platform for uploading, OCRing, annotating, and publishing primary-source documents. Run by MuckRock Foundation. Free for journalists. The default in newsrooms.
- **Aleph** (OCCRP) — open source platform for indexing, cross-referencing, and searching massive document collections plus structured datasets (corporate registries, sanctions lists, leaks). Self-hostable. Code at github.com/alephdata/aleph.
- **Datashare** (ICIJ) — open source desktop application for searching and analyzing local document collections. Runs on a single laptop or scales to a server. OCR, named-entity extraction, full-text search built in.
- **Apache Tika** — open source content extraction; pulls text out of nearly any file format.
- **Apache Tesseract** — open source OCR engine; the workhorse behind most document-processing pipelines.
- **Apache Solr** — open source search engine; powers DocumentCloud and many newsroom search tools.
- **Elasticsearch** (open core) — search and indexing; powers Datashare and Aleph.
- **Paperless-ngx** — open source document management system; small newsrooms use it for FOIA returns, court filings, and other steady-state document flow.

### Databases and structured data

- **PostgreSQL** — open source relational database; the standard backend for any custom newsroom data system.
- **SQLite** — open source single-file database; great for portable, encrypted master spreadsheets.
- **Datasette** — open source tool that exposes any SQLite database as a browsable, queryable web interface; widely used by data journalists.
- **OpenRefine** — open source data cleaning tool; the standard for normalizing names, addresses, and messy spreadsheets.
- **csvkit** — open source command-line tools for CSV manipulation.

### Graph databases and network visualization

- **Neo4j Community Edition** — open source graph database; the same engine ICIJ used for Panama Papers (in its commercial form), available free.
- **Gephi** — open source network visualization; the open alternative to commercial tools for relationship mapping.
- **Cytoscape** — open source network analysis; originally for biology, widely used for journalistic network mapping.

### Secure communication and source protection

- **SecureDrop** — open source whistleblower submission system; maintained by Freedom of the Press Foundation. Self-hosted; the gold standard.
- **Signal** — open source encrypted messenger; the universal source-comms tool.
- **GnuPG (GPG)** — open source PGP implementation; for encrypted email and file signing.
- **OnionShare** — open source tool for sending files of any size anonymously over Tor.
- **Element / Matrix** — open source secure chat platform; self-hostable Slack alternative for team communication.
- **Mattermost** (Team Edition) — open source self-hosted Slack alternative; used by newsrooms that cannot use commercial chat for legal-discovery reasons.
- **Briar** — open source peer-to-peer messenger; works without internet (Bluetooth, Wi-Fi); useful for in-person source meetings in surveilled environments.

### Secure operating systems and disk encryption

- **Tails** — open source Linux that boots from a USB stick, runs in RAM, routes everything through Tor; leaves no trace. The standard for handling leaked documents.
- **Qubes OS** — open source compartmentalized operating system; every application runs in its own isolated VM. Recommended for journalists handling state-actor threat models.
- **VeraCrypt** — open source disk encryption; for encrypting external drives, USB sticks, and document repositories.
- **LUKS** — open source full-disk encryption for Linux; the default for any serious newsroom workstation.
- **Cryptomator** — open source client-side encryption for cloud storage (so a Google Drive or Dropbox folder is encrypted before upload).

### Password and credential management

- **KeePassXC** — open source local password manager; the standard for newsroom credential management.
- **Vaultwarden** — open source self-hosted Bitwarden server; for shared team credentials with audit trail.
- **YubiKey** (hardware, not software, but works with open standards) — for hardware 2FA on every account.

### Notes, drafts, and collaboration

- **Standard Notes** — open source encrypted notes; cross-platform; reporters use it for sensitive interview prep and source notes.
- **Joplin** — open source notes with end-to-end encryption sync; supports markdown, attachments, search.
- **CryptPad** — open source encrypted collaborative documents; the Google Docs replacement for sensitive drafting.
- **Etherpad** — open source collaborative editing; self-hostable; useful for the team's master story memo.
- **OnlyOffice / Collabora Online** — open source self-hosted office suites; full Word/Excel/PowerPoint compatibility.
- **Nextcloud** — open source file sync and collaboration platform; replaces Google Drive, Dropbox, and Office 365 in one package; self-hostable; widely deployed in security-conscious organizations.
- **BookStack** — open source self-hosted wiki; for the team's running knowledge base on a target organization.

### Web research and archiving

- **ArchiveBox** — open source self-hosted web archiving; saves full pages with screenshots and PDFs; preserves evidence before takedowns.
- **Wallabag** — open source read-it-later and article archiving.
- **SingleFile** — open source browser extension that saves a complete webpage as a single HTML file.
- **Wayback Machine** browser extensions (open source) — for triggering archive captures and finding historical versions.

### Geolocation and OSINT

- **OpenStreetMap** — open data alternative to Google Maps; full data export available.
- **QGIS** — open source geographic information system; for spatial analysis and mapping.
- **Mapillary** — street-level imagery (open data layer available).
- **OSINT Framework** — open catalog of OSINT tools; not software but a navigation aid.

### Workflow and project management

- **Taiga** — open source project management; Kanban and scrum-style boards; self-hostable.
- **Wekan** — open source Trello alternative; self-hostable Kanban board.
- **Mattermost Boards** — Kanban built into the Mattermost chat platform.
- **Kimai** — open source time tracking; useful when reporting time has to be allocated across multiple investigations.

### Recommended self-hosted stack for a small team

A four-to-eight person investigative unit can run its entire technical infrastructure on a single rented server or in-house machine using the following stack:

| Layer | Tool |
|---|---|
| Server OS | Debian or Ubuntu Server with full-disk encryption (LUKS) |
| File storage and collaboration | Nextcloud |
| Document management and OCR | DocumentCloud (cloud) or Aleph (self-hosted) |
| Search across documents | Datashare (desktop) or Aleph (server) |
| Database | PostgreSQL + Datasette interface |
| Graph database | Neo4j Community Edition |
| Team chat | Element/Matrix or Mattermost |
| Encrypted notes | Joplin (with self-hosted sync) |
| Collaborative editing | CryptPad or Etherpad |
| Password sharing | Vaultwarden |
| Whistleblower submissions | SecureDrop (separate dedicated hardware) |
| Source comms | Signal |
| Web archiving | ArchiveBox |
| Project tracking | Wekan or Taiga |
| Workstation OS | Tails (USB, for sensitive work); Qubes OS (for daily) |
| Disk encryption | VeraCrypt for portable drives; LUKS for workstations |

The whole stack is free. The annual cost to operate is the rented server (a few hundred dollars), the SecureDrop hardware (a few hundred dollars one time), YubiKeys for the team (around $50 each), and burner phones with prepaid SIMs.

---

## Part 6: How a single piece of evidence travels

To anchor everything above, here is how one document — a leaked internal memo — moves from arrival to publication.

**Day 1 (Tuesday morning, 08:42).** A SecureDrop submission lands on the dedicated air-gapped workstation. The Web Producer (who maintains the SecureDrop instance) sees only that there is a new submission; cannot read the content; alerts the Investigations Editor.

**Day 1 (09:15).** The Investigations Editor and the Reporter assigned to document review enter the secure room. They boot the Tails laptop from its USB stick. They retrieve the encrypted submission, decrypt it on the air-gapped Secure Viewing Station, and find a 47-page PDF.

**Day 1 (09:30).** The Reporter computes the SHA-256 hash of the file, logs it in the document register with a timestamp and a code name ("Document MARROW-014"). The original file is copied once to a VeraCrypt-encrypted external drive that lives in the safe.

**Day 1 (10:00).** The Reporter runs OCR on a working copy using Tesseract. The text is extracted and indexed in the team's Aleph or Datashare instance.

**Day 1 (11:30).** The Reporter writes a 2-page summary memo: what the document is, who created it, what it appears to claim, what it would mean if true, what corroboration would be needed. The memo is filed in CryptPad. The Investigations Editor reads it within an hour.

**Day 1 (afternoon).** The Editor decides this is significant enough to bring to the next morning's huddle. The Lead Reporter and Data Reporter are not yet briefed.

**Day 2 (morning huddle).** The Editor presents Document MARROW-014. The team agrees on three immediate steps:
1. The Data Reporter pulls every name and date from the document and cross-references the master database for matches.
2. The News Researcher pulls public records on the entities named.
3. The Lead Reporter identifies who, in their existing source network, could plausibly authenticate the memo without compromising the source.

**Days 3–14.** The document is corroborated. Every claim in it is matched against an independent second source — a public filing, a second leaked document, a witness on background, a regulator's testimony.

**Days 15–60.** The document becomes one element in a broader story. Its specific claims are folded into the master timeline and the master database. The reporters continue independent reporting; the document is one anchor among many.

**Pre-publication (around month 6).** The In-House Counsel reads the document and the back-up file. The Lawyer asks: do we name the document publicly? Do we publish a redacted version? Do we describe its contents only? Each option has different legal exposure.

**Publication day.** The Web Producer publishes a redacted copy of the document on a self-hosted document viewer, linked from the story. The Visual Journalist has prepared an annotated graphic highlighting the three most important sentences. The Standards Editor has confirmed every quotation from the document matches the document exactly.

**Post-publication.** The original encrypted file remains in the safe. The hash chain is preserved as evidence of authenticity in case of legal challenge. The source's identity, recorded only in the source ledger under the code name, never leaves the editor's control.

---

## Summary: what makes the Spotlight model distinct

A four-person core in a locked room. An editor who reports as well as edits. Discipline about secrecy that extends *within* the newsroom, not just outside it. A small specialist support team called in only when needed. A long timeline (six months minimum, often two years) protected from daily news pressures. A document- and database-heavy methodology where every claim in the final story is footnoted to a primary source in the back-up file.

The team is small enough that everyone knows everything. The infrastructure is small enough that one Data Reporter can hold the database in their head. The legal exposure is large enough that the Executive Editor and the In-House Counsel must approve publication. The operation is built so that, on the day the story drops, the team has already imagined every possible counterattack and has a documented answer ready for each.
