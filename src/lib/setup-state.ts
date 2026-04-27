import fs from "node:fs/promises";
import { setupStatePath } from "./config";

export type SetupStatus = "pending" | "in_progress" | "complete";

export interface SetupState {
  status: SetupStatus;
  // Wizard may add more fields over time; we passthrough.
  [key: string]: unknown;
}

/**
 * Reads <wiki>/_runtime/setup-state.json. If the file doesn't exist or
 * can't be parsed, treat as 'pending' — the wizard hasn't run yet.
 */
export async function readSetupState(): Promise<SetupState> {
  try {
    const buf = await fs.readFile(setupStatePath(), "utf-8");
    const parsed = JSON.parse(buf) as Partial<SetupState>;
    const status = parsed.status;
    if (status === "complete" || status === "in_progress" || status === "pending") {
      return { ...parsed, status } as SetupState;
    }
    return { status: "pending" };
  } catch {
    return { status: "pending" };
  }
}

export async function isSetupComplete(): Promise<boolean> {
  const s = await readSetupState();
  return s.status === "complete";
}
