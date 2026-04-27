import fs from "node:fs/promises";
import path from "node:path";
import matter from "gray-matter";
import { wikiPath } from "./config";

export interface MarkdownDoc {
  relPath: string;
  frontmatter: Record<string, unknown>;
  body: string;
}

/**
 * Resolve a wiki-relative path to an absolute path inside <wikiPath>.
 * Throws if the resolved path escapes the wiki root.
 */
export function safeResolve(relPath: string): string {
  const root = wikiPath();
  const abs = path.resolve(root, relPath);
  if (!abs.startsWith(root + path.sep) && abs !== root) {
    throw new Error(`path escapes wiki root: ${relPath}`);
  }
  return abs;
}

export async function readMarkdown(relPath: string): Promise<MarkdownDoc> {
  const abs = safeResolve(relPath);
  const raw = await fs.readFile(abs, "utf-8");
  const { data, content } = matter(raw);
  return { relPath, frontmatter: data, body: content };
}

export async function listMarkdown(dir: string): Promise<string[]> {
  const abs = safeResolve(dir);
  let entries: string[];
  try {
    entries = await fs.readdir(abs);
  } catch {
    return [];
  }
  return entries
    .filter((e) => e.endsWith(".md"))
    .map((e) => path.posix.join(dir, e));
}

/**
 * [[Foo/bar]] → /sitemap/foo/bar  (lower-cased path segments)
 * [[Contractors/acme]] → /entities/contractor/acme  (singularize 'Contractors' → 'contractor')
 *
 * Stub heuristic — refine as wiki conventions firm up.
 */
const ENTITY_TYPE_MAP: Record<string, string> = {
  contractors: "contractor",
  people: "person",
  orgs: "org",
  organizations: "org",
  projects: "project",
};

export function resolveWikilink(target: string): string {
  const cleaned = target.trim().replace(/\\/g, "/");
  const [head, ...rest] = cleaned.split("/").filter(Boolean);
  if (!head) return "/sitemap";
  const lowerHead = head.toLowerCase();
  if (ENTITY_TYPE_MAP[lowerHead] && rest.length >= 1) {
    const slug = rest.map((r) => r.toLowerCase()).join("/");
    return `/entities/${ENTITY_TYPE_MAP[lowerHead]}/${slug}`;
  }
  const segs = [head, ...rest].map((s) => s.toLowerCase());
  return `/sitemap/${segs.join("/")}`;
}
