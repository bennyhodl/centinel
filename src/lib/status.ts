import fs from "node:fs/promises";
import path from "node:path";
import matter from "gray-matter";
import { runtimePath, wikiPath } from "./config";

export interface BoardSnapshot {
  body: string;
  mtime: number;
}

export type ActivityType = "request" | "response" | "notify" | "escalation";

export interface ActivityItem {
  id: string;
  filePath: string;
  timestamp: string; // ISO
  from: string;
  to: string;
  type: ActivityType | string;
  priority: string;
  summary: string;
  references: string[];
}

const BOARD_REL = path.join("status", "board.md");

export function boardAbsPath(): string {
  return path.join(runtimePath(), "status", "board.md");
}

export async function readBoard(): Promise<BoardSnapshot> {
  const abs = boardAbsPath();
  try {
    const [stat, body] = await Promise.all([
      fs.stat(abs),
      fs.readFile(abs, "utf-8"),
    ]);
    return { body, mtime: Math.floor(stat.mtimeMs) };
  } catch {
    return { body: "", mtime: 0 };
  }
}

/**
 * Filename: <YYYY-MM-DD>-<HHMM>-<from>-<short-slug>.md
 * Returns a Date or null.
 */
function parseFilenameDate(filename: string): Date | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})-(\d{2})(\d{2})-/.exec(filename);
  if (!m) return null;
  const [, y, mo, d, hh, mm] = m;
  // Treat as local time — filenames are produced locally.
  return new Date(Number(y), Number(mo) - 1, Number(d), Number(hh), Number(mm));
}

function flattenReferences(refs: unknown): string[] {
  if (!refs) return [];
  if (Array.isArray(refs)) {
    return refs.filter((r): r is string => typeof r === "string");
  }
  if (typeof refs === "object") {
    const out: string[] = [];
    for (const v of Object.values(refs as Record<string, unknown>)) {
      if (typeof v === "string") out.push(v);
      else if (Array.isArray(v))
        for (const x of v) if (typeof x === "string") out.push(x);
    }
    return out;
  }
  return [];
}

function summaryFromBody(body: string): string {
  const lines = body.split(/\r?\n/);
  for (const raw of lines) {
    const line = raw.trim();
    if (!line) continue;
    if (/^#{1,6}\s/.test(line)) continue;
    const cleaned = line.replace(/^[-*]\s+/, "").replace(/^>\s*/, "");
    if (!cleaned) continue;
    return cleaned.length > 120 ? cleaned.slice(0, 117) + "…" : cleaned;
  }
  return "";
}

async function isInvestigationConfidential(
  slug: string,
  cache: Map<string, boolean>,
): Promise<boolean> {
  if (cache.has(slug)) return cache.get(slug)!;
  const abs = path.join(wikiPath(), "Investigations", `${slug}.md`);
  try {
    const raw = await fs.readFile(abs, "utf-8");
    const { data } = matter(raw);
    const conf = Boolean(
      (data as Record<string, unknown>).confidential === true,
    );
    cache.set(slug, conf);
    return conf;
  } catch {
    cache.set(slug, false);
    return false;
  }
}

/**
 * Extract investigation slugs from a normalized list of references.
 * Accepts: "investigation:slug", "Investigations/slug", "Investigations/slug.md", or bare slugs from the structured 'investigation' key (handled in flatten).
 */
function investigationSlugsFromRefs(
  raw: unknown,
  flat: string[],
): string[] {
  const out = new Set<string>();
  // Structured: { investigation: 'slug' }
  if (raw && typeof raw === "object" && !Array.isArray(raw)) {
    const inv = (raw as Record<string, unknown>).investigation;
    if (typeof inv === "string" && inv) out.add(inv);
  }
  for (const r of flat) {
    const m =
      /^Investigations\/([^/.]+)(?:\.md)?$/i.exec(r) ||
      /^investigation:([\w-]+)$/i.exec(r);
    if (m) out.add(m[1]);
  }
  return [...out];
}

async function walkOutbox(root: string): Promise<string[]> {
  // Layout: <root>/<agent>/<YYYY-MM>/*.md
  const out: string[] = [];
  let agents: string[];
  try {
    agents = await fs.readdir(root);
  } catch {
    return out;
  }
  for (const agent of agents) {
    if (agent.startsWith("_")) continue; // skip _expired etc.
    const agentDir = path.join(root, agent);
    let months: string[];
    try {
      const stat = await fs.stat(agentDir);
      if (!stat.isDirectory()) continue;
      months = await fs.readdir(agentDir);
    } catch {
      continue;
    }
    for (const month of months) {
      if (!/^\d{4}-\d{2}$/.test(month)) continue;
      const monthDir = path.join(agentDir, month);
      let files: string[];
      try {
        files = await fs.readdir(monthDir);
      } catch {
        continue;
      }
      for (const f of files) {
        if (f.endsWith(".md")) out.push(path.join(monthDir, f));
      }
    }
  }
  return out;
}

export async function listRecentActivity(
  days = 7,
): Promise<ActivityItem[]> {
  const outboxRoot = path.join(runtimePath(), "outbox");
  const files = await walkOutbox(outboxRoot);
  const cutoff = new Date();
  cutoff.setDate(cutoff.getDate() - days);

  const confidentialCache = new Map<string, boolean>();
  const items: ActivityItem[] = [];

  for (const abs of files) {
    const filename = path.basename(abs);
    const dt = parseFilenameDate(filename);
    if (!dt) continue;
    if (dt < cutoff) continue;

    let raw: string;
    try {
      raw = await fs.readFile(abs, "utf-8");
    } catch {
      continue;
    }
    let parsed: matter.GrayMatterFile<string>;
    try {
      parsed = matter(raw);
    } catch {
      continue;
    }
    const fm = parsed.data as Record<string, unknown>;
    const refsRaw = fm.references;
    const references = flattenReferences(refsRaw);

    // Skip if any reference is to a confidential investigation.
    const invSlugs = investigationSlugsFromRefs(refsRaw, references);
    let suppressed = false;
    for (const slug of invSlugs) {
      if (await isInvestigationConfidential(slug, confidentialCache)) {
        suppressed = true;
        break;
      }
    }
    if (suppressed) continue;

    items.push({
      id:
        typeof fm.id === "string"
          ? fm.id
          : filename.replace(/\.md$/, ""),
      filePath: abs,
      timestamp:
        typeof fm.created === "string" ? fm.created : dt.toISOString(),
      from: typeof fm.from === "string" ? fm.from : "unknown",
      to: typeof fm.to === "string" ? fm.to : "unknown",
      type: typeof fm.type === "string" ? fm.type : "notify",
      priority:
        typeof fm.priority === "string" ? fm.priority : "normal",
      summary: summaryFromBody(parsed.content),
      references,
    });
  }

  items.sort((a, b) => (a.timestamp < b.timestamp ? 1 : -1));
  return items;
}

export { BOARD_REL };
