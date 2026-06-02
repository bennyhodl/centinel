/**
 * ChatSessionManager — owns per-thread editor `AgentSession`s.
 *
 * Phase 3 model: one `AgentSession` per chat thread, keyed by chatSessionId.
 * The session persists to `.runtime/sessions/editor-chat/<id>.jsonl` via pi's
 * native SessionManager, so:
 *   - reloading the page resumes mid-thread
 *   - branching/forking is free
 *   - every chat is a replayable artifact
 *
 * Concurrency policy: one in-flight prompt per chat session. Concurrent
 * requests to the same session get a 409. Different sessions can run in
 * parallel.
 */
import { randomUUID } from "node:crypto";
import { existsSync, mkdirSync, readdirSync, statSync } from "node:fs";
import { basename, resolve } from "node:path";
import {
	createAgentSession,
	createSyntheticSourceInfo,
	DefaultResourceLoader,
	getAgentDir,
	SessionManager,
	type AgentSession,
	type AgentSessionEvent,
	type Skill,
	type ToolDefinition,
} from "@mariozechner/pi-coding-agent";
import type { ServerConfig } from "../config.js";
import { chatSessionsDir } from "../config.js";
import type { RoleConfig } from "../roles/types.js";
import type { RunStore } from "../runtime/runStore.js";

export interface ChatEventEnvelope {
	t: string;
	event: AgentSessionEvent | { type: string; [k: string]: unknown };
}

export type ChatListener = (envelope: ChatEventEnvelope) => void;

interface ChatThread {
	id: string;
	session: AgentSession;
	sessionFile: string;
	listeners: Set<ChatListener>;
	buffered: ChatEventEnvelope[];
	inFlight: boolean;
	createdAt: string;
	lastActivityAt: string;
}

const BUFFER_LIMIT = 200;

export interface ChatSessionsDeps {
	config: ServerConfig;
	store: RunStore;
	editorRole: RoleConfig;
}

export class ChatSessions {
	private threads = new Map<string, ChatThread>();

	constructor(private deps: ChatSessionsDeps) {
		mkdirSync(chatSessionsDir(deps.config), { recursive: true });
	}

	/**
	 * Get an existing chat thread by id, or create a new one. If `id` is
	 * provided but no matching session file exists, returns undefined so
	 * the caller can decide how to respond (we don't fabricate sessions
	 * silently).
	 */
	async getOrCreate(id?: string): Promise<{ thread: ChatThread; created: boolean } | { error: "not_found" }> {
		if (id) {
			const existing = this.threads.get(id);
			if (existing) return { thread: existing, created: false };
			const file = resolve(chatSessionsDir(this.deps.config), `${id}.jsonl`);
			if (!existsSync(file)) return { error: "not_found" };
			const thread = await this.openExisting(id, file);
			return { thread, created: false };
		}
		const newId = randomUUID();
		const thread = await this.createNew(newId);
		return { thread, created: true };
	}

	private buildResourceLoader(): DefaultResourceLoader {
		const role = this.deps.editorRole;
		const piSkills: Skill[] = role.skills.map((spec) => ({
			name: spec.name,
			description: spec.description,
			filePath: spec.filePath,
			baseDir: spec.baseDir,
			sourceInfo: createSyntheticSourceInfo(spec.filePath, {
				source: `centinel:role:${role.name}`,
				baseDir: spec.baseDir,
			}),
			disableModelInvocation: false,
		}));
		return new DefaultResourceLoader({
			cwd: this.deps.config.repoRoot,
			agentDir: getAgentDir(),
			skillsOverride: (current) => ({ skills: piSkills, diagnostics: current.diagnostics }),
			...(role.systemPromptOverride
				? { systemPromptOverride: (_b: string | undefined) => role.systemPromptOverride!() }
				: {}),
			...(role.appendSystemPrompt
				? { appendSystemPromptOverride: (base: string[]) => [...base, role.appendSystemPrompt!()] }
				: {}),
		});
	}

	private buildTools(): ToolDefinition[] {
		const role = this.deps.editorRole;
		const built = role.customToolsBuilder
			? role.customToolsBuilder({ config: this.deps.config, store: this.deps.store })
			: [];
		return [...role.customTools, ...built];
	}

	private async createNew(id: string): Promise<ChatThread> {
		const dir = chatSessionsDir(this.deps.config);
		mkdirSync(dir, { recursive: true });
		const loader = this.buildResourceLoader();
		await loader.reload();

		// SessionManager.create opens a brand-new file in the given dir; we
		// rename via the SessionManager API isn't supported, so we let pi
		// generate the filename and capture it as our canonical id mapping.
		// To stay deterministic with our caller-provided id, we use
		// SessionManager.open with a path we choose.
		const file = resolve(dir, `${id}.jsonl`);
		const sessionManager = SessionManager.open(file);

		const { session } = await createAgentSession({
			cwd: this.deps.config.repoRoot,
			resourceLoader: loader,
			customTools: this.buildTools(),
			sessionManager,
		});

		const thread: ChatThread = {
			id,
			session,
			sessionFile: session.sessionFile ?? file,
			listeners: new Set(),
			buffered: [],
			inFlight: false,
			createdAt: new Date().toISOString(),
			lastActivityAt: new Date().toISOString(),
		};
		this.attach(thread);
		this.threads.set(id, thread);
		return thread;
	}

