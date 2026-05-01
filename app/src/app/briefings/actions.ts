"use server";

import { execFile } from "node:child_process";
import path from "node:path";
import { promisify } from "node:util";
import { revalidatePath } from "next/cache";

const run = promisify(execFile);

function centinelBin(): string {
  if (process.env.CENTINEL_BIN) return process.env.CENTINEL_BIN;
  return path.resolve(process.cwd(), "..", "bin", "centinel");
}

export interface RunNowResult {
  ok: true;
  output: string;
}

/**
 * Generate a weekly briefing now (out-of-schedule). Fire-and-forget on the
 * dispatcher side — Hermes runs the agent in the background. Operator should
 * watch /status for progress; the new briefing appears on this page when the
 * agent finishes writing it to <wiki>/Briefings/.
 */
export async function generateBriefingNowAction(): Promise<RunNowResult> {
  const bin = centinelBin();
  try {
    const { stdout, stderr } = await run(bin, ["briefing", "run-now"], {
      timeout: 30_000,
    });
    revalidatePath("/briefings");
    revalidatePath("/status");
    return { ok: true, output: (stdout || "") + (stderr || "") };
  } catch (err) {
    const e = err as { stdout?: string; stderr?: string; message?: string };
    const detail = (e.stderr || e.stdout || e.message || String(err)).trim();
    throw new Error(detail || "briefing run-now failed");
  }
}
