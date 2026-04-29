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

export interface InvestigationActionResult {
  ok: true;
  output: string;
}

async function dispatcherCall(
  args: string[],
  slug: string,
): Promise<InvestigationActionResult> {
  const bin = centinelBin();
  try {
    const { stdout, stderr } = await run(bin, args, { timeout: 30_000 });
    revalidatePath(`/investigations/${slug}`);
    revalidatePath("/investigations");
    return { ok: true, output: (stdout || "") + (stderr || "") };
  } catch (err) {
    const e = err as { stdout?: string; stderr?: string; message?: string };
    const detail = (e.stderr || e.stdout || e.message || String(err)).trim();
    throw new Error(detail);
  }
}

export async function pauseInvestigationAction(formData: FormData) {
  const slug = String(formData.get("slug") ?? "").trim();
  if (!slug) throw new Error("slug is required");
  return dispatcherCall(["investigate", "pause", slug], slug);
}

export async function resumeInvestigationAction(formData: FormData) {
  const slug = String(formData.get("slug") ?? "").trim();
  if (!slug) throw new Error("slug is required");
  return dispatcherCall(["investigate", "resume", slug], slug);
}

export async function triggerInvestigationAction(formData: FormData) {
  const slug = String(formData.get("slug") ?? "").trim();
  if (!slug) throw new Error("slug is required");
  return dispatcherCall(["investigate", "trigger", slug], slug);
}
