/**
 * Role registry — maps role name → RoleConfig builder.
 *
 * Phase 2: all five roles wired.
 */
import type { ServerConfig } from "../config.js";
import type { RoleConfig, RoleName } from "./types.js";
import { editorRole } from "./editor.js";
import { investigatorRole } from "./investigator.js";
import { archivistRole } from "./archivist.js";
import { dataReporterRole } from "./dataReporter.js";
import { watchRunnerRole } from "./watchRunner.js";

type RoleBuilder = (config: ServerConfig) => RoleConfig;

const roleBuilders: Record<RoleName, RoleBuilder> = {
	editor: editorRole,
	investigator: investigatorRole,
	archivist: archivistRole,
	"data-reporter": dataReporterRole,
	"watch-runner": watchRunnerRole,
};

export function getRole(config: ServerConfig, name: string): RoleConfig | undefined {
	const builder = roleBuilders[name as RoleName];
	return builder ? builder(config) : undefined;
}

export function listRoleNames(): RoleName[] {
	return Object.keys(roleBuilders) as RoleName[];
}
