/**
 * Interactive runtime — opens pi's full TUI scoped to a Centinel role.
 *
 * Replaces the deleted `bin/centinel-<role>` Hermes shims. Builds an
 * `AgentSessionRuntime` whose resource loader only exposes the role's skill
 * and whose tool set matches the role's cron/HTTP path, then hands it to
 * pi's `InteractiveMode`.
 *
 * See docs/PI_MIGRATION_PLAN.md (Phase 4 / Task 2).
 */
import { mkdirSync } from "node:fs";
import {
	AgentSessionRuntime,
	createAgentSessionFromServices,
	createAgentSessionServices,
	createAgentSessionRuntime,
	createSyntheticSourceInfo,
	getAgentDir,
	InteractiveMode,
	SessionManager,
	type CreateAgentSessionRuntimeFactory,
	type Skill,
	type ToolDefinition,
} from "@mariozechner/pi-coding-agent";
import type { ServerConfig } from "../config.js";
import { sessionsDir } from "../config.js";
import type { RoleConfig } from "../roles/types.js";
import type { RunStore } from "./runStore.js";

export interface RunInteractiveOptions {
	config: ServerConfig;
	role: RoleConfig;
	store: RunStore;
	initialMessage?: string;
}

function toPiSkill(spec: RoleConfig["skills"][number], roleName: string): Skill {
	return {
		name: spec.name,
		description: spec.description,
		filePath: spec.filePath,
		baseDir: spec.baseDir,
		sourceInfo: createSyntheticSourceInfo(spec.filePath, {
			source: `centinel:role:${roleName}`,
			baseDir: spec.baseDir,
		}),
		disableModelInvocation: false,
	};
}

/**
 * Boot pi's TUI scoped to `role`. Blocks until the user exits the session.
 */
export async function runInteractive(opts: RunInteractiveOptions): Promise<void> {
	const { config, role, store } = opts;

	const sessionDir = sessionsDir(config, role.name);
	mkdirSync(sessionDir, { recursive: true });

	const piSkills: Skill[] = role.skills.map((s) => toPiSkill(s, role.name));

	const builtTools = role.customToolsBuilder ? role.customToolsBuilder({ config, store }) : [];
	const customTools: ToolDefinition[] = [...role.customTools, ...builtTools];

	const sessionManager = SessionManager.create(sessionDir);

	const createRuntime: CreateAgentSessionRuntimeFactory = async ({ cwd, agentDir, sessionManager: sm, sessionStartEvent }) => {
		const services = await createAgentSessionServices({
			cwd,
			agentDir,
			resourceLoaderOptions: {
				skillsOverride: (base) => ({
					skills: piSkills,
					diagnostics: base.diagnostics,
				}),
				...(role.systemPromptOverride
					? { systemPromptOverride: () => role.systemPromptOverride!() }
					: {}),
				...(role.appendSystemPrompt
					? { appendSystemPromptOverride: (base: string[]) => [...base, role.appendSystemPrompt!()] }
					: {}),
			},
		});

		const created = await createAgentSessionFromServices({
			services,
			sessionManager: sm,
			sessionStartEvent,
			customTools,
		});

		return {
			...created,
			services,
			diagnostics: [...services.diagnostics],
		};
	};

	const runtime: AgentSessionRuntime = await createAgentSessionRuntime(createRuntime, {
		cwd: config.repoRoot,
		agentDir: getAgentDir(),
		sessionManager,
	});

	const interactive = new InteractiveMode(runtime, {
		initialMessage: opts.initialMessage,
	});

	await interactive.init();
	await interactive.run();
}
