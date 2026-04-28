import fs from "node:fs/promises";
import path from "node:path";
import { listMarkdown, readMarkdown, safeResolve } from "./wiki";

export const ENTITY_TYPES = ["contractor", "person", "org", "project"] as const;
export type EntityType = (typeof ENTITY_TYPES)[number];

export const ENTITY_TYPE_LABELS: Record<EntityType, string> = {
  contractor: "Contractors",
  person: "People",
  org: "Organizations",
  project: "Projects",
};

export interface EntitySummary {
  slug: string;
  type: EntityType;
  frontmatter: Record<string, unknown>;
  excerpt: string;
}

export interface EntityDoc {
  slug: string;
  type: EntityType;
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

function typeDir(type: EntityType): string {
  return path.posix.join("Entities", type);
}

export async function listEntityTypes(): Promise<
  { type: EntityType; label: string; count: number }[]
> {
  const out: { type: EntityType; label: string; count: number }[] = [];
  for (const type of ENTITY_TYPES) {
    let count = 0;
    try {
      const abs = safeResolve(typeDir(type));
      const entries = await fs.readdir(abs);
      count = entries.filter((e) => e.endsWith(".md")).length;
    } catch {
      count = 0;
    }
    out.push({ type, label: ENTITY_TYPE_LABELS[type], count });
  }
  return out;
}

export function isEntityType(t: string): t is EntityType {
  return (ENTITY_TYPES as readonly string[]).includes(t);
}

export async function listEntities(type: EntityType): Promise<EntitySummary[]> {
  const files = await listMarkdown(typeDir(type));
  const docs = await Promise.all(
    files.map(async (rel) => {
      try {
        const doc = await readMarkdown(rel);
        return {
          slug: path.basename(rel, ".md"),
          type,
          frontmatter: doc.frontmatter,
          excerpt: excerpt(doc.body),
        } satisfies EntitySummary;
      } catch {
        return null;
      }
    }),
  );
  const items = docs.filter((d): d is EntitySummary => !!d);
  items.sort((a, b) => {
    const ax = String(a.frontmatter.title ?? a.slug).toLowerCase();
    const bx = String(b.frontmatter.title ?? b.slug).toLowerCase();
    return ax.localeCompare(bx);
  });
  return items;
}

export async function readEntity(
  type: EntityType,
  slug: string,
): Promise<EntityDoc | null> {
  const rel = path.posix.join(typeDir(type), `${slug}.md`);
  try {
    safeResolve(rel);
    const doc = await readMarkdown(rel);
    return { slug, type, frontmatter: doc.frontmatter, body: doc.body };
  } catch {
    return null;
  }
}
