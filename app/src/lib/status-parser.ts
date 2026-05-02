// Client-safe board.md parser. Pure functions only — no fs / node imports.
// The full status helpers (file reading, activity walking) live in
// `@/lib/status` which pulls in node:fs/promises and can't be bundled into
// client components. This file is the import target for client UIs.

export interface InFlightRun {
  /** Free-form line as written by the agent, with bullet/dash stripped. */
  raw: string;
  /** Best-effort agent name (Investigator, Cartographer, Watch-runner, …). */
  agent: string | null;
  /** Best-effort target — investigation slug, watch id, or url snippet. */
  target: string | null;
  /** ISO timestamp when this run started, if we could parse one. */
  startedIso: string | null;
  /** Any extra inline status the agent wrote (e.g. "depth-crawl in progress"). */
  detail: string | null;
}

export interface RecentBoardLine {
  raw: string;
  agent: string | null;
}

export interface BoardSections {
  inFlight: InFlightRun[];
  recent: RecentBoardLine[];
  lastUpdated: string | null;
}

const KNOWN_AGENTS = [
  "Investigator",
  "Cartographer",
  "Watch-runner",
  "Watcher",
  "Data-reporter",
  "Reporter",
  "Archivist",
  "Editor",
];

function findAgentTag(line: string): string | null {
  const bracket = /\[([^\]]+)\]/.exec(line);
  if (bracket) {
    const v = bracket[1].trim();
    const hit = KNOWN_AGENTS.find((a) => a.toLowerCase() === v.toLowerCase());
    if (hit) return hit;
  }
  for (const a of KNOWN_AGENTS) {
    const re = new RegExp(`\\b${a}\\b`, "i");
    if (re.test(line)) return a;
  }
  return null;
}

function findStartedIso(line: string): string | null {
  const m =
    /started\s+(\d{4}-\d{2}-\d{2}[ T]\d{2}:\d{2}(?::\d{2})?)/i.exec(line) ||
    /(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:[.\d]*)?(?:Z|[+-]\d{2}:?\d{2})?)/.exec(
      line,
    );
  if (!m) return null;
  const candidate = m[1].replace(" ", "T");
  const d = new Date(candidate);
  return Number.isNaN(d.getTime()) ? null : d.toISOString();
}

function findInvestigationSlug(line: string): string | null {
  // Look at the FIRST pipe-delimited token (matches the user's
  // "lynn-hurtak | Investigator | started ..." format) before anything else.
  const head = line.split("|")[0].trim().toLowerCase();
  if (
    /^[a-z0-9][a-z0-9-]*-[a-z0-9-]+$/.test(head) &&
    !KNOWN_AGENTS.some((a) => a.toLowerCase() === head)
  ) {
    return head;
  }
  // Otherwise scan for a kebab-cased token elsewhere in the line.
  const m = /\b([a-z][a-z0-9]*-[a-z0-9-]{2,})\b/.exec(line);
  if (!m) return null;
  const cand = m[1];
  if (KNOWN_AGENTS.some((a) => a.toLowerCase() === cand.toLowerCase())) {
    return null;
  }
  return cand;
}

function parseInFlight(line: string): InFlightRun {
  const cleaned = line.replace(/^\s*[-*]\s+/, "").trim();
  const agent = findAgentTag(cleaned);
  const startedIso = findStartedIso(cleaned);
  const target = findInvestigationSlug(cleaned);

  let detail: string | null = null;
  if (cleaned.includes("|")) {
    const tail = cleaned.split("|").pop();
    if (tail) detail = tail.trim();
  } else {
    const m = /started\s+\S+(?:\s+\S+)?\s*[,–—-]?\s*(.+)$/i.exec(cleaned);
    if (m) detail = m[1].trim();
  }
  return { raw: cleaned, agent, target, startedIso, detail };
}

/**
 * Parse the agent-authored board.md into structured sections so the UI can
 * render rich run cards instead of dumping raw markdown.
 *
 * The skills agree on these section headers:
 *   ## In flight
 *   ## Last 24h activity
 * but bullet line FORMAT inside is freestyle. We extract what we can and
 * preserve `raw` so the UI can fall back to verbatim text if our heuristic
 * missed.
 */
export function parseBoard(markdown: string): BoardSections {
  const lines = markdown.split(/\r?\n/);
  let section: "in_flight" | "recent" | null = null;
  const inFlight: InFlightRun[] = [];
  const recent: RecentBoardLine[] = [];
  let lastUpdated: string | null = null;

  for (const raw of lines) {
    const line = raw.trim();

    if (!section) {
      const m =
        /(?:last\s+updated|updated|generated)[:\s]+([0-9TZ:.+\- ]+)/i.exec(
          line,
        );
      if (m) {
        const d = new Date(m[1].trim().replace(" ", "T"));
        if (!Number.isNaN(d.getTime())) lastUpdated = d.toISOString();
      }
    }

    if (/^#+\s*in\s*flight/i.test(line)) {
      section = "in_flight";
      continue;
    }
    if (/^#+\s*(last\s*24h|recent\s*activity|24h)/i.test(line)) {
      section = "recent";
      continue;
    }
    if (/^#+\s/.test(line)) {
      section = null;
      continue;
    }

    if (!line) continue;
    if (!/^[-*]\s+/.test(line)) continue;

    if (section === "in_flight") {
      inFlight.push(parseInFlight(line));
    } else if (section === "recent") {
      const cleaned = line.replace(/^\s*[-*]\s+/, "").trim();
      recent.push({ raw: cleaned, agent: findAgentTag(cleaned) });
    }
  }

  return { inFlight, recent, lastUpdated };
}
