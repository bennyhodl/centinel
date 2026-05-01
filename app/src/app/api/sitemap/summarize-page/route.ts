import { NextRequest } from "next/server";
import fs from "node:fs/promises";
import path from "node:path";
import { z } from "zod";
import { wikiPath } from "@/lib/config";
import { loadSitemap } from "@/lib/sitemap";
import { hermesComplete } from "@/lib/hermes";
import { getOrRefreshExtract, refreshExtract } from "@/lib/tavily";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const Body = z.object({
  url: z.string().url(),
  refresh: z.boolean().default(false),
});

/**
 * POST /api/sitemap/summarize-page
 *
 * Pulls the Tavily extract for `url` (cached or fresh), asks the LLM to
 * write a short factual summary of what's actually on the page, and persists
 * that onto the sitemap entry as `page_summary` + `page_summary_at`.
 *
 * Response: { ok, summary, fetched_at }
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
  const { url, refresh } = parsed.data;

  let extract;
  try {
    extract = refresh ? await refreshExtract(url) : await getOrRefreshExtract(url);
  } catch (e) {
    return Response.json(
      { error: "extract_failed", detail: e instanceof Error ? e.message : String(e) },
      { status: 502 },
    );
  }

  const truncated = extract.raw_content.slice(0, 12000);

  const prompt = [
    "You are a civic investigator's assistant. Read the page content below",
    "and write a SHORT factual summary (2–4 sentences, max 80 words) of what",
    "is actually on the page: what kind of content it contains (announcements,",
    "meeting notices, dataset, contracts, etc.), the most concrete items if",
    "any are listed, and any obvious time-sensitivity. Plain prose, no",
    "markdown, no preamble, no 'this page' / 'this link' filler.",
    "",
    `URL: ${url}`,
    `Title: ${extract.title ?? "(untitled)"}`,
    "",
    "--- PAGE CONTENT ---",
    truncated,
    "--- END PAGE CONTENT ---",
  ].join("\n");

  let summary: string;
  try {
    summary = await hermesComplete({
      prompt,
      sessionId: "centinel-page-summarizer",
      maxTokens: 220,
    });
  } catch (e) {
    return Response.json(
      { error: "hermes_failed", detail: e instanceof Error ? e.message : String(e) },
      { status: 502 },
    );
  }

  summary = summary.trim();
  const summarizedAt = new Date().toISOString();

  // Persist on the sitemap entry (best-effort).
  try {
    const doc = await loadSitemap();
    if (doc) {
      const entry = doc.entries.find((e) => e.url === url);
      if (entry) {
        entry.page_summary = summary;
        entry.page_summary_at = summarizedAt;
        const sitemapJsonPath = path.join(wikiPath(), "Sitemap", "sitemap.json");
        const tmp = `${sitemapJsonPath}.tmp`;
        await fs.writeFile(tmp, JSON.stringify(doc, null, 2));
        await fs.rename(tmp, sitemapJsonPath);
      }
    }
  } catch {
    // best-effort
  }

  return Response.json({
    ok: true,
    summary,
    fetched_at: extract.fetched_at,
    summarized_at: summarizedAt,
  });
}
