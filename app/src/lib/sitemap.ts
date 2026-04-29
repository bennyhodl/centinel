import fs from "node:fs/promises";
import path from "node:path";
import { z } from "zod";
import { wikiPath } from "./config";

/**
 * Sitemap schema — must match the contract emitted by the `sitemap-builder` skill.
 * See: ~/plans/centinel/research/skills/sitemap-builder.md
 */

export const SitemapEntryStatus = z.enum([
  "active",
  "broken",
  "excluded",
  "needs_review",
]);
export type SitemapEntryStatus = z.infer<typeof SitemapEntryStatus>;

export const SitemapEntryType = z.enum([
  "meetings",
  "contracts",
  "rfps",
  "budget",
  "boards",
  "permits",
  "ethics",
  "press",
  "personnel",
  "project",
  "document",
  "profile",
  "calendar",
  "form",
  "general",
]);
export type SitemapEntryType = z.infer<typeof SitemapEntryType>;

export const SitemapEntryContentKind = z.enum([
  "index",
  "document",
  "listing",
  "form",
  "profile",
  "news",
  "calendar",
  "search",
]);

export const SitemapEntry = z.object({
  url: z.string().url(),
  type: SitemapEntryType,
  description: z.string(),
  content_kind: SitemapEntryContentKind,
  contains: z.array(z.string()).default([]),
  linked_entities: z.array(z.string()).default([]),
  last_crawled: z.string(),
  content_hash: z.string(),
  parser: z.string().nullable().optional(),
  crawl_freq: z.string().optional(),
  status: SitemapEntryStatus,
  notes: z.array(z.string()).default([]),
});
export type SitemapEntry = z.infer<typeof SitemapEntry>;

export const SitemapDoc = z.object({
  domain: z.string(),
  generated_at: z.string(),
  entries: z.array(SitemapEntry),
});
export type SitemapDoc = z.infer<typeof SitemapDoc>;

export interface SitemapStats {
  total: number;
  byType: Record<string, number>;
  byStatus: Record<SitemapEntryStatus, number>;
  needsReview: number;
  broken: number;
}

function sitemapJsonPath(): string {
  return path.join(wikiPath(), "Sitemap", "sitemap.json");
}

export async function loadSitemap(): Promise<SitemapDoc | null> {
  try {
    const raw = await fs.readFile(sitemapJsonPath(), "utf-8");
    const parsed = JSON.parse(raw);
    // Tolerant load: some sitemap-builder runs emit a bare array of entries
    // instead of the wrapped {domain, generated_at, entries} doc. Coerce so
    // the web UI never blows up over a recoverable shape mismatch — and
    // rewrite the file in canonical form so downstream readers see the doc.
    let doc: unknown = parsed;
    if (Array.isArray(parsed)) {
      doc = {
        domain: "",
        generated_at: new Date().toISOString(),
        entries: parsed,
      };
      try {
        await fs.writeFile(sitemapJsonPath(), JSON.stringify(doc, null, 2));
      } catch {
        // best-effort canonicalization; don't fail the load if write is denied
      }
    }
    return SitemapDoc.parse(doc);
  } catch (err) {
    const e = err as NodeJS.ErrnoException;
    if (e.code === "ENOENT") return null;
    // Malformed sitemap — surface it loudly so we don't pretend it's empty.
    throw new Error(`Failed to load sitemap: ${e.message}`);
  }
}

export function computeStats(doc: SitemapDoc): SitemapStats {
  const byType: Record<string, number> = {};
  const byStatus: Record<SitemapEntryStatus, number> = {
    active: 0,
    broken: 0,
    excluded: 0,
    needs_review: 0,
  };
  for (const e of doc.entries) {
    byType[e.type] = (byType[e.type] ?? 0) + 1;
    byStatus[e.status] = (byStatus[e.status] ?? 0) + 1;
  }
  return {
    total: doc.entries.length,
    byType,
    byStatus,
    needsReview: byStatus.needs_review,
    broken: byStatus.broken,
  };
}

export function filterByType(doc: SitemapDoc, type: string): SitemapEntry[] {
  return doc.entries.filter((e) => e.type === type);
}

export function filterByStatus(
  doc: SitemapDoc,
  status: SitemapEntryStatus,
): SitemapEntry[] {
  return doc.entries.filter((e) => e.status === status);
}

export function searchEntries(doc: SitemapDoc, query: string): SitemapEntry[] {
  const q = query.toLowerCase();
  return doc.entries.filter(
    (e) =>
      e.url.toLowerCase().includes(q) ||
      e.description.toLowerCase().includes(q) ||
      e.contains.some((c) => c.toLowerCase().includes(q)),
  );
}

/**
 * Sort entries by status priority (needs_review first, then broken, then active),
 * then by URL alphabetically. Useful default ordering across views.
 */
const STATUS_ORDER: Record<SitemapEntryStatus, number> = {
  needs_review: 0,
  broken: 1,
  active: 2,
  excluded: 3,
};

export function sortEntries(entries: SitemapEntry[]): SitemapEntry[] {
  return [...entries].sort((a, b) => {
    const so = STATUS_ORDER[a.status] - STATUS_ORDER[b.status];
    if (so !== 0) return so;
    return a.url.localeCompare(b.url);
  });
}
