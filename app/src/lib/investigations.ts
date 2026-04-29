import fs from "node:fs/promises";
import path from "node:path";
import { listMarkdown, readMarkdown, safeResolve } from "./wiki";

export type InvestigationStatus = "active" | "paused" | "complete" | string;

export interface InvestigationSummary {
  slug: string;
  frontmatter: Record<string, unknown>;
  excerpt: string;
}

export interface InvestigationDoc {
  slug: string;
  frontmatter: Record<string, unknown>;
  body: string;
}

function excerpt(body: string, n = 200): string {
  const stripped = body
    .replace(/^#.*$/gm, "")
    .replace(/\[\[([^\]|\n]+?)(?:\|([^\]\n]+))?\]\]/g, (_m, t, l) => l ?? t)
    .replace(/[*_`>]/g, "")
    .trim();
  return stripped.length > n ? stripped.slice(0, n).trimEnd() + "…" : stripped;
}

function statusRank(s: unknown): number {
  if (s === "active") return 0;
  if (s === "paused") return 1;
  if (s === "complete") return 2;
  return 3;
}

export async function listInvestigations(): Promise<InvestigationSummary[]> {
  const files = await listMarkdown("Investigations");
  const docs = await Promise.all(
    files.map(async (rel) => {
      try {
        const doc = await readMarkdown(rel);
        return {
          slug: path.basename(rel, ".md"),
          frontmatter: doc.frontmatter,
          excerpt: excerpt(doc.body),
        } satisfies InvestigationSummary;
      } catch {
        return null;
      }
    }),
  );
  const items = docs.filter((d): d is InvestigationSummary => !!d);
  items.sort((a, b) => {
    const r = statusRank(a.frontmatter.status) - statusRank(b.frontmatter.status);
    if (r !== 0) return r;
    const av = String(a.frontmatter.created_at ?? a.frontmatter.created ?? "");
    const bv = String(b.frontmatter.created_at ?? b.frontmatter.created ?? "");
    return bv.localeCompare(av);
  });
  return items;
}

export async function readInvestigation(
  slug: string,
): Promise<InvestigationDoc | null> {
  const rel = path.posix.join("Investigations", `${slug}.md`);
  try {
    safeResolve(rel);
    const doc = await readMarkdown(rel);
    return { slug, frontmatter: doc.frontmatter, body: doc.body };
  } catch {
    return null;
  }
}

/** Slugify a free-form title into a stable kebab-case filename. */
export function slugifyTitle(title: string): string {
  return title
    .toLowerCase()
    .normalize("NFKD")
    .replace(/[\u0300-\u036f]/g, "") // strip diacritics
    .replace(/['"`’]/g, "")
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 80);
}

export type InvestigationSchedule = "daily" | "weekly" | "monthly" | "manual";

export interface CreateInvestigationInput {
  title: string;
  goal: string;
  seeds: string[];
  schedule: InvestigationSchedule;
  depth: number;
}

export interface CreateInvestigationResult {
  slug: string;
  absPath: string;
}

function todayIso(): string {
  return new Date().toISOString().slice(0, 10);
}

function renderInvestigationMarkdown(input: CreateInvestigationInput): string {
  const today = todayIso();
  const seedsYaml = input.seeds.length
    ? input.seeds.map((s) => `  - ${s}`).join("\n")
    : "  []";
  const seedsList = input.seeds.length
    ? input.seeds.map((s) => `- ${s}`).join("\n")
    : "<!-- no seeds yet -->";
  // Frontmatter shape matches civic-investigator/templates/investigation.md.
  return `---
title: ${JSON.stringify(input.title)}
goal: |
  ${input.goal.replace(/\n/g, "\n  ")}
seeds:
${seedsYaml}
status: active
depth: ${input.depth}
schedule: ${input.schedule}
created: ${today}
updated: ${today}
auto_complete: false
confidential: false
---

# ${input.title}

## Goal
${input.goal}

## Seeds
${seedsList}

## Methodology
<!-- Operator's hand-written notes on approach, hypotheses, what "done" looks like.
     The Investigator reads this section but never edits it. -->

## Notes
<!-- Operator's running notes. Free-form. The Investigator never touches this. -->

## Findings (auto-appended)
<!-- Investigator appends one bullet per draft finding emitted, with link. -->

## Open Questions
<!-- Both operator and Investigator append; never delete. -->

## Run log
<!-- Investigator owns; append-only; one ### Run YYYY-MM-DD HH:MM block per run. -->
`;
}

/**
 * Create a new investigation file in `<wiki>/Investigations/<slug>.md`.
 *
 * - Atomic write (`<slug>.md.tmp` → rename).
 * - Refuses to clobber an existing file.
 * - Does NOT register the cron job — caller shells out to
 *   `bin/centinel investigate register <slug>` for that.
 */
export async function createInvestigation(
  input: CreateInvestigationInput,
): Promise<CreateInvestigationResult> {
  const slug = slugifyTitle(input.title);
  if (!slug) throw new Error("Title must contain at least one alphanumeric character");

  const rel = path.posix.join("Investigations", `${slug}.md`);
  const abs = safeResolve(rel);

  // Ensure parent dir exists.
  await fs.mkdir(path.dirname(abs), { recursive: true });

  // Refuse to clobber.
  try {
    await fs.access(abs);
    throw new Error(`Investigation already exists: ${slug}`);
  } catch (e: unknown) {
    const err = e as NodeJS.ErrnoException;
    if (err && err.code !== "ENOENT") throw e;
  }

  const tmp = `${abs}.tmp`;
  await fs.writeFile(tmp, renderInvestigationMarkdown(input), "utf-8");
  await fs.rename(tmp, abs);

  return { slug, absPath: abs };
}

/** Find [[Findings/...]] wikilinks referenced from an investigation body. */
export function extractFindingLinks(body: string): string[] {
  const out = new Set<string>();
  const re = /\[\[([^\]\n]+?)\]\]/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(body))) {
    const target = m[1].split("|")[0].trim();
    if (/^findings\//i.test(target)) {
      out.add(target);
    }
  }
  return [...out];
}
