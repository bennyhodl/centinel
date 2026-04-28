import fs from "node:fs";
import { type NextRequest } from "next/server";
import { boardAbsPath, readBoard } from "@/lib/status";

export const dynamic = "force-dynamic";
export const runtime = "nodejs";

const ENCODER = new TextEncoder();

export async function GET(req: NextRequest) {
  const abs = boardAbsPath();

  const stream = new ReadableStream<Uint8Array>({
    async start(controller) {
      let closed = false;
      const send = (data: string) => {
        if (closed) return;
        try {
          controller.enqueue(ENCODER.encode(data));
        } catch {
          // controller already closed
        }
      };
      const sendEvent = async () => {
        const snap = await readBoard();
        send(`data: ${JSON.stringify(snap)}\n\n`);
      };
      const sendHeartbeat = () => send(`: ping ${Date.now()}\n\n`);

      // Initial push
      await sendEvent();

      // Debounced re-read on file change
      let debounce: NodeJS.Timeout | null = null;
      const trigger = () => {
        if (debounce) clearTimeout(debounce);
        debounce = setTimeout(() => {
          debounce = null;
          sendEvent().catch(() => {});
        }, 500);
      };

      // fs.watch may throw if the file doesn't exist yet; fall back to polling.
      let watcher: fs.FSWatcher | null = null;
      let pollTimer: NodeJS.Timeout | null = null;
      let lastMtime = 0;

      const startPolling = () => {
        pollTimer = setInterval(async () => {
          try {
            const stat = await fs.promises.stat(abs);
            const m = Math.floor(stat.mtimeMs);
            if (m !== lastMtime) {
              lastMtime = m;
              trigger();
            }
          } catch {
            // file missing — keep polling silently
          }
        }, 2000);
      };

      try {
        watcher = fs.watch(abs, { persistent: false }, () => trigger());
        watcher.on("error", () => {
          if (watcher) {
            watcher.close();
            watcher = null;
          }
          if (!pollTimer) startPolling();
        });
      } catch {
        startPolling();
      }

      const heartbeat = setInterval(sendHeartbeat, 15_000);

      const cleanup = () => {
        if (closed) return;
        closed = true;
        clearInterval(heartbeat);
        if (debounce) clearTimeout(debounce);
        if (pollTimer) clearInterval(pollTimer);
        if (watcher) {
          try {
            watcher.close();
          } catch {
            // ignore
          }
        }
        try {
          controller.close();
        } catch {
          // already closed
        }
      };

      req.signal.addEventListener("abort", cleanup);
    },
  });

  return new Response(stream, {
    status: 200,
    headers: {
      "Content-Type": "text/event-stream; charset=utf-8",
      "Cache-Control": "no-cache, no-transform",
      Connection: "keep-alive",
      "X-Accel-Buffering": "no",
    },
  });
}
