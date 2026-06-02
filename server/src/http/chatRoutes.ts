/**
 * Chat HTTP routes — the editor /chat surface backed by per-thread pi
 * AgentSessions.
 *
 *   POST /chat                        body: { sessionId?, message }
 *                                     SSE stream of editor events.
 *                                     First frame is {_chat_session, sessionId}.
 *                                     Last frame is {_chat_end, ok, finalText}.
 *
 *   GET  /chat/sessions               list on-disk sessions (newest first)
 *   GET  /chat/sessions/active        list in-memory active threads
 *   POST /chat/sessions/:id/abort     abort the current streaming turn
 *
 * The Next.js app proxies its existing text-stream contract to this surface
 * (translates SSE → text deltas) so the chat UI doesn't change.
 */
import type { IncomingMessage, ServerResponse } from "node:http";
import { ChatBusyError, type ChatSessions, type ChatEventEnvelope } from "../chat/chatSessions.js";
import { errorJson, HttpError, json, readJsonBody, startSse } from "./util.js";

export interface ChatRoutesDeps {
	chat: ChatSessions;
}

interface ChatBody {
	sessionId?: unknown;
	message?: unknown;
}

export async function handlePostChat(
	deps: ChatRoutesDeps,
	req: IncomingMessage,
	res: ServerResponse,
): Promise<void> {
	let body: ChatBody;
	try {
		body = await readJsonBody<ChatBody>(req);
	} catch (err) {
		errorJson(res, err);
		return;
	}

	if (typeof body.message !== "string" || body.message.trim() === "") {
		errorJson(res, new HttpError(400, "invalid_message", "`message` must be a non-empty string"));
		return;
	}

	const requestedId = typeof body.sessionId === "string" ? body.sessionId : undefined;

	const acquired = await deps.chat.getOrCreate(requestedId);
	if ("error" in acquired) {
		json(res, 404, { ok: false, error: "unknown_chat_session", sessionId: requestedId });
		return;
	}

	const thread = acquired.thread;
	const sse = startSse(req, res);
	sse.send({ type: "_chat_session", sessionId: thread.id, created: acquired.created });

	// Subscribe before sending so we don't miss any frames.
	const forward = (envelope: ChatEventEnvelope) => sse.send(envelope);
	const unsubscribe = deps.chat.subscribe(thread.id, forward);
	sse.onClose(unsubscribe);

	try {
		const result = await deps.chat.send(thread.id, body.message);
		sse.send({ type: "_chat_end", sessionId: thread.id, ok: result.ok, finalText: result.finalText, errorMessage: result.errorMessage });
	} catch (err) {
		if (err instanceof ChatBusyError) {
			sse.send({ type: "_chat_busy", sessionId: thread.id });
		} else {
			const msg = err instanceof Error ? err.message : String(err);
			sse.send({ type: "_chat_error", sessionId: thread.id, error: msg });
		}
	} finally {
		unsubscribe();
		sse.close();
	}
}

export function handleListChatSessionsOnDisk(
	deps: ChatRoutesDeps,
	_req: IncomingMessage,
	res: ServerResponse,
): void {
	json(res, 200, { ok: true, sessions: deps.chat.listOnDisk() });
}

export function handleListChatSessionsActive(
	deps: ChatRoutesDeps,
	_req: IncomingMessage,
	res: ServerResponse,
): void {
	json(res, 200, { ok: true, active: deps.chat.listInMemory() });
}

export async function handleAbortChatSession(
	deps: ChatRoutesDeps,
	id: string,
	_req: IncomingMessage,
	res: ServerResponse,
): Promise<void> {
	const ok = await deps.chat.abort(id);
	if (!ok) {
		json(res, 404, { ok: false, error: "unknown_chat_session", sessionId: id });
		return;
	}
	json(res, 200, { ok: true, sessionId: id });
}
