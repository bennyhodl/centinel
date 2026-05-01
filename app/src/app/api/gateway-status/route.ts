import { NextRequest } from "next/server";
import { execFile } from "node:child_process";
import path from "node:path";
import { promisify } from "node:util";

const run = promisify(execFile);

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

function centinelBin(): string {
  if (process.env.CENTINEL_BIN) return process.env.CENTINEL_BIN;
  return path.resolve(process.cwd(), "..", "bin", "centinel");
}

/**
 * GET /api/gateway-status
 *
 * Returns whether the Hermes scheduler/gateway process is alive. The
 * scheduler is what actually fires cron jobs (including operator Run-Now
 * triggers) — when it's down, every "trigger" is a no-op. We surface this
 * as a global banner so operators don't waste time clicking buttons that
 * silently do nothing.
 */
export async function GET(_req: NextRequest) {
  const bin = centinelBin();
  try {
    const { stdout } = await run(bin, ["gateway-status"], { timeout: 10_000 });
    const parsed = JSON.parse(stdout) as { running: boolean; detail: string };
    return Response.json(parsed, {
      headers: { "Cache-Control": "no-store" },
    });
  } catch (err) {
    const e = err as { stdout?: string; stderr?: string; message?: string };
    // gateway-status exits non-zero when down; execFile throws — try to
    // recover the JSON it printed before the non-zero exit.
    if (e.stdout) {
      try {
        const parsed = JSON.parse(e.stdout) as {
          running: boolean;
          detail: string;
        };
        return Response.json(parsed, {
          headers: { "Cache-Control": "no-store" },
        });
      } catch {
        // fall through
      }
    }
    return Response.json(
      {
        running: false,
        detail: (e.stderr || e.message || "gateway-status failed").trim(),
      },
      { headers: { "Cache-Control": "no-store" } },
    );
  }
}
