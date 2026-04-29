import { spawn } from "node:child_process";
import { NextRequest } from "next/server";
import { z } from "zod";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const MessageSchema = z.object({
  role: z.enum(["user", "assistant", "system"]),
  content: z.string(),
});

const RequestSchema = z.object({
  messages: z.array(MessageSchema).min(1),
  // Stable session name so multi-turn context persists. The web client
  // generates a UUID once per browser tab and keeps it in localStorage;
  // we pass it to `hermes chat --continue <name>` so each turn resumes
  // the same Hermes session.
  sessionId: z.string().min(1).max(64).regex(/^[A-Za-z0-9._-]+$/).optional(),
});

const HERMES_BIN = process.env.HERMES_BIN_PATH || "hermes";
const SKILL_NAME = "centinel-operator";

/**
 * /chat is now a real Hermes session — no more pre-RAG QMD injection or
 * OpenAI tool-calling. The `centinel-operator` skill teaches the agent
 * how to use the bin/centinel CLI; Hermes' built-in terminal/file/qmd
 * tools provide the execution and retrieval surface.
 *
 * Request shape: standard {messages: [...]}. The handler:
 *   1. Pulls the latest user message (only one per turn).
 *   2. Spawns `hermes chat -q -Q -s centinel-operator --continue <session>`.
 *   3. Streams stdout back to the browser as text/plain.
 *
 * Continuity: passing `--continue <sessionId>` resumes a named session
 * across messages, so the agent has the prior turns as context. The web
 * client owns the session id (stored in localStorage) so it's stable
 * for the life of the tab.
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

  // Default session if the client didn't provide one. Single-operator
  // case ends up with one shared session — fine for v0.1.
  const sessionId = parsed.data.sessionId ?? "centinel-web-chat";

  const args = [
    "chat",
    "-q",
    userQuery,
    "-Q", // quiet: suppress banner/spinner/tool previews
    "-s",
    SKILL_NAME,
    "--continue",
    sessionId,
  ];

  let child: ReturnType<typeof spawn>;
  try {
    child = spawn(HERMES_BIN, args, {
      env: { ...process.env },
      stdio: ["ignore", "pipe", "pipe"],
    });
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
    start(controller) {
      let closed = false;
      const safeEnqueue = (chunk: Uint8Array) => {
        if (closed) return;
        try {
          controller.enqueue(chunk);
        } catch {
          // already closed
        }
      };
      const closeOnce = () => {
        if (closed) return;
        closed = true;
        try {
          controller.close();
        } catch {
          // already closed
        }
      };

      child.stdout?.on("data", (chunk: Buffer) => {
        safeEnqueue(chunk);
      });

      child.stderr?.on("data", (chunk: Buffer) => {
        // Surface stderr inline (italicized) so the user sees agent errors
        // rather than them silently disappearing.
        const text = chunk.toString("utf-8");
        if (text.trim()) {
          safeEnqueue(encoder.encode(`\n\n_[hermes stderr: ${text.trim()}]_`));
        }
      });

      child.on("error", (err: Error) => {
        safeEnqueue(
          encoder.encode(`\n\n_[hermes failed to start: ${err.message}]_`),
        );
        closeOnce();
      });

      child.on("close", () => {
        closeOnce();
      });

      abort.addEventListener("abort", () => {
        try {
          child.kill("SIGTERM");
        } catch {
          // noop
        }
        closeOnce();
      });
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