	private async openExisting(id: string, file: string): Promise<ChatThread> {
		const loader = this.buildResourceLoader();
		await loader.reload();
		const sessionManager = SessionManager.open(file);
		const { session } = await createAgentSession({
			cwd: this.deps.config.repoRoot,
			resourceLoader: loader,
			customTools: this.buildTools(),
			sessionManager,
		});
		const thread: ChatThread = {
			id,
			session,
			sessionFile: session.sessionFile ?? file,
			listeners: new Set(),
			buffered: [],
			inFlight: false,
			createdAt: statSync(file).birthtime.toISOString(),
			lastActivityAt: statSync(file).mtime.toISOString(),
		};
		this.attach(thread);
		this.threads.set(id, thread);
		return thread;
	}

	private attach(thread: ChatThread): void {
		thread.session.subscribe((event) => {
			const envelope: ChatEventEnvelope = { t: new Date().toISOString(), event };
			thread.buffered.push(envelope);
			if (thread.buffered.length > BUFFER_LIMIT) {
				thread.buffered.splice(0, thread.buffered.length - BUFFER_LIMIT);
			}
			thread.lastActivityAt = envelope.t;
			for (const l of thread.listeners) {
				try {
					l(envelope);
				} catch {
					/* listener errors don't crash the agent */
				}
			}
		});
	}

	/** Subscribe to a thread's event stream. Returns unsubscribe fn. */
	subscribe(id: string, listener: ChatListener): () => void {
		const thread = this.threads.get(id);
		if (!thread) return () => {};
		thread.listeners.add(listener);
		return () => thread.listeners.delete(listener);
	}

	getBuffered(id: string): ChatEventEnvelope[] | undefined {
		return this.threads.get(id)?.buffered.slice();
	}

	/**
	 * Send a user message to a thread. Caller is expected to have already
	 * subscribed to events for streaming. Resolves when the assistant turn
	 * (including all tool calls) is fully complete. Returns the assistant's
	 * final text for callers that just want a one-shot result.
	 */
	async send(id: string, message: string): Promise<{ ok: boolean; finalText: string; errorMessage?: string }> {
		const thread = this.threads.get(id);
		if (!thread) throw new Error(`unknown chat session: ${id}`);
		if (thread.inFlight) throw new ChatBusyError(id);
		thread.inFlight = true;
		let finalText = "";
		try {
			// Capture final assistant text from turn_end events.
			const off = thread.session.subscribe((event) => {
				if (event.type === "turn_end" && event.message?.role === "assistant") {
					const t = extractAssistantText(event.message);
					if (t) finalText = t;
				}
			});
			try {
				await thread.session.prompt(message);
			} finally {
				off();
			}
			thread.lastActivityAt = new Date().toISOString();
			return { ok: true, finalText };
		} catch (err) {
			const msg = err instanceof Error ? err.message : String(err);
			return { ok: false, finalText, errorMessage: msg };
		} finally {
			thread.inFlight = false;
		}
	}

	async abort(id: string): Promise<boolean> {
		const thread = this.threads.get(id);
		if (!thread) return false;
		await thread.session.abort();
		return true;
	}

	listInMemory(): Array<{ id: string; createdAt: string; lastActivityAt: string; inFlight: boolean }> {
		return [...this.threads.values()].map((t) => ({
			id: t.id,
			createdAt: t.createdAt,
			lastActivityAt: t.lastActivityAt,
			inFlight: t.inFlight,
		}));
	}

	/** List sessions on disk, newest first. */
	listOnDisk(): Array<{ id: string; createdAt: string; modifiedAt: string }> {
		const dir = chatSessionsDir(this.deps.config);
		if (!existsSync(dir)) return [];
		const files = readdirSync(dir).filter((f) => f.endsWith(".jsonl"));
		const out = files.map((f) => {
			const id = basename(f, ".jsonl");
			const stat = statSync(resolve(dir, f));
			return {
				id,
				createdAt: stat.birthtime.toISOString(),
				modifiedAt: stat.mtime.toISOString(),
			};
		});
		out.sort((a, b) => (a.modifiedAt < b.modifiedAt ? 1 : -1));
		return out;
	}

	async dispose(): Promise<void> {
		for (const t of this.threads.values()) {
			try {
				t.session.dispose();
			} catch {
				/* ignore */
			}
		}
		this.threads.clear();
	}
}

export class ChatBusyError extends Error {
	constructor(public chatSessionId: string) {
		super(`chat session ${chatSessionId} is currently streaming`);
	}
}

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
