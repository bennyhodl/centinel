import { NextRequest } from "next/server";
import { z } from "zod";
import OpenAI from "openai";
import { config } from "@/lib/config";
import { buildChatPrompt } from "@/lib/chat-prompt";
import { qmdQuery, renderHitsAsContext } from "@/lib/wiki-search";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const MessageSchema = z.object({
  role: z.enum(["user", "assistant", "system"]),
  content: z.string(),
});

const RequestSchema = z.object({
  messages: z.array(MessageSchema).min(1),
});

// Hermes exposes an OpenAI-compatible endpoint. We assume model="gpt-4o" is
// accepted; Hermes routes to whatever it has configured. Override-friendly via
// HERMES_MODEL env if the operator wants something else.
const DEFAULT_MODEL = process.env.HERMES_MODEL ?? "gpt-4o";

let _client: OpenAI | null = null;
function getClient(): OpenAI {
  if (_client) return _client;
  const baseURL = config.hermesApiUrl();
  const apiKey = config.hermesApiKey();
  if (!baseURL) {
    throw new Error("HERMES_API_URL is not configured");
  }
  _client = new OpenAI({
    baseURL,
    apiKey: apiKey || "missing",
  });
  return _client;
}

export async function POST(req: NextRequest) {
  let body: unknown;
  try {
    body = await req.json();
  } catch {
    return Response.json({ error: "invalid_json" }, { status: 400 });
  }

  const parsed = RequestSchema.safeParse(body);
  if (!parsed.success) {
    return Response.json(
      { error: "invalid_request", issues: parsed.error.issues },
      { status: 400 },
    );
  }

  // ── Pre-RAG: run QMD on the latest user message ─────────────────────────
  // Per docs/EDITOR_ANSWER_SOURCES.md: "QMD always runs." The chat is a
  // wiki Q&A surface, so retrieval is mandatory, not optional. We pull the
  // most recent user message (skip trailing assistant placeholders) and use
  // it as the search query.
  const lastUser = [...parsed.data.messages].reverse().find((m) => m.role === "user");
  const userQuery = lastUser?.content ?? "";
  const hits = await qmdQuery(userQuery, 6);
  const contextBlock = renderHitsAsContext(hits);
  const systemPrompt = buildChatPrompt(contextBlock);

  let client: OpenAI;
  try {
    client = getClient();
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    return Response.json({ error: "server_misconfigured", detail: msg }, { status: 500 });
  }

  const messages = [
    { role: "system" as const, content: systemPrompt },
    ...parsed.data.messages.map((m) => ({ role: m.role, content: m.content })),
  ];

  let stream: AsyncIterable<{ choices: { delta: { content?: string | null } }[] }>;
  try {
    stream = (await client.chat.completions.create({
      model: DEFAULT_MODEL,
      messages,
      stream: true,
    })) as unknown as AsyncIterable<{
      choices: { delta: { content?: string | null } }[];
    }>;
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    return Response.json(
      { error: "hermes_unavailable", detail: msg },
      { status: 502 },
    );
  }

  const encoder = new TextEncoder();
  const abort = req.signal;

  const readable = new ReadableStream<Uint8Array>({
    async start(controller) {
      try {
        for await (const chunk of stream) {
          if (abort.aborted) break;
          const delta = chunk.choices?.[0]?.delta?.content;
          if (delta) controller.enqueue(encoder.encode(delta));
        }
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        controller.enqueue(
          encoder.encode(`\n\n_[stream error: ${msg}]_`),
        );
      } finally {
        controller.close();
      }
    },
  });

  return new Response(readable, {
    headers: {
      "Content-Type": "text/plain; charset=utf-8",
      "Cache-Control": "no-cache, no-transform",
      "X-Accel-Buffering": "no",
    },
  });
}
