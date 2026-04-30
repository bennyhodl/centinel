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
  // Optional override — when omitted, we use the persistent default session.
  // Lets the client start a fresh conversation without polluting history.
  sessionId: z.string().min(1).max(128).optional(),
});

const DEFAULT_SESSION_ID = "centinel-web-chat";

/**
 * /chat is a streaming proxy to Hermes' OpenAI-compatible API server. We
 * translate Hermes' SSE stream into NDJSON events the client can render
 * structurally:
 *
 *   { "type": "delta",        "text": "..."         }   // assistant content chunk
 *   { "type": "tool_call",    "id": "...", "name": "...", "args": { ... } }
 *   { "type": "tool_output",  "id": "...", "text": "...", "truncated": true }
 *   { "type": "error",        "message": "..."      }
 *   { "type": "done" }
 *
 * Hermes emits two SSE event types:
 *   - default OpenAI chat-completions chunks  (content deltas)
 *   - `event: hermes.tool.progress`           (function_call / function_call_output)
 */
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

  const lastUser = [...parsed.data.messages].reverse().find((m) => m.role === "user");
  const userQuery = lastUser?.content?.trim();
  if (!userQuery) {
    return Response.json({ error: "empty_message" }, { status: 400 });
  }

  const baseURL = config.hermesApiUrl();
  const apiKey = config.hermesApiKey();
  if (!baseURL) {
    return Response.json(
      { error: "server_misconfigured", detail: "HERMES_API_URL is not configured" },
      { status: 500 },
    );
  }

  const url = baseURL.replace(/\/$/, "") + "/chat/completions";
  const payload = {
    model: process.env.HERMES_MODEL ?? "hermes-default",
    messages: [{ role: "user", content: userQuery }],
    stream: true,
  };

  const sessionId = parsed.data.sessionId?.trim() || DEFAULT_SESSION_ID;

  let upstream: Response;
  try {
    upstream = await fetch(url, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${apiKey || "missing"}`,
        "X-Hermes-Session-Id": sessionId,
      },
      body: JSON.stringify(payload),
      signal: req.signal,
    });
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    return Response.json(
      { error: "hermes_unavailable", detail: msg },
      { status: 502 },
    );
  }

  if (!upstream.ok || !upstream.body) {
    let detail = `HTTP ${upstream.status}`;
    try {
      const text = await upstream.text();
      detail = text.slice(0, 1000) || detail;
    } catch {
      /* ignore */
    }
    return Response.json({ error: "hermes_error", detail }, { status: 502 });
  }

  const encoder = new TextEncoder();
  const decoder = new TextDecoder();

  const readable = new ReadableStream<Uint8Array>({
    async start(controller) {
      let closed = false;
      const reader = upstream.body!.getReader();
      let buffer = "";
      // Counter so each tool call gets a stable id even when Hermes doesn't
      // give us one explicitly — used by the client to pair calls + outputs.
      let toolSeq = 0;
      // Stack of pending tool call ids; tool_output frames pop the last one.
      const toolStack: string[] = [];

      const send = (obj: Record<string, unknown>) => {
        if (closed) return;
        try {
          controller.enqueue(encoder.encode(JSON.stringify(obj) + "\n"));
        } catch {
          /* already closed */
        }
      };

      const closeOnce = () => {
        if (closed) return;
        closed = true;
        try {
          controller.close();
        } catch {
          /* already closed */
        }
      };

      try {
        while (true) {
          const { value, done } = await reader.read();
          if (done) break;
          buffer += decoder.decode(value, { stream: true });

          // SSE frames are separated by blank lines.
          let sep: number;
          while ((sep = buffer.indexOf("\n\n")) !== -1) {
            const frame = buffer.slice(0, sep);
            buffer = buffer.slice(sep + 2);

            const eventLine = frame
              .split(/\r?\n/)
              .find((l) => l.startsWith("event:"));
            const eventName = eventLine
              ? eventLine.slice("event:".length).trim()
              : "message";

            const dataLines = frame
              .split(/\r?\n/)
              .filter((l) => l.startsWith("data:"))
              .map((l) => l.slice(5).trimStart());
            if (dataLines.length === 0) continue;

            const dataPayload = dataLines.join("\n");
            if (dataPayload === "[DONE]") {
              send({ type: "done" });
              closeOnce();
              return;
            }

            let parsed: unknown;
            try {
              parsed = JSON.parse(dataPayload);
            } catch {
              continue; // malformed frame
            }

            if (eventName === "hermes.tool.progress") {
              const tp = parsed as {
                type?: "function_call" | "function_call_output";
                id?: string;
                name?: string;
                arguments?: string;
                output?: string | unknown;
              };

              if (tp.type === "function_call") {
                let args: Record<string, unknown> | string = {};
                try {
                  args = tp.arguments
                    ? (JSON.parse(tp.arguments) as Record<string, unknown>)
                    : {};
                } catch {
                  args = tp.arguments ?? "";
                }
                const id = tp.id || `t${++toolSeq}`;
                toolStack.push(id);
                send({
                  type: "tool_call",
                  id,
                  name: tp.name ?? "tool",
                  args,
                });
              } else if (tp.type === "function_call_output") {
                const raw =
                  typeof tp.output === "string"
                    ? tp.output
                    : JSON.stringify(tp.output ?? "");
                const LIMIT = 1200;
                const truncated = raw.length > LIMIT;
                const text = truncated ? raw.slice(0, LIMIT) : raw;
                const id = tp.id || toolStack.pop() || `t${toolSeq}`;
                send({
                  type: "tool_output",
                  id,
                  text,
                  truncated,
                  fullLength: raw.length,
                });
              }
              continue;
            }

            // Default: OpenAI chat-completion chunk.
            const delta = (
              parsed as {
                choices?: { delta?: { content?: string | null } }[];
              }
            ).choices?.[0]?.delta?.content;
            if (delta) {
              send({ type: "delta", text: delta });
            }
          }
        }
        send({ type: "done" });
      } catch (e) {
        if (!closed) {
          const msg = e instanceof Error ? e.message : String(e);
          send({ type: "error", message: msg });
        }
      } finally {
        closeOnce();
      }
    },
  });

  return new Response(readable, {
    headers: {
      "Content-Type": "application/x-ndjson; charset=utf-8",
      "Cache-Control": "no-cache, no-transform",
      "X-Accel-Buffering": "no",
    },
  });
}
