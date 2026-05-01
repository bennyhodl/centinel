import { NextRequest } from "next/server";
import fs from "node:fs/promises";
import path from "node:path";
import { z } from "zod";
import { wikiPath } from "@/lib/config";
import { loadSitemap, SitemapEntryStatus } from "@/lib/sitemap";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const Body = z.object({
  url: z.string().url(),
  status: SitemapEntryStatus,
});

/**
 * POST /api/sitemap/triage  → flips status on a sitemap entry.
 * Used by the per-leaf approve/exclude buttons. Writes back to sitemap.json.
 */
export async function POST(req: NextRequest) {
  let body: unknown;
  try {
    body = await req.json();
  } catch {
    return Response.json({ error: "invalid_json" }, { status: 400 });
  }
  const parsed = Body.safeParse(body);
  if (!parsed.success) {
    return Response.json(
      { error: "invalid_request", issues: parsed.error.issues },
      { status: 400 },
    );
  }

  const doc = await loadSitemap();
  if (!doc) {
    return Response.json({ error: "sitemap_missing" }, { status: 404 });
  }

  const entry = doc.entries.find((e) => e.url === parsed.data.url);
  if (!entry) {
    return Response.json({ error: "url_not_found" }, { status: 404 });
  }

  entry.status = parsed.data.status;
  entry.notes = [
    ...entry.notes,
    `[triage ${new Date().toISOString()}] operator → ${parsed.data.status}`,
  ];

  const sitemapJsonPath = path.join(wikiPath(), "Sitemap", "sitemap.json");
  await fs.writeFile(sitemapJsonPath, JSON.stringify(doc, null, 2));

  return Response.json({ ok: true, status: entry.status });
}
