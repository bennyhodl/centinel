/**
 * /chat — proxy to the Centinel runtime server.
 *
 * Phase 3 of the pi-agent migration (see docs/PI_MIGRATION_PLAN.md). The
 * route used to call Hermes' OpenAI-compatible endpoint directly; now it
 * forwards to centinel-server's stateful `/chat` SSE surface and
 * re-projects the editor's events back to the existing client contract
 * (a `text/plain` stream of raw text deltas).
 *
 * Client → this route:
 *   POST { messages: [{role, content}, ...], sessionId?: string }
 *
 * Client ← this route:
 *   text/plain stream of assistant text deltas, plus an optional
 *   `x-centinel-chat-session-id` response header so the UI can persist the
 *   session id across reloads in a future change.
 *
 * The implementation extracts the latest user message (the rest is held by
 * the server-side AgentSession, which carries the full thread history)
 * and posts it to `POST /chat` on centinel-server, asking for SSE.
 *
 * Phase 4 will swap this for a richer protocol the UI can use to render
 * tool calls and delegations live.
 */

import { NextRequest } from "next/server";
import { z } from "zod";
import { config } from "@/lib/config";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const MessageSchema = z.object({
  role: z.enum(["user", "assistant", "system"]),
  content: z.string(),
});

const RequestSchema = z.object({
  messages: z.array(MessageSchema).min(1),
  sessionId: z.string().optional(),
});

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

  // Extract the last user message — the server-side AgentSession owns the
  // running history. The client still posts the whole thread for now
  // (compat with the existing chat page); we'll drop that in Phase 4.
  const lastUser = [...parsed.data.messages].reverse().find((m) => m.role === "user");
  if (!lastUser) {
    return Response.json({ error: "no_user_message" }, { status: 400 });
  }

  // Caller may also pass sessionId in a header for compat with future clients.
  const headerSessionId = req.headers.get("x-centinel-chat-session-id") ?? undefined;
  const sessionId = parsed.data.sessionId ?? headerSessionId;

  const serverUrl = config.centinelServerUrl();
  const upstream = await fetch(`${serverUrl}/chat`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      accept: "text/event-stream",
    },
    body: JSON.stringify({ message: lastUser.content, sessionId }),
    signal: req.signal,
  });

  if (!upstream.ok || !upstream.body) {
    const detail = await upstream.text().catch(() => "");
    return Response.json(
      { error: "centinel_server_unavailable", status: upstream.status, detail },
      { status: 502 },
    );
  }

  const encoder = new TextEncoder();
  const decoder = new TextDecoder();
  let resolvedSessionId: string | undefined;
  let resolveHeaders: () => void = () => {};
  const headersReady = new Promise<void>((res) => {
    resolveHeaders = res;
  });

  const readable = new ReadableStream<Uint8Array>({
    async start(controller) {
      const reader = upstream.body!.getReader();
      let sseBuffer = "";
      let sawAnyText = false;

      try {
        while (true) {
          const { value, done } = await reader.read();
          if (done) break;
          sseBuffer += decoder.decode(value, { stream: true });

          // SSE frames are separated by \n\n
          let idx: number;
          while ((idx = sseBuffer.indexOf("\n\n")) !== -1) {
            const frame = sseBuffer.slice(0, idx);
            sseBuffer = sseBuffer.slice(idx + 2);
            const dataLine = frame.split("\n").find((l) => l.startsWith("data:"));
            if (!dataLine) continue;
            const data = dataLine.slice(5).trim();
            if (!data) continue;
            let obj: unknown;
            try {
              obj = JSON.parse(data);
            } catch {
              continue;
            }
            const envelope = obj as {
              t?: string;
              type?: string;
              event?: { type?: string; assistantMessageEvent?: { type?: string; delta?: string } };
              sessionId?: string;
              finalText?: string;
            };

            // First-frame meta: {type:"_chat_session", sessionId}
            if (envelope.type === "_chat_session" && typeof envelope.sessionId === "string") {
              resolvedSessionId = envelope.sessionId;
              resolveHeaders();
              continue;
            }

            // Pi event envelopes have shape { t, event: {...} }
            const innerType = envelope.event?.type;
            if (innerType === "message_update") {
              const ame = envelope.event!.assistantMessageEvent;
              if (ame?.type === "text_delta" && typeof ame.delta === "string") {
                controller.enqueue(encoder.encode(ame.delta));
                sawAnyText = true;
              }
              continue;
            }

            if (envelope.type === "_chat_end") {
              if (!sawAnyText && typeof envelope.finalText === "string" && envelope.finalText) {
                controller.enqueue(encoder.encode(envelope.finalText));
              }
              break;
            }

            if (envelope.type === "_chat_error" || envelope.type === "_chat_busy") {
              controller.enqueue(
                encoder.encode(
                  `\n\n_[chat ${envelope.type === "_chat_busy" ? "busy" : "error"}]_`,
                ),
              );
              break;
            }
          }
        }
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        controller.enqueue(encoder.encode(`\n\n_[stream error: ${msg}]_`));
      } finally {
        // Ensure headers resolve even if we never got a _chat_session frame.
        resolveHeaders();
        controller.close();
      }
    },
  });

  // Race: wait for headers (or 250ms timeout) so we can include the
  // session-id response header.
  await Promise.race([
    headersReady,
    new Promise<void>((res) => setTimeout(res, 250)),
  ]);

  const headers: Record<string, string> = {
    "Content-Type": "text/plain; charset=utf-8",
    "Cache-Control": "no-cache, no-transform",
    "X-Accel-Buffering": "no",
  };
  if (resolvedSessionId) headers["x-centinel-chat-session-id"] = resolvedSessionId;

  return new Response(readable, { headers });
}
