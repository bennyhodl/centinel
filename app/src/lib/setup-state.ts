import fs from "node:fs/promises";
import path from "node:path";
import { z } from "zod";
import { runtimePath, setupStatePath } from "./config";

/**
 * Setup-wizard state — persisted to <wiki>/_runtime/setup-state.json.
 *
 * The wizard is the only thing rendered until status === "complete".
 * After completion, the file remains so the operator can audit/redo setup.
 */

export const SetupStatus = z.enum(["pending", "in_progress", "complete"]);
export type SetupStatus = z.infer<typeof SetupStatus>;

export const SetupStep = z.union([
  z.literal(1),
  z.literal(2),
  z.literal(3),
  z.literal(4),
  z.literal(5),
  z.literal(6),
  z.literal(7),
]);
export type SetupStep = z.infer<typeof SetupStep>;

export const WATCH_PRESETS = [
  {
    id: "errant-spending",
    label: "Errant spending",
    description:
      "Flags procurement awards, no-bid contracts, and budget transfers above thresholds.",
  },
  {
    id: "corruption",
    label: "Corruption signals",
    description:
      "Watches for COI overlaps between contractors, council members, and donors.",
  },
  {
    id: "policy-drift",
    label: "Policy drift",
    description:
      "Detects when ordinances, board appointments, or staffing change without a council vote.",
  },
] as const;

export const NotificationChannel = z.enum(["none", "discord", "telegram"]);

export const SetupState = z.object({
  status: SetupStatus,
  step: SetupStep.default(1),
  cityDomain: z.string().optional(),
  projectName: z.string().optional(),
  watchPresets: z.array(z.string()).default([]),
  notification: z
    .object({
      channel: NotificationChannel.default("none"),
      target: z.string().optional(),
    })
    .optional(),
  bootstrap: z
    .object({
      startedAt: z.string().optional(),
      finishedAt: z.string().optional(),
      logPath: z.string().optional(),
      // STUB: real shell-out writes more here (pages crawled, classified, etc.)
      stubMode: z.boolean().optional(),
    })
    .optional(),
  startedAt: z.string().optional(),
  completedAt: z.string().optional(),
});
export type SetupState = z.infer<typeof SetupState>;

const DEFAULT_STATE: SetupState = {
  status: "pending",
  step: 1,
  watchPresets: [],
};

/**
 * Reads <wiki>/_runtime/setup-state.json. Missing or malformed → pending defaults.
 */
export async function readSetupState(): Promise<SetupState> {
  try {
    const buf = await fs.readFile(setupStatePath(), "utf-8");
    const parsed = SetupState.safeParse(JSON.parse(buf));
    if (parsed.success) return parsed.data;
  } catch {
    // fall through
  }
  return DEFAULT_STATE;
}

export async function writeSetupState(state: SetupState): Promise<void> {
  await fs.mkdir(runtimePath(), { recursive: true });
  const validated = SetupState.parse(state);
  const tmp = setupStatePath() + ".tmp";
  await fs.writeFile(tmp, JSON.stringify(validated, null, 2), "utf-8");
  await fs.rename(tmp, setupStatePath());
}

export async function isSetupComplete(): Promise<boolean> {
  const s = await readSetupState();
  return s.status === "complete";
}

/**
 * Path where bootstrap stub writes its progress log.
 * Real implementation would point at the live Hermes session log.
 */
export function bootstrapLogPath(): string {
  return path.join(runtimePath(), "bootstrap.log");
}
