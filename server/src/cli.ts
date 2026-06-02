#!/usr/bin/env node
/**
 * centinel — the operator-facing CLI.
 *
 * Phase 1 subcommands:
 *   centinel server start            boot centinel-server
 *   centinel server status           probe /health
 *   centinel role <name> -p "..."    run a role through the local server
 *                                    (default: tail SSE events; --no-tail
 *                                    returns JSON summary)
 *   centinel run list                list recent runs
 *   centinel run get <id>            print summary JSON for a run
 *   centinel run tail <id>           SSE-tail a run (live or replay)
 *
 * Future phases add: cron, investigate, doctor.
 * See docs/PI_MIGRATION_PLAN.md.
 */

import { parseArgs } from "node:util";
import { loadConfig, healthUrl, type ServerConfig } from "./config.js";
import { start as startServer } from "./server.js";

interface CommandContext {
	positionals: string[];
	values: Record<string, string | boolean | undefined>;
	config: ServerConfig;
}

type Command = (ctx: CommandContext) => Promise<number>;

const HELP = `centinel \u2014 Centinel runtime CLI (phase 3)

Usage:
  centinel server start
  centinel server status

  centinel role <name> --prompt "..." [--tail | --no-tail] [--source cli|cron|http|delegate]
      run a role end-to-end. Defaults to --tail (stream events to stderr;
      print final assistant text + summary to stdout on completion).
  centinel role <name> --interactive [--prompt "..."]
      open pi's full TUI scoped to <name>'s skill + tools. The session
      persists under .runtime/sessions/<name>/. No server required.

  centinel run list [--limit 50]
  centinel run get <runId>
  centinel run tail <runId>

  centinel cron list
  centinel cron get <name>
  centinel cron fire <name> [--tail | --no-tail] [--force]
  centinel cron pause <name>
  centinel cron resume <name>
  centinel cron pause-all
  centinel cron resume-all

  centinel investigate register <slug> --cron "<expr>" [--prompt "..."] [--role investigator]
  centinel investigate unregister <slug>

  centinel doctor
      run health checks (server, runtime dir, skills, persona, wiki,
      provider key, cron table parses). Exit 0 if all pass, 1 otherwise.

  centinel cron seed-paused
      seed default cron jobs into .runtime/cron.json (paused). Used by
      bootstrap so it can prepare the table without booting the server.

  centinel chat send --message "..." [--session <id>] [--no-tail]
      send a turn to the editor chat. If --session is omitted a new session
      is created and its id printed.
  centinel chat list [--active]
  centinel chat abort <sessionId>

  centinel --help

Environment:
  CENTINEL_HOST                 server bind/connect host (default: 127.0.0.1)
  CENTINEL_PORT                 server bind/connect port (default: 8787)
  CENTINEL_RUNTIME_DIR          where .runtime/{runs,sessions}/ live (default: <repo>/.runtime)
`;

// --- server commands ---

const serverStart: Command = async () => {
	await startServer();
	await new Promise<void>(() => {}); // keep alive
	return 0;
};

const serverStatus: Command = async ({ config }) => {
	try {
		const res = await fetch(healthUrl(config));
		const text = await res.text();
		if (!res.ok) {
			console.error(`[centinel] server responded ${res.status}`);
			console.error(text);
			return 1;
		}
		console.log(safePrettyJson(text));
		return 0;
	} catch (err) {
		const msg = err instanceof Error ? err.message : String(err);
		console.error(`[centinel] could not reach ${healthUrl(config)}: ${msg}`);
		console.error(`[centinel] is the server running? try: centinel server start`);
		return 2;
	}
};

// --- role command ---

