/**
 * Investigator role — pi-agent reimplementation of the Hermes
 * `investigator` profile + `civic-investigator` skill.
 *
 * Phase 1: loaded with only the civic-investigator SKILL.md and a stubbed
 * set of Centinel-specific custom tools. Default pi coding tools (read,
 * bash, edit, write) are added automatically by createAgentSession.
 *
 * Side-by-side acceptance: an operator runs the same prompt through
 * `bin/centinel-investigator` (Hermes) and `bin/centinel role investigator
 * --prompt ...` (pi) and diffs the resulting wiki/findings artifacts.
 */
import { resolve } from "node:path";
import type { ServerConfig } from "../config.js";
import type { RoleConfig } from "./types.js";
import { centinelCustomTools } from "../runtime/customTools.js";

export function investigatorRole(config: ServerConfig): RoleConfig {
	const skillDir = resolve(config.skillsDir, "civic-investigator");
	return {
		name: "investigator",
		skills: [
			{
				name: "civic-investigator",
				// Mirrors the frontmatter in skills/civic-investigator/SKILL.md.
				description:
					"Run an operator-defined civic investigation end-to-end. Read an investigation YAML, depth-crawl " +
					"public .gov seeds, extract entities into the wiki, accumulate cited evidence, and emit candidate " +
					"connection findings into Findings/draft/ for human review.",
				filePath: resolve(skillDir, "SKILL.md"),
				baseDir: skillDir,
			},
		],
		customTools: centinelCustomTools,
	};
}
