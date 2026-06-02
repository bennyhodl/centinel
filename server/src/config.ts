/**
 * Centinel runtime server configuration.
 *
 * Phase 0: only the bare minimum (host, port, repo root) is wired.
 * Future phases will load doge.config.yaml and per-role overrides here.
 */

import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

export interface ServerConfig {
	host: string;
	port: number;
	repoRoot: string;
	/** Absolute path to <repoRoot>/.runtime (or $CENTINEL_RUNTIME_DIR). Holds per-run logs and sessions. */
	runtimeDir: string;
	/** Absolute path to <repoRoot>/skills. */
	skillsDir: string;
}

const DEFAULT_HOST = "127.0.0.1";
const DEFAULT_PORT = 8787;

export function loadConfig(): ServerConfig {
	const host = process.env.CENTINEL_HOST ?? DEFAULT_HOST;
	const portRaw = process.env.CENTINEL_PORT;
	const port = portRaw ? Number.parseInt(portRaw, 10) : DEFAULT_PORT;
	if (Number.isNaN(port) || port <= 0 || port > 65535) {
		throw new Error(`Invalid CENTINEL_PORT: ${portRaw}`);
	}

	// dist/config.js lives at <repo>/server/dist/config.js;
	// walk up two dirs to land on the repo root.
	const here = dirname(fileURLToPath(import.meta.url));
	const repoRoot = resolve(here, "..", "..");

	const runtimeDir = process.env.CENTINEL_RUNTIME_DIR ?? resolve(repoRoot, ".runtime");
	const skillsDir = resolve(repoRoot, "skills");

	return { host, port, repoRoot, runtimeDir, skillsDir };
}

/**
 * Resolve the EDITOR_PERSONA.md path. Honors $CENTINEL_EDITOR_PERSONA_PATH
 * (matching the Next.js app's env var) so operators can point at
 * `~/plans/centinel/EDITOR_PERSONA.md` or any other location. Falls back
 * to the in-repo locked copy at `docs/EDITOR_PERSONA.md`.
 */
export function editorPersonaPath(config: ServerConfig): string {
	if (process.env.CENTINEL_EDITOR_PERSONA_PATH) return process.env.CENTINEL_EDITOR_PERSONA_PATH;
	return resolve(config.repoRoot, "docs", "EDITOR_PERSONA.md");
}

export function chatSessionsDir(config: ServerConfig): string {
	return resolve(config.runtimeDir, "sessions", "editor-chat");
}

export function runsDir(config: ServerConfig): string {
	return resolve(config.runtimeDir, "runs");
}

export function sessionsDir(config: ServerConfig, role: string): string {
	return resolve(config.runtimeDir, "sessions", role);
}

export function healthUrl(config: ServerConfig): string {
	return `http://${config.host}:${config.port}/health`;
}