const roleRun: Command = async ({ positionals, values, config }) => {
	const roleName = positionals[0];
	if (!roleName) {
		console.error("[centinel] usage: centinel role <name> [--interactive | --prompt \"...\"]");
		return 1;
	}

	if (values.interactive === true) {
		const { runInteractive } = await import("./runtime/interactive.js");
		const { getRole } = await import("./roles/registry.js");
		const { RunStore } = await import("./runtime/runStore.js");
		const role = getRole(config, roleName);
		if (!role) {
			console.error(`[centinel] unknown role: ${roleName}`);
			return 1;
		}
		const initial = typeof values.prompt === "string" && values.prompt.trim() !== "" ? values.prompt : undefined;
		try {
			await runInteractive({
				config,
				role,
				store: new RunStore(config),
				initialMessage: initial,
			});
			return 0;
		} catch (err) {
			const msg = err instanceof Error ? err.message : String(err);
			console.error(`[centinel] interactive session failed: ${msg}`);
			return 1;
		}
	}

	const promptArg = values.prompt;
	let prompt: string;
	if (typeof promptArg === "string" && promptArg.trim() !== "") {
		prompt = promptArg;
	} else if (!process.stdin.isTTY) {
		prompt = await readAllStdin();
		if (!prompt.trim()) {
			console.error("[centinel] no prompt given (use --prompt or pipe via stdin)");
			return 1;
		}
	} else {
		console.error("[centinel] --prompt is required (or pipe text via stdin)");
		return 1;
	}

	const tail = values.tail !== false; // default true
	const source = typeof values.source === "string" ? values.source : "cli";

	const url = `http://${config.host}:${config.port}/run/${encodeURIComponent(roleName)}`;
	const body = JSON.stringify({ prompt, source });

	if (!tail) {
		try {
			const res = await fetch(url, {
				method: "POST",
				headers: { "content-type": "application/json" },
				body,
			});
			const text = await res.text();
			console.log(safePrettyJson(text));
			return res.ok ? 0 : 1;
		} catch (err) {
			console.error(`[centinel] request failed: ${err instanceof Error ? err.message : err}`);
			return 2;
		}
	}

	// Streaming mode.
	try {
		const res = await fetch(url, {
			method: "POST",
			headers: { "content-type": "application/json", accept: "text/event-stream" },
			body,
		});
		if (!res.ok || !res.body) {
			console.error(`[centinel] server returned ${res.status}`);
			console.error(await res.text());
			return 1;
		}
		const summary = await streamSseToStderr(res.body);
		if (summary?.runId) {
			console.error(`[centinel] run complete: ${summary.runId}`);
			// Pull final summary from disk via HTTP for canonical output.
			try {
				const sumRes = await fetch(`http://${config.host}:${config.port}/runs/${summary.runId}`);
				const sumText = await sumRes.text();
				console.log(safePrettyJson(sumText));
				const parsed = JSON.parse(sumText) as { ok?: boolean };
				return parsed.ok === false ? 1 : 0;
			} catch {
				return 0;
			}
		}
		return 0;
	} catch (err) {
		console.error(`[centinel] request failed: ${err instanceof Error ? err.message : err}`);
		return 2;
	}
};

// --- run commands ---

const runList: Command = async ({ values, config }) => {
	const limit = typeof values.limit === "string" ? Number.parseInt(values.limit, 10) : 50;
	const res = await fetch(`http://${config.host}:${config.port}/runs?limit=${limit}`);
	const text = await res.text();
	console.log(safePrettyJson(text));
	return res.ok ? 0 : 1;
};

const runGet: Command = async ({ positionals, config }) => {
	const runId = positionals[0];
	if (!runId) {
		console.error("[centinel] usage: centinel run get <runId>");
		return 1;
	}
	const res = await fetch(`http://${config.host}:${config.port}/runs/${encodeURIComponent(runId)}`);
	const text = await res.text();
	console.log(safePrettyJson(text));
	return res.ok ? 0 : 1;
};

const runTail: Command = async ({ positionals, config }) => {
	const runId = positionals[0];
	if (!runId) {
		console.error("[centinel] usage: centinel run tail <runId>");
		return 1;
	}
	const res = await fetch(`http://${config.host}:${config.port}/runs/${encodeURIComponent(runId)}/events`, {
		headers: { accept: "text/event-stream" },
	});
	if (!res.ok || !res.body) {
		console.error(`[centinel] server returned ${res.status}`);
		console.error(await res.text());
		return 1;
	}
	await streamSseToStderr(res.body);
	return 0;
};

// --- cron commands ---

const cronList: Command = async ({ config }) => {
	const res = await fetch(`http://${config.host}:${config.port}/cron/jobs`);
	const text = await res.text();
	console.log(safePrettyJson(text));
	return res.ok ? 0 : 1;
};

const cronGet: Command = async ({ positionals, config }) => {
	const name = positionals[0];
	if (!name) {
		console.error("[centinel] usage: centinel cron get <name>");
		return 1;
	}
	const res = await fetch(`http://${config.host}:${config.port}/cron/jobs/${encodeURIComponent(name)}`);
	console.log(safePrettyJson(await res.text()));
	return res.ok ? 0 : 1;
};

