/**
 * Data Reporter role — pi-agent reimplementation of the `data-reporter`
 * Hermes profile + `civic-data-reporter` skill.
 *
 * Owns the entity database. Imports new records, normalizes names, dedups
 * entities, runs operator queries, builds the daily summary stat, backs up
 * weekly.
 */
import { resolve } from "node:path";
import type { ServerConfig } from "../config.js";
import type { RoleConfig } from "./types.js";
import { centinelCustomTools } from "../runtime/customTools.js";

export function dataReporterRole(config: ServerConfig): RoleConfig {
	const skillDir = resolve(config.skillsDir, "civic-data-reporter");
	return {
		name: "data-reporter",
		skills: [
			{
				name: "civic-data-reporter",
				description:
					"Owner of the entity DB (SQLite + Datasette). Imports new records, normalizes aliases, " +
					"dedups entities, runs ad-hoc operator queries, documents methodology, weekly backups.",
				filePath: resolve(skillDir, "SKILL.md"),
				baseDir: skillDir,
			},
		],
		customTools: centinelCustomTools,
	};
}
