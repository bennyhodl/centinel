/**
 * Archivist role — pi-agent reimplementation of the `archivist` Hermes
 * profile + `civic-archivist` skill.
 *
 * Owns document intake: every PDF/HTML capture/transcript/image is hashed
 * → vault → OCR'd → indexed → tagged → summarized. Cross-checks
 * names/dates/dollars against the Data Reporter's DB. The unglamorous
 * backbone (Spotlight 2nd-seat Reporter).
 */
import { resolve } from "node:path";
import type { ServerConfig } from "../config.js";
import type { RoleConfig } from "./types.js";
import { centinelCustomTools } from "../runtime/customTools.js";

export function archivistRole(config: ServerConfig): RoleConfig {
	const skillDir = resolve(config.skillsDir, "civic-archivist");
	return {
		name: "archivist",
		skills: [
			{
				name: "civic-archivist",
				description:
					"Document intake. Hash → vault → OCR → index → tag → 1-3 paragraph summary. " +
					"Cross-checks vault content against the entity DB and flags discrepancies. Maintains vault manifest integrity.",
				filePath: resolve(skillDir, "SKILL.md"),
				baseDir: skillDir,
			},
		],
		customTools: centinelCustomTools,
	};
}
