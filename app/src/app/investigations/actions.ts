"use server";

import { execFile } from "node:child_process";
import path from "node:path";
import { promisify } from "node:util";
import { revalidatePath } from "next/cache";
import { redirect } from "next/navigation";
import { z } from "zod";
import {
  createInvestigation,
  type InvestigationSchedule,
} from "@/lib/investigations";

const run = promisify(execFile);

/** Resolve `bin/centinel`. Mirrors setup/actions.ts. */
function centinelBin(): string {
  if (process.env.CENTINEL_BIN) return process.env.CENTINEL_BIN;
  // process.cwd() is `app/` in dev; in standalone build it's the project root.
  return path.resolve(process.cwd(), "..", "bin", "centinel");
}

const InputSchema = z.object({
  title: z
    .string()
    .trim()
    .min(3, "Title must be at least 3 characters")
    .max(200, "Title must be at most 200 characters"),
  goal: z
    .string()
    .trim()
    .min(10, "Goal must be at least 10 characters")
    .max(4000, "Goal must be at most 4000 characters"),
  seedsRaw: z.string(),
  schedule: z.enum(["daily", "weekly", "monthly", "manual"]),
  depth: z.coerce.number().int().min(1).max(5),
});

function parseSeeds(raw: string): string[] {
  const lines = raw
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter(Boolean);
  for (const line of lines) {
    let url: URL;
    try {
      url = new URL(line);
    } catch {
      throw new Error(`Invalid seed URL: ${line}`);
    }
    if (url.protocol !== "http:" && url.protocol !== "https:") {
      throw new Error(`Seed URL must be http(s): ${line}`);
    }
  }
  return lines;
}

export async function createInvestigationAction(formData: FormData) {
  const parsed = InputSchema.parse({
    title: formData.get("title"),
    goal: formData.get("goal"),
    seedsRaw: String(formData.get("seeds") ?? ""),
    schedule: formData.get("schedule"),
    depth: formData.get("depth"),
  });

  const seeds = parseSeeds(parsed.seedsRaw);

  const { slug } = await createInvestigation({
    title: parsed.title,
    goal: parsed.goal,
    seeds,
    schedule: parsed.schedule as InvestigationSchedule,
    depth: parsed.depth,
  });

  // Register cron via the dispatcher. `manual` schedule short-circuits inside
  // the dispatcher (no cron registered). Synchronous — fast call.
  const bin = centinelBin();
  try {
    await run(bin, ["investigate", "register", slug], { timeout: 30_000 });
  } catch (err) {
    // The investigation file is already on disk; surface the cron error but
    // don't roll back the file. Operator can fix profile/cron and re-run
    // `centinel investigate register <slug>` manually.
    const e = err as { stdout?: string; stderr?: string; message?: string };
    const detail = (e.stderr || e.stdout || e.message || String(err)).trim();
    throw new Error(
      `Investigation file written to Investigations/${slug}.md, but cron registration failed:\n${detail}`,
    );
  }

  // Cold-start fix: kick off the first run immediately, in the background.
  // The user shouldn't have to wait for the next scheduled tick to see anything
  // happen. `run-now` is fire-and-forget on the dispatcher side (Popen +
  // start_new_session), so this returns in milliseconds. Best-effort — if it
  // fails we still redirect; the cron will run on schedule.
  try {
    await run(bin, ["investigate", "run-now", slug], { timeout: 15_000 });
  } catch {
    // soft-fail; cron is registered, normal schedule still applies
  }

  revalidatePath("/investigations");
  redirect(`/investigations/${slug}`);
}
