/**
 * Minimal HTTP helpers used by the route handlers. Phase 1 stays dep-free
 * (no fastify/express) — we have ~5 routes and a streaming endpoint.
 */
import type { IncomingMessage, ServerResponse } from "node:http";

export async function readJsonBody<T = unknown>(req: IncomingMessage, maxBytes = 1_000_000): Promise<T> {
	const chunks: Buffer[] = [];
	let total = 0;
	for await (const chunk of req) {
		const buf = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk as Buffer);
		total += buf.length;
		if (total > maxBytes) {
			throw new HttpError(413, "request_too_large", `body exceeded ${maxBytes} bytes`);
		}
		chunks.push(buf);
	}
	if (total === 0) return {} as T;
	const text = Buffer.concat(chunks).toString("utf8");
	try {
		return JSON.parse(text) as T;
	} catch (err) {
		throw new HttpError(400, "invalid_json", err instanceof Error ? err.message : String(err));
	}
}

export class HttpError extends Error {
	constructor(
		public status: number,
		public code: string,
		message: string,
	) {
		super(message);
	}
}

export function json(res: ServerResponse, status: number, body: unknown): void {
	const payload = JSON.stringify(body);
	res.writeHead(status, {
		"content-type": "application/json; charset=utf-8",
		"content-length": Buffer.byteLength(payload),
	});
	res.end(payload);
}

export function errorJson(res: ServerResponse, err: unknown): void {
	if (err instanceof HttpError) {
		json(res, err.status, { ok: false, error: err.code, message: err.message });
		return;
	}
	const msg = err instanceof Error ? err.message : String(err);
	json(res, 500, { ok: false, error: "internal_error", message: msg });
}

/**
 * Start a server-sent events stream. Returns a `send` function the caller
 * uses to emit data frames and a `close` function for cleanup.
 */
export function startSse(req: IncomingMessage, res: ServerResponse): {
	send: (data: unknown) => void;
	close: () => void;
	onClose: (cb: () => void) => void;
} {
	res.writeHead(200, {
		"content-type": "text/event-stream; charset=utf-8",
		"cache-control": "no-cache, no-transform",
		connection: "keep-alive",
		"x-accel-buffering": "no",
	});
	res.write(": ok\n\n");

	let closed = false;
	const send = (data: unknown) => {
		if (closed) return;
		const text = typeof data === "string" ? data : JSON.stringify(data);
		res.write(`data: ${text}\n\n`);
	};
	const close = () => {
		if (closed) return;
		closed = true;
		res.end();
	};

	const onClose = (cb: () => void) => {
		req.on("close", () => {
			closed = true;
			cb();
		});
	};

	return { send, close, onClose };
}

/** Returns true if the client prefers SSE. */
export function wantsSse(req: IncomingMessage): boolean {
	const accept = req.headers.accept ?? "";
	return accept.includes("text/event-stream");
}
