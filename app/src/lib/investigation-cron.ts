// Cron status read for a single investigation. Shells out to
// `bin/centinel investigate cron-status <slug>` which prints JSON.

import { execFile } from "node:child_process";
import path from "node:path";
import { promisify } from "node:util";

const run = promisify(execFile);

export interface InvestigationCronStatus {
  slug: string;
  registered: boolean;
  job_id: string | null;
  name: string;
  schedule_word: string | null;
  schedule_cron: string | null;
  active: boolean | null;
  next_run: string | null;
  last_run: string | null;
  error: string | null;
}

function centinelBin(): string {
  if (process.env.CENTINEL_BIN) return process.env.CENTINEL_BIN;
  return path.resolve(process.cwd(), "..", "bin", "centinel");
}

export async function readInvestigationCronStatus(
  slug: string,
): Promise<InvestigationCronStatus> {
  const bin = centinelBin();
  try {
    const { stdout } = await run(bin, ["investigate", "cron-status", slug], {
      timeout: 15_000,
    });
    // Dispatcher writes JSON to stdout; tolerate trailing whitespace/log lines
    // by isolating the last well-formed JSON object.
    const trimmed = stdout.trim();
    const start = trimmed.lastIndexOf("{");
    const end = trimmed.lastIndexOf("}");
    if (start === -1 || end === -1 || end < start) {
      throw new Error("no JSON object in cron-status output");
    }
    return JSON.parse(trimmed.slice(start, end + 1)) as InvestigationCronStatus;
  } catch (err) {
    const e = err as { stdout?: string; stderr?: string; message?: string };
    const detail = (e.stderr || e.stdout || e.message || String(err)).trim();
    return {
      slug,
      registered: false,
      job_id: null,
      name: `centinel-investigation-${slug}`,
      schedule_word: null,
      schedule_cron: null,
      active: null,
      next_run: null,
      last_run: null,
      error: `dispatcher failed: ${detail}`,
    };
  }
}
