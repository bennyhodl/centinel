/**
 * RunLogger — append-only JSONL writer for a single role run.
 *
 * Every pi `AgentSessionEvent` we observe gets one line in
 * `<runtimeDir>/runs/<runId>.jsonl`:
 *
 *   {"t":"2026-05-21T17:30:00.123Z","event":{...}}
 *
 * On completion the caller writes a sibling `<runId>.json` with the run
 * summary (RunResult). The two files together are the durable record of
 * the run — replayable, diffable against Hermes runs, and inspectable
 * from the web app.
 */
import { mkdirSync, createWriteStream, writeFileSync, type WriteStream } from "node:fs";
import { dirname } from "node:path";
import type { AgentSessionEvent } from "@mariozechner/pi-coding-agent";
import type { RunResult } from "../roles/types.js";

export interface RunLoggerOptions {
	runId: string;
	logFile: string;
	summaryFile: string;
}

export class RunLogger {
	readonly runId: string;
	readonly logFile: string;
	readonly summaryFile: string;
	private stream: WriteStream;
	private closed = false;

	constructor(opts: RunLoggerOptions) {
		this.runId = opts.runId;
		this.logFile = opts.logFile;
		this.summaryFile = opts.summaryFile;
		mkdirSync(dirname(this.logFile), { recursive: true });
		mkdirSync(dirname(this.summaryFile), { recursive: true });
		this.stream = createWriteStream(this.logFile, { flags: "a", encoding: "utf8" });
	}

	/**
	 * Record one pi event. Best-effort serialization — events that can't be
	 * stringified (cyclic refs, etc.) are logged as `{ ...unserializable }`
	 * so the stream never crashes the agent.
	 */
	logEvent(event: AgentSessionEvent): void {
		if (this.closed) return;
		const line = safeStringify({
			t: new Date().toISOString(),
			event,
		});
		this.stream.write(line + "\n");
	}

	/**
	 * Free-form annotations from runRole itself (start, end, errors that
	 * happen outside the event stream). Type field prefixed with `_` to
	 * distinguish from pi events.
	 */
	logMeta(kind: string, payload: Record<string, unknown> = {}): void {
		if (this.closed) return;
		const line = safeStringify({
			t: new Date().toISOString(),
			event: { type: `_${kind}`, ...payload },
		});
		this.stream.write(line + "\n");
	}

	writeSummary(result: RunResult): void {
		writeFileSync(this.summaryFile, JSON.stringify(result, null, 2) + "\n", "utf8");
	}

	async close(): Promise<void> {
		if (this.closed) return;
		this.closed = true;
		await new Promise<void>((res) => this.stream.end(res));
	}
}

function safeStringify(obj: unknown): string {
	try {
		return JSON.stringify(obj);
	} catch (err) {
		return JSON.stringify({
			t: new Date().toISOString(),
			event: {
				type: "_unserializable",
				error: err instanceof Error ? err.message : String(err),
			},
		});
	}
}
