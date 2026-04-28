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
