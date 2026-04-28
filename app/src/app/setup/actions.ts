"use server";

import fs from "node:fs/promises";
import { revalidatePath } from "next/cache";
import { redirect } from "next/navigation";
import {
  bootstrapLogPath,
  readSetupState,
  WATCH_PRESETS,
  writeSetupState,
} from "@/lib/setup-state";

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
  const projectName = String(formData.get("projectName") ?? "").trim() || "Tampa-DOGE";
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

export async function startBootstrap() {
  const state = await readSetupState();
  if (!state.cityDomain) throw new Error("Cannot bootstrap without a city domain");

  const startedAt = nowIso();
  const logPath = bootstrapLogPath();

  // STUB: Real implementation shells out to:
  //   hermes session run sitemap-builder --mode bootstrap --target <domain>
  // and tails the log to the browser via SSE. For now we synthesize a log file
  // so the wizard's progress UI has something to render and the state machine
  // exercises end-to-end.
  await fs.mkdir(logPath.split("/").slice(0, -1).join("/") || "/", {
    recursive: true,
  });
  const stubLog = [
    `[${startedAt}] STUB BOOTSTRAP — ${state.cityDomain}`,
    `[${startedAt}] (real run would invoke sitemap-builder skill)`,
    `[${startedAt}] fetching sitemap.xml…`,
    `[${startedAt}] queued 0 seed URLs`,
    `[${startedAt}] bootstrap stub complete — replace with real shell-out`,
  ].join("\n");
  await fs.writeFile(logPath, stubLog, "utf-8");

  await writeSetupState({
    ...state,
    step: 6,
    bootstrap: {
      startedAt,
      finishedAt: nowIso(),
      logPath,
      stubMode: true,
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

export async function completeSetup() {
  const state = await readSetupState();
  await writeSetupState({
    ...state,
    status: "complete",
    completedAt: nowIso(),
  });
  revalidatePath("/setup");
  revalidatePath("/");
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
