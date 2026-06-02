/**
 * Editor role — pi-agent reimplementation of the default Hermes profile.
 *
 * Loaded with:
 *   - `sitemap-builder` skill (so the editor owns the sitemap directly)
 *   - the locked EDITOR_PERSONA.md, appended to pi's default system prompt
 *   - the four stub Centinel tools (qmd_search, db_query, vault_put,
 *     web_fetch) shared with every other role
 *   - the `delegate` tool — built at runtime so it can call back into
 *     runRole() for in-process specialist runs
 *
 * The editor is also the runner for the briefings, huddle-rollup, and
 * sitemap-lint cron jobs. The persona injection means even cron-driven
 * editor runs answer in the editor's voice.
 */
import { existsSync, readFileSync, statSync } from "node:fs";
import { resolve } from "node:path";
import type { ServerConfig } from "../config.js";
import { editorPersonaPath } from "../config.js";
import type { RoleConfig } from "./types.js";
import { centinelCustomTools } from "../runtime/customTools.js";
import { buildDelegateTool } from "../runtime/delegate.js";
import { stripFrontmatter } from "@mariozechner/pi-coding-agent";

/** Cache to avoid re-reading the persona on every cron tick / chat turn. */
let personaCache: { path: string; mtimeMs: number; text: string } | null = null;

function loadPersona(path: string): string | undefined {
	if (!existsSync(path)) {
		console.warn(`[centinel] EDITOR_PERSONA.md not found at ${path} — editor will run without persona override.`);
		return undefined;
	}
	try {
		const stat = statSync(path);
		if (personaCache && personaCache.path === path && personaCache.mtimeMs === stat.mtimeMs) {
			return personaCache.text;
		}
		const raw = readFileSync(path, "utf8");
		const stripped = stripFrontmatter(raw).trim();
		personaCache = { path, mtimeMs: stat.mtimeMs, text: stripped };
		return stripped;
	} catch (err) {
		console.warn(`[centinel] failed to read ${path}: ${err instanceof Error ? err.message : err}`);
		return undefined;
	}
}

export function editorRole(config: ServerConfig): RoleConfig {
	const skillDir = resolve(config.skillsDir, "sitemap-builder");
	const personaPath = editorPersonaPath(config);

	return {
		name: "editor",
		skills: [
			{
				name: "sitemap-builder",
				description:
					"Crawl-and-describe the city's .gov surface, classify every URL by type/content kind, suggest parsers, " +
					"and maintain <wiki>/Sitemap/. Owns the editor's mental map of the city.",
				filePath: resolve(skillDir, "SKILL.md"),
				baseDir: skillDir,
			},
		],
		customTools: centinelCustomTools,
		customToolsBuilder: (deps) => [buildDelegateTool(deps)],
		appendSystemPrompt: () => {
			const persona = loadPersona(personaPath);
			if (!persona) return "";
			return [
				"",
				"# Editor persona",
				"",
				"You are the Centinel **Editor** — head of the investigative unit. Speak and act per the persona spec below.",
				"",
				persona,
				"",
				"## Operating notes",
				"",
				"- For depth analysis or fresh ingest, call the `delegate` tool to hand off to a specialist role (investigator, archivist, data-reporter, watch-runner). Do not delegate trivial questions.",
				"- All `delegate` calls show up live in `/status` as nested runs the operator can watch.",
				"- You never publish narratives or contact named subjects. The human operator wears all editorial-authority hats.",
			].join("\n");
		},
	};
}