const cronFire: Command = async ({ positionals, values, config }) => {
	const name = positionals[0];
	if (!name) {
		console.error("[centinel] usage: centinel cron fire <name> [--tail|--no-tail] [--force]");
		return 1;
	}
	const tail = values.tail !== false;
	const force = values.force === true;
	const url = `http://${config.host}:${config.port}/cron/jobs/${encodeURIComponent(name)}/fire`;
	const body = JSON.stringify({ force });

	if (!tail) {
		const res = await fetch(url, {
			method: "POST",
			headers: { "content-type": "application/json" },
			body,
		});
		console.log(safePrettyJson(await res.text()));
		return res.ok ? 0 : 1;
	}

	const res = await fetch(url, {
		method: "POST",
		headers: { "content-type": "application/json", accept: "text/event-stream" },
		body,
	});
	if (!res.ok || !res.body) {
		console.error(`[centinel] server returned ${res.status}`);
		console.error(await res.text());
		return 1;
	}
	const summary = await streamSseToStderr(res.body);
	if (summary?.runId) {
		console.error(`[centinel] cron run complete: ${summary.runId}`);
		const sumRes = await fetch(`http://${config.host}:${config.port}/runs/${summary.runId}`);
		console.log(safePrettyJson(await sumRes.text()));
	}
	return 0;
};

const cronPause: Command = async ({ positionals, config }) => {
	const name = positionals[0];
	if (!name) {
		console.error("[centinel] usage: centinel cron pause <name>");
		return 1;
	}
	const res = await fetch(`http://${config.host}:${config.port}/cron/jobs/${encodeURIComponent(name)}/pause`, { method: "POST" });
	console.log(safePrettyJson(await res.text()));
	return res.ok ? 0 : 1;
};

const cronResume: Command = async ({ positionals, config }) => {
	const name = positionals[0];
	if (!name) {
		console.error("[centinel] usage: centinel cron resume <name>");
		return 1;
	}
	const res = await fetch(`http://${config.host}:${config.port}/cron/jobs/${encodeURIComponent(name)}/resume`, { method: "POST" });
	console.log(safePrettyJson(await res.text()));
	return res.ok ? 0 : 1;
};

const cronPauseAll: Command = async ({ config }) => {
	const res = await fetch(`http://${config.host}:${config.port}/cron/pause-all`, { method: "POST" });
	console.log(safePrettyJson(await res.text()));
	return res.ok ? 0 : 1;
};

const cronResumeAll: Command = async ({ config }) => {
	const res = await fetch(`http://${config.host}:${config.port}/cron/resume-all`, { method: "POST" });
	console.log(safePrettyJson(await res.text()));
	return res.ok ? 0 : 1;
};

// --- investigation commands ---

const investigateRegister: Command = async ({ positionals, values, config }) => {
	const slug = positionals[0];
	if (!slug) {
		console.error(
			'[centinel] usage: centinel investigate register <slug> --cron "<expr>" [--prompt "..."] [--role ...]',
		);
		return 1;
	}
	const cron = values.cron;
	if (typeof cron !== "string" || cron.trim() === "") {
		console.error("[centinel] --cron is required");
		return 1;
	}
	const body: Record<string, unknown> = { cron };
	if (typeof values.prompt === "string") body.prompt = values.prompt;
	if (typeof values.role === "string") body.role = values.role;
	const res = await fetch(
		`http://${config.host}:${config.port}/cron/investigations/${encodeURIComponent(slug)}/register`,
		{ method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) },
	);
	console.log(safePrettyJson(await res.text()));
	return res.ok ? 0 : 1;
};

const investigateUnregister: Command = async ({ positionals, config }) => {
	const slug = positionals[0];
	if (!slug) {
		console.error("[centinel] usage: centinel investigate unregister <slug>");
		return 1;
	}
	const res = await fetch(
		`http://${config.host}:${config.port}/cron/investigations/${encodeURIComponent(slug)}`,
		{ method: "DELETE" },
	);
	console.log(safePrettyJson(await res.text()));
	return res.ok ? 0 : 1;
};

// --- chat commands ---

