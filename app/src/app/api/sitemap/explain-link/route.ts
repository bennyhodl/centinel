import { NextRequest } from "next/server";
import fs from "node:fs/promises";
import path from "node:path";
import { z } from "zod";
import { wikiPath } from "@/lib/config";
import { loadSitemap } from "@/lib/sitemap";
import { hermesComplete } from "@/lib/hermes";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const Body = z.object({
  page_url: z.string().url(),
  link_href: z.string(),
  link_anchor: z.string().default(""),
});

/**
 * POST /api/sitemap/explain-link
 *
 * Returns a one-line LLM-written explanation of where this link goes and
 * why a civic investigator might care. Persists the answer onto the
 * sitemap entry's matching link record so we don't pay twice.
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

  const { page_url, link_href, link_anchor } = parsed.data;

  const prompt = [
    "You are a civic investigator's assistant. In ONE sentence (max 30 words),",
    "explain what this link likely points to and why an investigator might",
    "care. Be concrete; never use the words 'this link' or 'this page'.",
    "",
    `Source page: ${page_url}`,
    `Link anchor text: "${link_anchor || "(none)"}"`,
    `Link URL: ${link_href}`,
  ].join("\n");

  let summary: string;
  try {
    summary = await hermesComplete({
      prompt,
      sessionId: "centinel-link-explainer",
      maxTokens: 80,
    });
  } catch (e) {
    return Response.json(
      { error: "hermes_failed", detail: e instanceof Error ? e.message : String(e) },
      { status: 502 },
    );
  }

  // Persist on the sitemap entry's matching link (best-effort).
  try {
    const doc = await loadSitemap();
    if (doc) {
      const entry = doc.entries.find((e) => e.url === page_url);
      if (entry) {
        const link = entry.links.find((l) => l.href === link_href);
        if (link) {
          link.llm_summary = summary;
          const sitemapJsonPath = path.join(wikiPath(), "Sitemap", "sitemap.json");
          await fs.writeFile(sitemapJsonPath, JSON.stringify(doc, null, 2));
        }
      }
    }
  } catch {
    // best-effort; don't fail the response if persistence breaks
  }

  return Response.json({ ok: true, summary });
}
