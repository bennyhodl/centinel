/**
 * SSE endpoint that streams the bootstrap log to the browser.
 *
 * The wizard's Step 6 opens an EventSource against this route. We tail
 * the log file by polling `fs.stat` for size changes and reading the
 * delta. Each line becomes a `data:` event. When the dispatcher exits
 * (the parent process is no longer alive AND no growth in 5s), we send
 * a `done` event and close.
 *
 * Why polling, not fs.watch:
 *  - fs.watch on Linux fires inconsistently for append-only writes.
 *  - The log volume is tiny (kbytes), so 500ms polling is fine.
 *
 * Why not just read the whole file each tick:
 *  - We track byte offset and read the delta, so the client only ever
 *    receives new lines.
 */
import fs from "node:fs/promises";
import { readSetupState } from "@/lib/setup-state";

export const dynamic = "force-dynamic";
export const runtime = "nodejs";

const POLL_INTERVAL_MS = 500;
const QUIET_DEADLINE_MS = 5_000; // mark "done" after this many ms with no growth

export async function GET() {
  const state = await readSetupState();
  const logPath = state.bootstrap?.logPath;

  const encoder = new TextEncoder();
  const stream = new ReadableStream({
    async start(controller) {
      const send = (event: string, data: string) => {
        controller.enqueue(encoder.encode(`event: ${event}\ndata: ${data}\n\n`));
      };

      if (!logPath) {
        send("error", "No bootstrap log path on file. Run Step 5 first.");
        controller.close();
        return;
      }

      let offset = 0;
      let lastGrowthAt = Date.now();
      let closed = false;

      const tick = async () => {
        if (closed) return;
        try {
          const stat = await fs.stat(logPath);
          if (stat.size > offset) {
            const fh = await fs.open(logPath, "r");
            try {
              const buf = Buffer.alloc(stat.size - offset);
              await fh.read(buf, 0, buf.length, offset);
              offset = stat.size;
              lastGrowthAt = Date.now();
              const text = buf.toString("utf-8");
              for (const line of text.split("\n")) {
                if (line.length > 0) send("line", line);
              }
            } finally {
              await fh.close();
            }
          }
          if (Date.now() - lastGrowthAt > QUIET_DEADLINE_MS) {
            send("done", "");
            closed = true;
            controller.close();
            return;
          }
          setTimeout(tick, POLL_INTERVAL_MS);
        } catch (err) {
          send(
            "error",
            err instanceof Error ? err.message : String(err),
          );
          closed = true;
          controller.close();
        }
      };

      // Replay everything that's already in the file before live-tailing.
      try {
        const initial = await fs.readFile(logPath, "utf-8");
        offset = Buffer.byteLength(initial, "utf-8");
        for (const line of initial.split("\n")) {
          if (line.length > 0) send("line", line);
        }
      } catch {
        // Log file doesn't exist yet — first tick will catch it.
      }

      // Cleanup if the client goes away.
      const abort = () => {
        closed = true;
        try {
          controller.close();
        } catch {
          /* already closed */
        }
      };
      // ReadableStream cancellation handler.
      // (Next.js wires this when the request is aborted.)
      void abort;

      setTimeout(tick, POLL_INTERVAL_MS);
    },
  });

  return new Response(stream, {
    headers: {
      "Content-Type": "text/event-stream",
      "Cache-Control": "no-cache, no-transform",
      Connection: "keep-alive",
    },
  });
}
