import { NextRequest } from "next/server";
import fs from "node:fs/promises";
import path from "node:path";
import { z } from "zod";
import { wikiPath } from "@/lib/config";
import { loadSitemap } from "@/lib/sitemap";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const Body = z.object({
  url: z.string().url(),
  investigation_slug: z.string().min(1),
  note: z.string().optional(),
});

/**
 * POST /api/sitemap/seed
 *
 * Appends a URL to <wiki>/Investigations/<slug>/seed-urls.md (creating it if
 * needed) and stamps the sitemap entry with `investigation_refs: [...slug]`.
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

  const { url, investigation_slug, note } = parsed.data;

  // Validate the investigation exists.
  const invDir = path.join(wikiPath(), "Investigations", investigation_slug);
  try {
    const stat = await fs.stat(invDir);
    if (!stat.isDirectory()) throw new Error("not a dir");
  } catch {
    return Response.json(
      { error: "investigation_not_found", slug: investigation_slug },
      { status: 404 },
    );
  }

  // Append to seed-urls.md
  const seedPath = path.join(invDir, "seed-urls.md");
  let existing = "";
  try {
    existing = await fs.readFile(seedPath, "utf-8");
  } catch {
    existing = `# Seed URLs\n\nURLs seeded into this investigation from the Sitemap atlas. The civic-investigator agent reads this list on each run.\n\n`;
  }
  const ts = new Date().toISOString();
  const line = `- [ ] \`${url}\` — seeded ${ts}${note ? ` — ${note}` : ""}\n`;
  await fs.writeFile(seedPath, existing + line);

  // Update sitemap entry investigation_refs
  const doc = await loadSitemap();
  if (doc) {
    const entry = doc.entries.find((e) => e.url === url);
    if (entry) {
      const refs = new Set(entry.investigation_refs ?? []);
      refs.add(investigation_slug);
      entry.investigation_refs = [...refs];
      const sitemapJsonPath = path.join(wikiPath(), "Sitemap", "sitemap.json");
      await fs.writeFile(sitemapJsonPath, JSON.stringify(doc, null, 2));
    }
  }

  return Response.json({ ok: true, seedPath });
}
