/**
 * runRole() — the single entry point for executing any Centinel role.
 *
 * Cron, HTTP, the editor's `delegate` tool, and the CLI all funnel through
 * this function. There is no parallel code path. Everything that runs
 * produces a `.runtime/runs/<id>.{jsonl,json}` pair plus a pi session file.
 */
import { randomUUID } from "node:crypto";
import { mkdirSync } from "node:fs";
import { resolve } from "node:path";
import {
	createAgentSession,
	createSyntheticSourceInfo,
	DefaultResourceLoader,
	getAgentDir,
	SessionManager,
	type AgentSessionEvent,
	type Skill,
	type ToolDefinition,
} from "@mariozechner/pi-coding-agent";
import type { ServerConfig } from "../config.js";
import { runsDir, sessionsDir } from "../config.js";
import type { RoleConfig, RunInput, RunResult } from "../roles/types.js";
import { RunLogger } from "./runLogger.js";
import type { RunStore } from "./runStore.js";

export interface RunRoleDeps {
	config: ServerConfig;
	store: RunStore;
}

/**
 * Convert a Centinel SkillSpec → a pi Skill object so we can hand it to
 * DefaultResourceLoader via skillsOverride.
 */
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

export async function runRole(
	deps: RunRoleDeps,
	role: RoleConfig,
	input: RunInput,
): Promise<RunResult> {
	const runId = input.runId ?? randomUUID();
	const startedAt = new Date();
	const logFile = resolve(runsDir(deps.config), `${runId}.jsonl`);
	const summaryFile = resolve(runsDir(deps.config), `${runId}.json`);

	mkdirSync(sessionsDir(deps.config, role.name), { recursive: true });

	const logger = new RunLogger({ runId, logFile, summaryFile });
	logger.logMeta("run_start", {
		runId,
		role: role.name,
		source: input.source,
		prompt: input.prompt,
		context: input.context,
		startedAt: startedAt.toISOString(),
	});

	// Register so SSE subscribers can tail this run live.
	deps.store.registerActive({
		runId,
		role: role.name,
		source: input.source,
		startedAt: startedAt.toISOString(),
		prompt: input.prompt,
	});

	const piSkills: Skill[] = role.skills.map((s) => toPiSkill(s, role.name));

	// Per-role scoping: replace whatever was auto-discovered with only this
	// role's skills. We don't want the Archivist to inherit the
	// Investigator's SKILL.md.
	const loader = new DefaultResourceLoader({
		cwd: deps.config.repoRoot,
		agentDir: getAgentDir(),
		skillsOverride: (current) => ({
			skills: piSkills,
			diagnostics: current.diagnostics,
		}),
		...(role.systemPromptOverride
			? { systemPromptOverride: (_base: string | undefined) => role.systemPromptOverride!() }
			: {}),
		...(role.appendSystemPrompt
			? { appendSystemPromptOverride: (base: string[]) => [...base, role.appendSystemPrompt!()] }
			: {}),
	});
	await loader.reload();

	const builtTools = role.customToolsBuilder ? role.customToolsBuilder({ config: deps.config, store: deps.store }) : [];
	const customTools: ToolDefinition[] = [...role.customTools, ...builtTools];

	const toolCalls: Array<{ tool: string; ok: boolean }> = [];
	let finalText = "";

	const result: RunResult = {
		runId,
		role: role.name,
		source: input.source,
		ok: false,
		finalText: "",
		toolCalls,
		startedAt: startedAt.toISOString(),
		endedAt: startedAt.toISOString(),
		durationMs: 0,
		sessionFile: undefined,
		logFile,
		summaryFile,
	};

	try {
		const { session } = await createAgentSession({
			cwd: deps.config.repoRoot,
			resourceLoader: loader,
			customTools,
			sessionManager: SessionManager.create(sessionsDir(deps.config, role.name)),
		});

		result.sessionFile = session.sessionFile;

		const unsubscribe = session.subscribe((event: AgentSessionEvent) => {
			const logged = { t: new Date().toISOString(), event };
			logger.logEvent(event);
			deps.store.publish(runId, logged);

			// Lightweight tool-call + final-text bookkeeping.
			if (event.type === "tool_execution_end") {
				toolCalls.push({
					tool: event.toolName,
					ok: !event.isError,
				});
			}
			if (event.type === "turn_end" && event.message?.role === "assistant") {
				const text = extractAssistantText(event.message);
				if (text) finalText = text;
			}
		});

		try {
			await session.prompt(input.prompt);
		} finally {
			unsubscribe();
			session.dispose();
		}

		result.ok = true;
		result.finalText = finalText;
	} catch (err) {
		const msg = err instanceof Error ? err.message : String(err);
		result.ok = false;
		result.errorMessage = msg;
		logger.logMeta("run_error", { error: msg, stack: err instanceof Error ? err.stack : undefined });
	} finally {
		const endedAt = new Date();
		result.endedAt = endedAt.toISOString();
		result.durationMs = endedAt.getTime() - startedAt.getTime();
		logger.logMeta("run_end", { ok: result.ok, durationMs: result.durationMs });
		logger.writeSummary(result);
		await logger.close();
		deps.store.completeActive(runId);
	}

	return result;
}

/**
 * Best-effort extraction of the assistant's final text. pi messages have a
 * `content` array of text/thinking/tool_use parts; we pick up any text
 * parts and join them.
 */
function extractAssistantText(message: unknown): string {
	if (!message || typeof message !== "object") return "";
	const content = (message as { content?: unknown }).content;
	if (!Array.isArray(content)) return "";
	const parts: string[] = [];
	for (const part of content) {
		if (part && typeof part === "object") {
			const p = part as { type?: string; text?: string };
			if (p.type === "text" && typeof p.text === "string") parts.push(p.text);
		}
	}
	return parts.join("\n").trim();
}
