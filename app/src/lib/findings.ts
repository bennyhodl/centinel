import fs from "node:fs/promises";
import path from "node:path";
import matter from "gray-matter";
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

/**
 * Promote a draft finding to published.
 *
 * Per WEB_APP_DESIGN.md: "Publish finding | mv draft/foo.md published/foo.md".
 * We additionally stamp `published_at` into the frontmatter so the new file
 * sorts correctly on the published feed.
 *
 * Atomic enough for our needs: write the new file, then unlink the draft.
 * If the unlink fails, the operator gets a duplicate they can clean up;
 * preferable to losing the body.
 */
export async function promoteDraftToPublished(slug: string): Promise<void> {
  const draftRel = path.posix.join("Findings", "draft", `${slug}.md`);
  const publishedRel = path.posix.join("Findings", "published", `${slug}.md`);

  const draftAbs = safeResolve(draftRel);
  const publishedAbs = safeResolve(publishedRel);

  // Refuse to clobber an existing published copy.
  try {
    await fs.access(publishedAbs);
    throw new Error(`Already published: ${slug}`);
  } catch (e: unknown) {
    const err = e as NodeJS.ErrnoException;
    if (err && err.code !== "ENOENT") throw e;
  }

  // Read draft, stamp published_at, write to published/, then unlink draft.
  let raw: string;
  try {
    raw = await fs.readFile(draftAbs, "utf-8");
  } catch (e: unknown) {
    const err = e as NodeJS.ErrnoException;
    if (err && err.code === "ENOENT") {
      throw new Error(`Draft not found: ${slug}`);
    }
    throw e;
  }

  const parsed = matter(raw);
  const fm = { ...parsed.data, published_at: new Date().toISOString() };
  // gray-matter's stringify preserves the `---` fences and keeps body verbatim.
  const stamped = matter.stringify(parsed.content, fm);

  await fs.mkdir(path.dirname(publishedAbs), { recursive: true });
  const tmp = `${publishedAbs}.tmp`;
  await fs.writeFile(tmp, stamped, "utf-8");
  await fs.rename(tmp, publishedAbs);

  await fs.unlink(draftAbs);
}
