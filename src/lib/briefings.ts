import path from "node:path";
import { listMarkdown, readMarkdown, safeResolve } from "./wiki";

export interface BriefingSummary {
  slug: string;
  date: string;
  headline: string;
  excerpt: string;
  frontmatter: Record<string, unknown>;
}

export interface BriefingDoc {
  slug: string;
  date: string;
  frontmatter: Record<string, unknown>;
  body: string;
}

function firstH1(body: string): string {
  const m = body.match(/^#\s+(.+?)\s*$/m);
  return m ? m[1] : "";
}

function excerpt(body: string, n = 280): string {
  const stripped = body
    .replace(/^#.*$/gm, "")
    .replace(/\[\[([^\]|\n]+?)(?:\|([^\]\n]+))?\]\]/g, (_m, t, l) => l ?? t)
    .replace(/[*_`>]/g, "")
    .trim();
  return stripped.length > n ? stripped.slice(0, n).trimEnd() + "…" : stripped;
}

export async function listBriefings(): Promise<BriefingSummary[]> {
  const files = await listMarkdown("Briefings");
  const docs = await Promise.all(
    files.map(async (rel) => {
      try {
        const doc = await readMarkdown(rel);
        const slug = path.basename(rel, ".md");
        return {
          slug,
          date: slug,
          headline: firstH1(doc.body) || (doc.frontmatter.title as string) || slug,
          excerpt: excerpt(doc.body),
          frontmatter: doc.frontmatter,
        } satisfies BriefingSummary;
      } catch {
        return null;
      }
    }),
  );
  const items = docs.filter((d): d is BriefingSummary => !!d);
  items.sort((a, b) => b.date.localeCompare(a.date));
  return items;
}

export async function readBriefing(slug: string): Promise<BriefingDoc | null> {
  const rel = path.posix.join("Briefings", `${slug}.md`);
  try {
    safeResolve(rel);
    const doc = await readMarkdown(rel);
    return { slug, date: slug, frontmatter: doc.frontmatter, body: doc.body };
  } catch {
    return null;
  }
}