const chatSend: Command = async ({ values, config }) => {
	const message =
		typeof values.message === "string"
			? values.message
			: typeof values.prompt === "string"
				? values.prompt
				: !process.stdin.isTTY
					? await readAllStdin()
					: "";
	if (!message.trim()) {
		console.error("[centinel] --message is required (or pipe via stdin)");
		return 1;
	}
	const sessionId = typeof values.session === "string" ? values.session : undefined;
	const tail = values.tail !== false;
	const body: Record<string, unknown> = { message };
	if (sessionId) body.sessionId = sessionId;

	const url = `http://${config.host}:${config.port}/chat`;
	if (!tail) {
		const res = await fetch(url, {
			method: "POST",
			headers: { "content-type": "application/json" },
			body: JSON.stringify(body),
		});
		console.log(safePrettyJson(await res.text()));
		return res.ok ? 0 : 1;
	}

	const res = await fetch(url, {
		method: "POST",
		headers: { "content-type": "application/json", accept: "text/event-stream" },
		body: JSON.stringify(body),
	});
	if (!res.ok || !res.body) {
		console.error(`[centinel] server returned ${res.status}`);
		console.error(await res.text());
		return 1;
	}
	await streamSseToStderr(res.body);
	return 0;
};

const chatList: Command = async ({ values, config }) => {
	const path = values.active === true ? "/chat/sessions/active" : "/chat/sessions";
	const res = await fetch(`http://${config.host}:${config.port}${path}`);
	console.log(safePrettyJson(await res.text()));
	return res.ok ? 0 : 1;
};

const chatAbort: Command = async ({ positionals, config }) => {
	const id = positionals[0];
	if (!id) {
		console.error("[centinel] usage: centinel chat abort <sessionId>");
		return 1;
	}
	const res = await fetch(`http://${config.host}:${config.port}/chat/sessions/${encodeURIComponent(id)}/abort`, { method: "POST" });
	console.log(safePrettyJson(await res.text()));
	return res.ok ? 0 : 1;
};

// --- doctor command ---

const doctorRun: Command = async ({ config }) => {
	const { runDoctor, formatDoctorChecks } = await import("./doctor.js");
	const checks = await runDoctor(config);
	console.log(formatDoctorChecks(checks));
	return checks.every((c) => c.ok) ? 0 : 1;
};

// --- cron seed-paused (offline) ---

const cronSeedPaused: Command = async ({ config }) => {
	const { CronTable } = await import("./cron/cronTable.js");
	const { loadDogeConfig } = await import("./dogeConfig.js");
	const table = new CronTable(config);
	table.load();
	const yaml = loadDogeConfig(config);
	const result = table.seedDefaults(yaml.cronScheduleOverrides);
	console.log(JSON.stringify(result, null, 2));
	if (result.unknownOverrideKeys.length > 0) {
		console.error(`[centinel] unknown cron override keys in doge.config.yaml: ${result.unknownOverrideKeys.join(", ")}`);
	}
	return 0;
};

// --- routing ---

const commands: Record<string, Record<string, Command>> = {
	server: { start: serverStart, status: serverStatus },
	role: { __default: roleRun },
	run: { list: runList, get: runGet, tail: runTail },
	cron: {
		list: cronList,
		get: cronGet,
		fire: cronFire,
		pause: cronPause,
		resume: cronResume,
		"pause-all": cronPauseAll,
		"resume-all": cronResumeAll,
		"seed-paused": cronSeedPaused,
	},
	investigate: { register: investigateRegister, unregister: investigateUnregister },
	chat: { send: chatSend, list: chatList, abort: chatAbort },
	doctor: { __default: doctorRun },
};

