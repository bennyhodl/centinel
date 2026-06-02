/**
 * RunStore — process-wide registry of role runs.
 *
 * - Active runs (currently executing) live in memory so SSE subscribers can
 *   tail events in real time.
 * - Completed runs are read back from disk (`<runtimeDir>/runs/<id>.json`).
 *
 * No persistence beyond what RunLogger writes to disk — restarting the
 * server forgets which runs were "active." That's fine for Phase 1; if a
 * run is in-flight when the server dies it counts as crashed and the
 * summary file simply won't exist.
 */
import { readFileSync, readdirSync, existsSync, createReadStream } from "node:fs";
import { resolve } from "node:path";
import { createInterface } from "node:readline";
import type { ServerConfig } from "../config.js";
import { runsDir } from "../config.js";
import type { RoleName, RunResult, RunSource } from "../roles/types.js";

export interface LoggedEvent {
	t: string;
	event: { type: string; [k: string]: unknown };
}

export interface ActiveRun {
	runId: string;
	role: RoleName;
	source: RunSource;
	startedAt: string;
	prompt: string;
	/** Ring-ish buffer of events for late subscribers (capped to keep memory in check). */
	buffered: LoggedEvent[];
	listeners: Set<RunListener>;
}

export type RunListener = (event: LoggedEvent) => void;

const BUFFER_LIMIT = 1000;

export class RunStore {
	private active = new Map<string, ActiveRun>();
	constructor(private config: ServerConfig) {}

	registerActive(run: Omit<ActiveRun, "buffered" | "listeners">): ActiveRun {
		const full: ActiveRun = {
			...run,
			buffered: [],
			listeners: new Set(),
		};
		this.active.set(run.runId, full);
		return full;
	}

	/** Publish an event to subscribers and into the buffer. */
	publish(runId: string, event: LoggedEvent): void {
		const run = this.active.get(runId);
		if (!run) return;
		run.buffered.push(event);
		if (run.buffered.length > BUFFER_LIMIT) {
			run.buffered.splice(0, run.buffered.length - BUFFER_LIMIT);
		}
		for (const l of run.listeners) {
			try {
				l(event);
			} catch {
				// Listener errors must not crash the agent.
			}
		}
	}

	completeActive(runId: string): void {
		const run = this.active.get(runId);
		if (!run) return;
		// Final sentinel so subscribers know we're done.
		const endEv: LoggedEvent = {
			t: new Date().toISOString(),
			event: { type: "_run_end" },
		};
		for (const l of run.listeners) {
			try {
				l(endEv);
			} catch {
				/* ignore */
			}
		}
		run.listeners.clear();
		this.active.delete(runId);
	}

	getActive(runId: string): ActiveRun | undefined {
		return this.active.get(runId);
	}

	subscribe(runId: string, listener: RunListener): () => void {
		const run = this.active.get(runId);
		if (!run) return () => {};
		run.listeners.add(listener);
		return () => run.listeners.delete(listener);
	}

	/** Return buffered events for an active run, or undefined if not active. */
	getBuffered(runId: string): LoggedEvent[] | undefined {
		return this.active.get(runId)?.buffered.slice();
	}

	listActive(): ActiveRun[] {
		return [...this.active.values()];
	}

	getSummaryFromDisk(runId: string): RunResult | undefined {
		const file = resolve(runsDir(this.config), `${runId}.json`);
		if (!existsSync(file)) return undefined;
		try {
			return JSON.parse(readFileSync(file, "utf8")) as RunResult;
		} catch {
			return undefined;
		}
	}

	getLogFilePath(runId: string): string {
		return resolve(runsDir(this.config), `${runId}.jsonl`);
	}

	async *replayFromDisk(runId: string): AsyncIterableIterator<LoggedEvent> {
		const path = this.getLogFilePath(runId);
		if (!existsSync(path)) return;
		const rl = createInterface({
			input: createReadStream(path, { encoding: "utf8" }),
			crlfDelay: Infinity,
		});
		for await (const line of rl) {
			if (!line.trim()) continue;
			try {
				yield JSON.parse(line) as LoggedEvent;
			} catch {
				// Skip malformed lines.
			}
		}
	}

	/** List recent runs from disk, newest first. */
	listRecent(limit = 50): RunResult[] {
		const dir = runsDir(this.config);
		if (!existsSync(dir)) return [];
		const files = readdirSync(dir)
			.filter((f) => f.endsWith(".json"))
			.map((f) => f.slice(0, -5));
		const results: RunResult[] = [];
		for (const id of files) {
			const summary = this.getSummaryFromDisk(id);
			if (summary) results.push(summary);
		}
		results.sort((a, b) => (a.startedAt < b.startedAt ? 1 : -1));
		return results.slice(0, limit);
	}
}
