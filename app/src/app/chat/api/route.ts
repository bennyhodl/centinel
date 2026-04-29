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
});

// Stable session id passed via X-Hermes-Session-Id so Hermes loads the
// session's history server-side. The web client sends only the new
// user message; Hermes owns multi-turn context.
const SESSION_ID = "centinel-web-chat";

/**
 * /chat is a thin streaming proxy to Hermes' OpenAI-compatible API server
 * (`gateway/platforms/api_server.py` — POST /v1/chat/completions).
 *
 *   1. Take the latest user message.
 *   2. POST to <HERMES_API_URL>/chat/completions with stream=true and
 *      X-Hermes-Session-Id: centinel-web-chat for session continuity.
 *   3. Translate the SSE stream from OpenAI's chat-completions format
 *      to plain text for the browser, extracting `delta.content` chunks.
 *
 * The Hermes API server is responsible for skill loading (controlled by
 * `platform_toolsets.api_server` in ~/.hermes/config.yaml plus
 * `hermes skills config` enabling the centinel-operator skill), session
 * state (per X-Hermes-Session-Id), and tool execution. We just stream.
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
  // We send only the new user message — the Hermes session has the
  // prior turns. The model field is informational; Hermes routes to
  // whatever the api_server platform is configured for.
  const payload = {
    model: process.env.HERMES_MODEL ?? "hermes-default",
    messages: [{ role: "user", content: userQuery }],
    stream: true,
  };

  let upstream: Response;
  try {
    upstream = await fetch(url, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "Authorization": `Bearer ${apiKey || "missing"}`,
        "X-Hermes-Session-Id": SESSION_ID,
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
    return Response.json(
      { error: "hermes_error", detail },
      { status: 502 },
    );
  }

  const encoder = new TextEncoder();
  const decoder = new TextDecoder();

  const readable = new ReadableStream<Uint8Array>({
    async start(controller) {
      let closed = false;
      const reader = upstream.body!.getReader();
      let buffer = "";

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

            // Each frame may have multiple `data: ...` lines. We
            // concatenate the data payloads per the SSE spec.
            const dataLines = frame
              .split(/\r?\n/)
              .filter((l) => l.startsWith("data:"))
              .map((l) => l.slice(5).trimStart());
            if (dataLines.length === 0) continue;

            const dataPayload = dataLines.join("\n");
            if (dataPayload === "[DONE]") {
              closeOnce();
              return;
            }

            try {
              const parsed = JSON.parse(dataPayload) as {
                choices?: { delta?: { content?: string | null } }[];
              };
              const delta = parsed.choices?.[0]?.delta?.content;
              if (delta) {
                controller.enqueue(encoder.encode(delta));
              }
            } catch {
              // Malformed frame — skip silently rather than blow up the stream.
            }
          }
        }
      } catch (e) {
        if (!closed) {
          const msg = e instanceof Error ? e.message : String(e);
          try {
            controller.enqueue(
              encoder.encode(`\n\n_[stream error: ${msg}]_`),
            );
          } catch {
            /* ignore */
          }
        }
      } finally {
        closeOnce();
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