async function main(): Promise<number> {
	const parsed = parseArgs({
		args: process.argv.slice(2),
		allowPositionals: true,
		options: {
			help: { type: "boolean", short: "h" },
			prompt: { type: "string", short: "p" },
			tail: { type: "boolean", default: true },
			interactive: { type: "boolean", short: "i" },
			force: { type: "boolean" },
			source: { type: "string" },
			limit: { type: "string" },
			cron: { type: "string" },
			role: { type: "string" },
			message: { type: "string", short: "m" },
			session: { type: "string" },
			active: { type: "boolean" },
		},
		strict: false,
	});

	if (parsed.values.help) {
		process.stdout.write(HELP);
		return 0;
	}
	if (parsed.positionals.length === 0) {
		process.stdout.write(HELP);
		return 1;
	}

	const [group, ...rest] = parsed.positionals;
	if (!group) {
		process.stdout.write(HELP);
		return 1;
	}
	const groupCommands = commands[group];
	if (!groupCommands) {
		console.error(`[centinel] unknown command group: ${group}\n`);
		process.stdout.write(HELP);
		return 1;
	}

	// `role` takes the next positional as its target, not as a subcommand.
	let sub: string;
	let positionals: string[];
	if ("__default" in groupCommands) {
		sub = "__default";
		positionals = rest;
	} else {
		const [s, ...r] = rest;
		if (!s) {
			console.error(`[centinel] ${group} expects a subcommand\n`);
			process.stdout.write(HELP);
			return 1;
		}
		sub = s;
		positionals = r;
	}

	const cmd = groupCommands[sub];
	if (!cmd) {
		console.error(`[centinel] unknown subcommand: ${group} ${sub}\n`);
		process.stdout.write(HELP);
		return 1;
	}

	const ctx: CommandContext = {
		positionals,
		values: parsed.values as CommandContext["values"],
		config: loadConfig(),
	};
	return await cmd(ctx);
}

// --- helpers ---

function safePrettyJson(text: string): string {
	try {
		return JSON.stringify(JSON.parse(text), null, 2);
	} catch {
		return text;
	}
}

async function readAllStdin(): Promise<string> {
	const chunks: Buffer[] = [];
	for await (const chunk of process.stdin) {
		chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk as Buffer));
	}
	return Buffer.concat(chunks).toString("utf8");
}

interface SseSummary {
	runId?: string;
}

async function streamSseToStderr(body: ReadableStream<Uint8Array>): Promise<SseSummary | undefined> {
	const reader = body.getReader();
	const decoder = new TextDecoder();
	let buf = "";
	let summary: SseSummary | undefined;
	while (true) {
		const { value, done } = await reader.read();
		if (done) break;
		buf += decoder.decode(value, { stream: true });
		let idx;
		while ((idx = buf.indexOf("\n\n")) !== -1) {
			const frame = buf.slice(0, idx);
			buf = buf.slice(idx + 2);
			const lines = frame.split("\n");
			for (const line of lines) {
				if (!line.startsWith("data:")) continue;
				const data = line.slice(5).trim();
				if (!data) continue;
				try {
					const obj = JSON.parse(data);
					if (obj && typeof obj === "object" && "runId" in obj && typeof obj.runId === "string") {
						summary = { runId: obj.runId };
					}
					// Render compactly: t + event.type + small payload preview.
					process.stderr.write(formatSseEvent(obj) + "\n");
				} catch {
					process.stderr.write(data + "\n");
				}
			}
		}
	}
	return summary;
}

function formatSseEvent(obj: unknown): string {
	if (!obj || typeof obj !== "object") return String(obj);
	const o = obj as { t?: string; event?: { type?: string; [k: string]: unknown }; type?: string };
	if (o.event && typeof o.event === "object") {
		const ev = o.event;
		const t = o.t ? o.t.slice(11, 23) : "";
		const type = ev.type ?? "?";
		const extras = summarizeEvent(ev);
		return `${t} ${type}${extras ? "  " + extras : ""}`;
	}
	if (o.type) return `[${o.type}] ${JSON.stringify(o)}`;
	return JSON.stringify(o);
}

function summarizeEvent(ev: Record<string, unknown>): string {
	const type = ev.type;
	if (type === "tool_execution_start") return `tool=${ev.toolName}`;
	if (type === "tool_execution_end") return `tool=${ev.toolName} ok=${!ev.isError}`;
	if (type === "message_update") {
		const inner = (ev.assistantMessageEvent as { type?: string; delta?: string } | undefined);
		if (inner?.type === "text_delta" && typeof inner.delta === "string") {
			return `text +${inner.delta.length}c`;
		}
		return inner?.type ? `kind=${inner.type}` : "";
	}
	if (type === "_run_announced") return `runId=${ev.runId}`;
	if (type === "_run_end") return "";
	if (type === "_runtime_error") return `error=${ev.error}`;
	return "";
}

main()
	.then((code) => process.exit(code))
	.catch((err: unknown) => {
		console.error("[centinel] fatal:", err);
		process.exit(1);
	});
