import { NextResponse } from "next/server";
import path from "node:path";
import os from "node:os";
import fs from "node:fs";
import Database from "better-sqlite3";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

/**
 * Read prior turns of the persistent /chat session straight from Hermes'
 * SQLite state DB. Returns messages in display form:
 *
 *   { role: "user" | "assistant", content: string }[]
 *
 * Tool-call assistant messages and tool-result messages are summarized
 * inline rather than expanded — the UI showed them live; we don't need to
 * re-render them in full on reload.
 *
 * Mirrors the contract: chat sessions live in state.db with id
 * "centinel-web-chat" (see `/chat/api` POST handler — same SESSION_ID).
 */

const DEFAULT_SESSION_ID = "centinel-web-chat";
const DB_PATH =
  process.env.HERMES_STATE_DB ||
  path.join(os.homedir(), ".hermes", "state.db");

interface RawMessage {
  role: string;
  content: string | null;
  tool_calls: string | null;
  tool_name: string | null;
}

export async function GET(req: Request) {
  const url = new URL(req.url);
  const requested = (url.searchParams.get("sessionId") || "").trim();
  const sessionId =
    requested && requested.length <= 128 ? requested : DEFAULT_SESSION_ID;

  if (!fs.existsSync(DB_PATH)) {
    return NextResponse.json({ messages: [], sessionId, reason: "db_missing" });
  }

  let db: Database.Database;
  try {
    db = new Database(DB_PATH, { readonly: true, fileMustExist: true });
  } catch (e) {
    return NextResponse.json(
      { messages: [], error: "db_open_failed", detail: String(e) },
      { status: 500 },
    );
  }

  let rows: RawMessage[] = [];
  try {
    rows = db
      .prepare(
        `SELECT role, content, tool_calls, tool_name
           FROM messages
          WHERE session_id = ?
          ORDER BY timestamp, id`,
      )
      .all(sessionId) as RawMessage[];
  } catch (e) {
    db.close();
    return NextResponse.json(
      { messages: [], error: "query_failed", detail: String(e) },
      { status: 500 },
    );
  } finally {
    try {
      db.close();
    } catch {
      /* ignore */
    }
  }

  const out: { role: "user" | "assistant"; content: string }[] = [];
  for (const r of rows) {
    if (r.role === "system") continue; // skip system prompts
    if (r.role === "tool") continue; // tool outputs were ephemeral live UI
    if (r.role === "user") {
      const text = (r.content ?? "").trim();
      if (text) out.push({ role: "user", content: text });
      continue;
    }
    if (r.role === "assistant") {
      const text = (r.content ?? "").trim();
      // If the assistant message was a pure tool call (no content), drop it
      // — the resolution after the tool returns will become its own
      // assistant turn with the actual narrative content.
      if (text) out.push({ role: "assistant", content: text });
    }
  }

  return NextResponse.json({ messages: out });
}
