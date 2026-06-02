/**
 * `centinel doctor` — diagnostic checks for a configured Centinel install.
 *
 * Pure-function checks that can run from CLI without booting the server.
 * Each check returns `{ name, ok, detail }`; the caller decides how to
 * render and what to exit with.
 *
 * See docs/PHASE_4_PLAN.md (Task 3).
 */
import { accessSync, constants, existsSync, mkdirSync, statSync } from "node:fs";
import { homedir } from "node:os";
import { resolve } from "node:path";
import { Cron } from "croner";
import { editorPersonaPath, healthUrl, runsDir, sessionsDir, type ServerConfig } from "./config.js";
import { CronTable } from "./cron/cronTable.js";
import { loadDogeConfig } from "./dogeConfig.js";
import { listRoleNames } from "./roles/registry.js";

export interface DoctorCheck {
	name: string;
	ok: boolean;
	detail: string;
}

function expandHome(p: string): string {
	if (p.startsWith("~/")) return resolve(homedir(), p.slice(2));
	if (p === "~") return homedir();
	return p;
}

function resolveWikiPath(config: ServerConfig): string | undefined {
	if (process.env.CENTINEL_WIKI_PATH) return resolve(expandHome(process.env.CENTINEL_WIKI_PATH));
	const yaml = loadDogeConfig(config);
	if (yaml.wikiPath) return resolve(expandHome(yaml.wikiPath));
	return undefined;
}

function writable(path: string): boolean {
	try {
		accessSync(path, constants.W_OK);
		return true;
	} catch {
		return false;
	}
}

async function checkServerHealth(config: ServerConfig): Promise<DoctorCheck> {
	const url = healthUrl(config);
	try {
		const res = await fetch(url);
		if (!res.ok) return { name: "server reachable", ok: false, detail: `${url} → HTTP ${res.status}` };
		return { name: "server reachable", ok: true, detail: url };
	} catch (err) {
		const msg = err instanceof Error ? err.message : String(err);
		return { name: "server reachable", ok: false, detail: `${url} → ${msg}` };
	}
}

function checkRuntimeDir(config: ServerConfig): DoctorCheck {
	const dir = config.runtimeDir;
	try {
		mkdirSync(dir, { recursive: true });
		mkdirSync(runsDir(config), { recursive: true });
		// One sessions dir per role is created lazily; just make sure the
		// parent is writable.
		mkdirSync(sessionsDir(config, "editor"), { recursive: true });
	} catch (err) {
		return { name: "runtime dir writable", ok: false, detail: `${dir} → ${err instanceof Error ? err.message : err}` };
	}
	if (!writable(dir)) return { name: "runtime dir writable", ok: false, detail: `${dir} (no write access)` };
	return { name: "runtime dir writable", ok: true, detail: dir };
}

function checkSkills(config: ServerConfig): DoctorCheck[] {
	const checks: DoctorCheck[] = [];
	const skillMap: Record<string, string> = {
		editor: "sitemap-builder",
		investigator: "civic-investigator",
		archivist: "civic-archivist",
		"data-reporter": "civic-data-reporter",
		"watch-runner": "civic-watch-runner",
	};
	for (const role of listRoleNames()) {
		const dir = skillMap[role];
		if (!dir) {
			checks.push({ name: `skill for ${role}`, ok: false, detail: "no skill mapping" });
			continue;
		}
		const file = resolve(config.skillsDir, dir, "SKILL.md");
		const ok = existsSync(file) && statSync(file).isFile();
		checks.push({ name: `skill ${role}`, ok, detail: file });
	}
	return checks;
}

function checkEditorPersona(config: ServerConfig): DoctorCheck {
	const path = editorPersonaPath(config);
	const ok = existsSync(path) && statSync(path).isFile();
	return { name: "editor persona", ok, detail: path };
}

function checkWiki(config: ServerConfig): DoctorCheck {
	const path = resolveWikiPath(config);
	if (!path) return { name: "wiki path", ok: false, detail: "could not resolve (set CENTINEL_WIKI_PATH or `wiki.path` in doge.config.yaml)" };
	if (!existsSync(path)) return { name: "wiki path", ok: false, detail: `${path} (missing)` };
	if (!writable(path)) return { name: "wiki path", ok: false, detail: `${path} (not writable)` };
	return { name: "wiki path", ok: true, detail: path };
}

function checkProviderKey(): DoctorCheck {
	const envKeys = ["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "GROQ_API_KEY", "GOOGLE_API_KEY"];
	for (const k of envKeys) {
		if ((process.env[k] ?? "").trim() !== "") {
			return { name: "provider api key", ok: true, detail: `$${k}` };
		}
	}
	const authFile = resolve(homedir(), ".pi", "agent", "auth.json");
	if (existsSync(authFile)) {
		return { name: "provider api key", ok: true, detail: authFile };
	}
	return {
		name: "provider api key",
		ok: false,
		detail: "no ANTHROPIC_API_KEY / OPENAI_API_KEY and no ~/.pi/agent/auth.json",
	};
}

function checkCronTable(config: ServerConfig): DoctorCheck {
	const table = new CronTable(config);
	table.load();
	const entries = table.all();
	if (entries.length === 0) {
		return { name: "cron table", ok: true, detail: "empty (will be seeded on first server boot or `centinel cron seed-paused`)" };
	}
	const broken: string[] = [];
	for (const entry of entries) {
		try {
			const job = new Cron(entry.cron, { paused: true }, () => {});
			job.stop();
		} catch (err) {
			broken.push(`${entry.name} (${entry.cron}): ${err instanceof Error ? err.message : err}`);
		}
	}
	if (broken.length > 0) return { name: "cron table", ok: false, detail: broken.join("; ") };
	return { name: "cron table", ok: true, detail: `${entries.length} entries parse cleanly` };
}

export async function runDoctor(config: ServerConfig): Promise<DoctorCheck[]> {
	const checks: DoctorCheck[] = [];
	checks.push(await checkServerHealth(config));
	checks.push(checkRuntimeDir(config));
	checks.push(...checkSkills(config));
	checks.push(checkEditorPersona(config));
	checks.push(checkWiki(config));
	checks.push(checkProviderKey());
	checks.push(checkCronTable(config));
	return checks;
}

export function formatDoctorChecks(checks: DoctorCheck[]): string {
	const lines: string[] = [];
	for (const c of checks) {
		const mark = c.ok ? "OK  " : "FAIL";
		lines.push(`[${mark}] ${c.name.padEnd(24)} ${c.detail}`);
	}
	return lines.join("\n");
}
