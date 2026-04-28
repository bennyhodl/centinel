"use server";

import { spawn } from "node:child_process";
import fs from "node:fs/promises";
import path from "node:path";
import { revalidatePath } from "next/cache";
import { redirect } from "next/navigation";
import {
  bootstrapLogPath,
  readSetupState,
  WATCH_PRESETS,
  writeSetupState,
} from "@/lib/setup-state";

// Resolve the dispatcher binary. The web app runs from `app/`, so the
// repo's `bin/centinel` lives at `../bin/centinel`. CENTINEL_BIN env var
// overrides for non-monorepo deploys.
function centinelBin(): string {
  if (process.env.CENTINEL_BIN) return process.env.CENTINEL_BIN;
  // process.cwd() is `app/` in dev; in standalone build it's the project root.
  // Try both.
  const candidates = [
    path.resolve(process.cwd(), "..", "bin", "centinel"),
    path.resolve(process.cwd(), "bin", "centinel"),
  ];
  return candidates[0]; // Caller checks existence; logs the path on failure.
}

const VALID_PRESET_IDS = new Set(WATCH_PRESETS.map((p) => p.id));

function nowIso(): string {
  return new Date().toISOString();
}

export async function submitStep1(formData: FormData) {
  const raw = String(formData.get("cityDomain") ?? "").trim();
  if (!raw) {
    throw new Error("City domain is required");
  }
  // Normalize: strip protocol + trailing slash
  const cityDomain = raw
    .replace(/^https?:\/\//, "")
    .replace(/\/+$/, "")
    .toLowerCase();

  if (!/^[a-z0-9.-]+\.[a-z]{2,}$/.test(cityDomain)) {
    throw new Error(`Invalid domain: ${raw}`);
  }

  const state = await readSetupState();
  await writeSetupState({
    ...state,
    status: state.status === "pending" ? "in_progress" : state.status,
    cityDomain,
    step: 2,
    startedAt: state.startedAt ?? nowIso(),
  });
  revalidatePath("/setup");
  redirect("/setup");
}

export async function submitStep2(formData: FormData) {
  const projectName = String(formData.get("projectName") ?? "").trim() || "Centinel";
  const state = await readSetupState();
  await writeSetupState({ ...state, projectName, step: 3 });
  revalidatePath("/setup");
  redirect("/setup");
}

export async function submitStep3(formData: FormData) {
  const watchPresets = WATCH_PRESETS
    .filter((p) => formData.get(`preset:${p.id}`) === "on")
    .map((p) => p.id);
  for (const id of watchPresets) {
    if (!VALID_PRESET_IDS.has(id)) throw new Error(`Unknown preset: ${id}`);
  }
  const state = await readSetupState();
  await writeSetupState({ ...state, watchPresets, step: 4 });
  revalidatePath("/setup");
  redirect("/setup");
}

export async function submitStep4(formData: FormData) {
  const channel = String(formData.get("channel") ?? "none");
  const target = String(formData.get("target") ?? "").trim() || undefined;
  if (channel !== "none" && channel !== "discord" && channel !== "telegram") {
    throw new Error(`Unknown channel: ${channel}`);
  }
  const state = await readSetupState();
  await writeSetupState({
    ...state,
    notification: { channel, target },
    step: 5,
  });
  revalidatePath("/setup");
  redirect("/setup");
}

/**
 * Kick off the sitemap bootstrap.
 *
 * Spawns `bin/centinel bootstrap-sitemap <domain>` detached, captures its
 * stdout/stderr to the log file, and immediately returns. The wizard advances
 * to Step 6 right away — Step 6 polls/streams the log file via SSE
 * (`/api/setup/bootstrap-log`) so the operator sees live progress.
 *
 * The bootstrap finishing (or crashing) is reflected by the dispatcher
 * exiting; we mark `finishedAt` when that happens. The web app is allowed
 * to crash mid-bootstrap — the dispatcher keeps running because it's
 * detached from the parent process.
 */
export async function startBootstrap() {
  const state = await readSetupState();
  if (!state.cityDomain) throw new Error("Cannot bootstrap without a city domain");

  const startedAt = nowIso();
  const logPath = bootstrapLogPath();
  await fs.mkdir(path.dirname(logPath), { recursive: true });

  const bin = centinelBin();

  // Open the log file for append; pipe child stdout/stderr into it.
  const logFh = await fs.open(logPath, "w");
  await logFh.write(`[${startedAt}] centinel bootstrap-sitemap ${state.cityDomain}\n`);
  await logFh.write(`[${startedAt}] dispatcher: ${bin}\n`);
  await logFh.write(`[${startedAt}] ─────────────────────────────────────────\n`);

  try {
    const child = spawn(bin, ["bootstrap-sitemap", state.cityDomain], {
      detached: true,
      stdio: ["ignore", logFh.fd, logFh.fd],
      env: { ...process.env },
    });
    // Detach so the dispatcher survives the web request lifecycle.
    child.unref();
    // Close our handle in the parent — the child has its own fd now.
    await logFh.close();
  } catch (err) {
    await logFh.write(
      `[${nowIso()}] ❌ Failed to spawn dispatcher: ${err instanceof Error ? err.message : String(err)}\n`,
    );
    await logFh.close();
    throw err;
  }

  await writeSetupState({
    ...state,
    step: 6,
    bootstrap: {
      startedAt,
      logPath,
      stubMode: false,
    },
  });
  revalidatePath("/setup");
  redirect("/setup");
}

export async function continueToActivation() {
  const state = await readSetupState();
  await writeSetupState({ ...state, step: 7 });
  revalidatePath("/setup");
  redirect("/setup");
}

/**
 * Wizard Step 7. Activates cron by calling `centinel cron resume-all`,
 * then marks setup complete.
 *
 * Synchronous — `centinel cron resume-all` returns quickly (just flips
 * paused → active for already-registered jobs). If it fails, we surface
 * the error to the operator via the wizard rather than silently completing.
 */
export async function completeSetup() {
  const state = await readSetupState();
  const bin = centinelBin();

  // Run `centinel cron resume-all` and capture output for diagnostics.
  // Failures here mean cron didn't activate — surface them.
  const { execFile } = await import("node:child_process");
  const { promisify } = await import("node:util");
  const run = promisify(execFile);
  let cronResumeOutput = "";
  let cronResumeError: string | null = null;
  try {
    const result = await run(bin, ["cron", "resume-all"], { timeout: 30_000 });
    cronResumeOutput = (result.stdout || "") + (result.stderr || "");
  } catch (err) {
    const e = err as { stdout?: string; stderr?: string; message?: string };
    cronResumeOutput = (e.stdout || "") + (e.stderr || "");
    cronResumeError = e.message ?? String(err);
  }

  await writeSetupState({
    ...state,
    status: cronResumeError ? state.status : "complete",
    completedAt: cronResumeError ? state.completedAt : nowIso(),
    activation: {
      attemptedAt: nowIso(),
      output: cronResumeOutput,
      error: cronResumeError,
    },
  });
  revalidatePath("/setup");
  revalidatePath("/");
  if (cronResumeError) {
    // Stay on /setup so the operator can see what failed.
    redirect("/setup");
  }
  redirect("/sitemap");
}

export async function jumpToStep(formData: FormData) {
  const target = Number(formData.get("step"));
  if (!Number.isInteger(target) || target < 1 || target > 7) {
    throw new Error(`Invalid step: ${target}`);
  }
  const state = await readSetupState();
  // Only allow jumping back to a step <= current step.
  if (target > state.step) return;
  await writeSetupState({ ...state, step: target as 1 | 2 | 3 | 4 | 5 | 6 | 7 });
  revalidatePath("/setup");
  redirect("/setup");
}

export async function resetSetup() {
  await writeSetupState({
    status: "pending",
    step: 1,
    watchPresets: [],
  });
  revalidatePath("/setup");
  revalidatePath("/");
  redirect("/setup");
}
