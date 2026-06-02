import path from "node:path";
import os from "node:os";

function expandHome(p: string): string {
  if (p.startsWith("~/")) return path.join(os.homedir(), p.slice(2));
  if (p === "~") return os.homedir();
  return p;
}

/**
 * Resolves the path of the user's wiki — root for all markdown reads & DB.
 * Override with CENTINEL_WIKI_PATH; defaults to ~/wiki/Tampa.
 */
export function wikiPath(): string {
  const raw = process.env.CENTINEL_WIKI_PATH ?? "~/wiki/Tampa";
  return path.resolve(expandHome(raw));
}

export function dbPath(): string {
  return path.join(wikiPath(), "_data", "tampa.db");
}

export function runtimePath(): string {
  return path.join(wikiPath(), "_runtime");
}

export function setupStatePath(): string {
  return path.join(runtimePath(), "setup-state.json");
}

export function vaultPath(): string {
  return path.join(wikiPath(), "Vault");
}

export const config = {
  wikiPath,
  dbPath,
  runtimePath,
  setupStatePath,
  vaultPath,
  authPassword: () => process.env.CENTINEL_PASSWORD ?? "",
  /** Centinel runtime server base URL (used by /chat proxy). */
  centinelServerUrl: () =>
    process.env.CENTINEL_SERVER_URL ??
    `http://${process.env.CENTINEL_HOST ?? "127.0.0.1"}:${process.env.CENTINEL_PORT ?? "8787"}`,
};
