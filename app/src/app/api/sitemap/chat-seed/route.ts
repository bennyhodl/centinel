import { NextRequest } from "next/server";
import { z } from "zod";
import { refreshExtract, getOrRefreshExtract } from "@/lib/tavily";
import { hermesComplete } from "@/lib/hermes";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const Body = z.object({
  url: z.string().url(),
  sessionId: z.string().min(1).max(200),
  refresh: z.boolean().default(false),
});

/**
 * POST /api/sitemap/chat-seed
 *
 * Used by the sitemap leaf "chat about this page" / "refresh & chat" flow.
 *
 *  1. Get the Tavily extract for the URL (cached or fresh).
 *  2. Send a single seed message to the Hermes session — content is the
 *     page extract framed as context. The session id is what the user
 *     will then resume on /chat.
 *
 * Returns the extract metadata + a sessionId the client should redirect to.
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
  const { url, sessionId, refresh } = parsed.data;

  let extract;
  try {
    extract = refresh
      ? await refreshExtract(url)
      : await getOrRefreshExtract(url);
  } catch (e) {
    return Response.json(
      { error: "extract_failed", detail: e instanceof Error ? e.message : String(e) },
      { status: 502 },
    );
  }

  const truncated = extract.raw_content.slice(0, 12000);

  const seedPrompt = [
    "You are about to discuss a single web page with a civic investigator.",
    "I am injecting the page's extracted content below so you have it for",
    "the rest of this conversation. Acknowledge briefly (1 sentence) what",
    "the page is about, then wait for the operator's question.",
    "",
    `URL: ${url}`,
    `Title: ${extract.title ?? "(untitled)"}`,
    `Fetched: ${extract.fetched_at}`,
    "",
    "--- PAGE CONTENT (truncated to 12k chars) ---",
    truncated,
    "--- END PAGE CONTENT ---",
  ].join("\n");

  let assistantReply: string;
  try {
    assistantReply = await hermesComplete({
      prompt: seedPrompt,
      sessionId,
      maxTokens: 200,
    });
  } catch (e) {
    return Response.json(
      { error: "hermes_failed", detail: e instanceof Error ? e.message : String(e) },
      { status: 502 },
    );
  }

  return Response.json({
    ok: true,
    sessionId,
    title: extract.title,
    fetched_at: extract.fetched_at,
    content_chars: extract.raw_content.length,
    assistant_preview: assistantReply.slice(0, 400),
  });
}
