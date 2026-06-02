/**
 * Centinel role types.
 *
 * A "role" is the pi-agent reimplementation of a Hermes profile. Each role
 * has its own skill, tool set, and (optional) system-prompt override, but
 * every role runs through the same `runRole()` function — cron, HTTP, and
 * CLI all call into the same code path.
 *
 * See docs/PI_MIGRATION_PLAN.md.
 */
import type { ToolDefinition } from "@mariozechner/pi-coding-agent";
import type { ServerConfig } from "../config.js";
import type { RunStore } from "../runtime/runStore.js";

export type RoleName =
	| "editor"
	| "investigator"
	| "archivist"
	| "data-reporter"
	| "watch-runner";

export type RunSource = "cron" | "http" | "delegate" | "cli";

export interface SkillSpec {
	/** Skill name (matches frontmatter `name:`). */
	name: string;
	/** One-line description (matches frontmatter `description:`). */
	description: string;
	/** Absolute path to the SKILL.md file. */
	filePath: string;
	/** Absolute path to the skill's directory (the dir that contains SKILL.md). */
	baseDir: string;
}

/**
 * Deps a role's runtime tool builder gets. Used by tools (like `delegate`)
 * that need to call back into runRole / the run store.
 */
export interface RoleToolBuilderDeps {
	config: ServerConfig;
	store: RunStore;
}

export interface RoleConfig {
	name: RoleName;
	/** Skills to load for this role. Usually one; editor loads two. */
	skills: SkillSpec[];
	/** Static tools beyond the built-in coding tools (read/write/edit/bash). */
	customTools: ToolDefinition[];
	/**
	 * Optional builder for tools that need runtime deps (config, run store,
	 * etc.). Merged with `customTools`. Used by the editor for `delegate`.
	 */
	customToolsBuilder?: (deps: RoleToolBuilderDeps) => ToolDefinition[];
	/**
	 * Optional override that fully replaces pi's default system prompt.
	 * Prefer `appendSystemPrompt` so the default tool/skill block stays in
	 * place.
	 */
	systemPromptOverride?: () => string;
	/**
	 * Optional addition appended to pi's default system prompt (after the
	 * tool/skill block). The editor uses this to inject the persona.
	 */
	appendSystemPrompt?: () => string;
}

export interface RunInput {
	/** The natural-language prompt to send to the role. */
	prompt: string;
	/** Caller tag for the run log. */
	source: RunSource;
	/** Optional explicit run id (default: generated). */
	runId?: string;
	/** Optional caller-supplied structured context (logged into the summary). */
	context?: Record<string, unknown>;
}

export interface RunResult {
	runId: string;
	role: RoleName;
	source: RunSource;
	ok: boolean;
	finalText: string;
	toolCalls: Array<{ tool: string; ok: boolean }>;
	startedAt: string;
	endedAt: string;
	durationMs: number;
	sessionFile: string | undefined;
	logFile: string;
	summaryFile: string;
	errorMessage?: string;
}
