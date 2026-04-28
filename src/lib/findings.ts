import path from "node:path";
import { listMarkdown, readMarkdown, safeResolve } from "./wiki";

export type FindingStack = "raw" | "published" | "draft";

export interface FindingSummary {
  slug: string;
  stack: FindingStack;
  frontmatter: Record<string, unknown>;
  excerpt: string;
}

export interface FindingDoc {
  slug: string;
  stack: FindingStack;
  frontmatter: Record<string, unknown>;
  body: string;
  sources: string[];
}

function dirFor(stack: FindingStack): string {
  return path.posix.join("Findings", stack);
}

function dateField(stack: FindingStack): string {
  return stack === "draft"
    ? "drafted_at"
    : stack === "published"
      ? "published_at"
      : "generated_at";
}

function excerpt(body: string, n = 240): string {
  const stripped = body
    .replace(/^#.*$/gm, "")
    .replace(/\[\[([^\]|\n]+?)(?:\|([^\]\n]+))?\]\]/g, (_m, t, l) => l ?? t)
    .replace(/[*_`>]/g, "")
    .trim();
  return stripped.length > n ? stripped.slice(0, n).trimEnd() + "…" : stripped;
}

function toSlug(relPath: string): string {
  return path.basename(relPath, ".md");
}

export async function listFindings(
  stack: FindingStack,
): Promise<FindingSummary[]> {
  const files = await listMarkdown(dirFor(stack));
  const docs = await Promise.all(
    files.map(async (rel) => {
      try {
        const doc = await readMarkdown(rel);
        return {
          slug: toSlug(rel),
          stack,
          frontmatter: doc.frontmatter,
          excerpt: excerpt(doc.body),
        } satisfies FindingSummary;
      } catch {
        return null;
      }
    }),
  );
  const items = docs.filter((d): d is FindingSummary => !!d);
  const key = dateField(stack);
  items.sort((a, b) => {
    const av = String(a.frontmatter[key] ?? "");
    const bv = String(b.frontmatter[key] ?? "");
    return bv.localeCompare(av);
  });
  return items;
}

export async function listAllFindings(): Promise<FindingSummary[]> {
  const stacks: FindingStack[] = ["published", "draft", "raw"];
  const all = (await Promise.all(stacks.map(listFindings))).flat();
  all.sort((a, b) => {
    const av = String(
      a.frontmatter[dateField(a.stack)] ?? a.frontmatter["date"] ?? "",
    );
    const bv = String(
      b.frontmatter[dateField(b.stack)] ?? b.frontmatter["date"] ?? "",
    );
    return bv.localeCompare(av);
  });
  return all;
}

export async function readFinding(
  stack: FindingStack,
  slug: string,
): Promise<FindingDoc | null> {
  const rel = path.posix.join(dirFor(stack), `${slug}.md`);
  try {
    safeResolve(rel);
    const doc = await readMarkdown(rel);
    const fmSources = Array.isArray(doc.frontmatter.sources)
      ? (doc.frontmatter.sources as unknown[]).map(String)
      : [];
    const single =
      typeof doc.frontmatter.source_vault_path === "string"
        ? [doc.frontmatter.source_vault_path as string]
        : typeof doc.frontmatter.source_url === "string"
          ? [doc.frontmatter.source_url as string]
          : [];
    return {
      slug,
      stack,
      frontmatter: doc.frontmatter,
      body: doc.body,
      sources: [...fmSources, ...single],
    };
  } catch {
    return null;
  }
}

export async function findFindingAcrossStacks(
  slug: string,
): Promise<FindingDoc | null> {
  for (const stack of ["published", "draft", "raw"] as FindingStack[]) {
    const doc = await readFinding(stack, slug);
    if (doc) return doc;
  }
  return null;
}
