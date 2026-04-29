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
});

const HERMES_BIN = process.env.HERMES_BIN_PATH || "hermes";
const SKILL_NAME = "centinel-operator";
// Single, fixed session name. Hermes' --continue handles all multi-turn
// state — we don't manage session ids per-tab or per-message. Operator
// can wipe history with `hermes sessions delete centinel-web-chat`.
const SESSION_NAME = "centinel-web-chat";

/**
 * /chat is a thin streaming wrapper around `hermes chat`. Every request
 * resumes the same Hermes session via --continue, so multi-turn context,
 * skill memory, and tool history are all owned by Hermes — not by us.
 *
 *   1. Take the latest user message.
 *   2. Spawn `hermes chat -q -Q -s centinel-operator --continue centinel-web-chat`.
 *   3. Stream stdout to the browser.
 *
 * Hermes auto-creates the named session on first run and resumes it on
 * every subsequent run. We don't track ids client-side.
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

  const args = [
    "chat",
    "-q",
    userQuery,
    "-Q", // quiet: suppress banner/spinner/tool previews
    "-s",
    SKILL_NAME,
    "--continue",
    SESSION_NAME,
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
