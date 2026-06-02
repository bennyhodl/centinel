/**
 * Watch Runner role — pi-agent reimplementation of the `watch-runner`
 * Hermes profile + `civic-watch-runner` skill.
 *
 * Continuous scanning over sitemap diffs and new wiki pages against
 * preset + user-defined watches. Hits land in Findings/raw/. Maps to the
 * Spotlight News Researcher.
 */
import { resolve } from "node:path";
import type { ServerConfig } from "../config.js";
import type { RoleConfig } from "./types.js";
import { centinelCustomTools } from "../runtime/customTools.js";

export function watchRunnerRole(config: ServerConfig): RoleConfig {
	const skillDir = resolve(config.skillsDir, "civic-watch-runner");
	return {
		name: "watch-runner",
		skills: [
			{
				name: "civic-watch-runner",
				description:
					"Runs continuously over sitemap diffs + new wiki pages. Matches against preset watches and " +
					"user-defined YAML watches. Auto-classifies hits as raw-data (publish) or narrative (draft).",
				filePath: resolve(skillDir, "SKILL.md"),
				baseDir: skillDir,
			},
		],
		customTools: centinelCustomTools,
	};
}
