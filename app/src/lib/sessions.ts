import fs from "node:fs/promises";
import path from "node:path";
import os from "node:os";

/**
 * Centinel session viewer — read Hermes session JSONs off disk and serve them
 * to the web UI so operators can debug cron runs without shelling into the box.
 *
 * Hermes writes one JSON per agent session to:
 *   ~/.hermes/sessions/                     (default profile)
 *   ~/.hermes/profiles/<profile>/sessions/  (named profiles)
 *
 * Cron-launched sessions are named:
 *   session_cron_<job_id>_<YYYYMMDD>_<HHMMSS>.json
 * which lets us filter by job_id (which we know from a Centinel investigation's
 * frontmatter `cron_job_id`) without parsing every file.
 */

const HERMES_HOME = process.env.HERMES_HOME
  ? path.resolve(process.env.HERMES_HOME)
  : path.join(os.homedir(), ".hermes");

export const KNOWN_PROFILES = [
  "default",
  "investigator",
  "watch-runner",
  "data-reporter",
  "archivist",
] as const;

export type Profile = (typeof KNOWN_PROFILES)[number];

function sessionsDirFor(profile: Profile): string {
  if (profile === "default") return path.join(HERMES_HOME, "sessions");
  return path.join(HERMES_HOME, "profiles", profile, "sessions");
}

export interface SessionSummary {
  id: string; // bare session id, no `session_` prefix or `.json` suffix
  profile: Profile;
  /** absolute path to the JSON on disk */
  filePath: string;
  /** parsed from filename: cron job id, or null for non-cron sessions */
  cronJobId: string | null;
  /** parsed from filename for cron sessions */
  startedAt: string | null; // ISO
  /** mtime — useful for sorting */
  mtimeMs: number;
  sizeBytes: number;
  /** loaded lazily; only populated by readSession() */
  messageCount?: number;
  model?: string;
}

export interface SessionMessage {
  index: number;
  role: string;
  /** Stringified content for simple display. For tool_use / tool_result we
   *  pull out the structured fields below. */
  textContent: string;
  toolName?: string;
  toolArgs?: unknown;
  toolResult?: string;
  isToolUse: boolean;
  isToolResult: boolean;
  finishReason?: string;
  reasoning?: string;
}

export interface SessionDoc {
  id: string;
  profile: Profile;
  filePath: string;
  model?: string;
  sessionStart?: string;
  lastUpdated?: string;
  systemPromptPreview?: string;
  messages: SessionMessage[];
}

const FILENAME_RE =
  /^session_(cron_([a-f0-9]+)_(\d{8})_(\d{6})|.+)\.json$/i;

function parseFilename(
  filename: string,
): { id: string; cronJobId: string | null; startedAt: string | null } | null {
  const m = FILENAME_RE.exec(filename);
  if (!m) return null;
  const id = filename.replace(/^session_/, "").replace(/\.json$/, "");
  if (!m[2]) return { id, cronJobId: null, startedAt: null };
  const [, , jobId, ymd, hms] = m;
  const iso = `${ymd.slice(0, 4)}-${ymd.slice(4, 6)}-${ymd.slice(6, 8)}T${hms.slice(0, 2)}:${hms.slice(2, 4)}:${hms.slice(4, 6)}`;
  return { id, cronJobId: jobId, startedAt: new Date(iso).toISOString() };
}

async function listOne(profile: Profile): Promise<SessionSummary[]> {
  const dir = sessionsDirFor(profile);
  let entries: string[];
  try {
    entries = await fs.readdir(dir);
  } catch {
    return [];
  }
  const out: SessionSummary[] = [];
  for (const f of entries) {
    if (!f.startsWith("session_") || !f.endsWith(".json")) continue;
    const parsed = parseFilename(f);
    if (!parsed) continue;
    const filePath = path.join(dir, f);
    let stat;
    try {
      stat = await fs.stat(filePath);
    } catch {
      continue;
    }
    out.push({
      id: parsed.id,
      profile,
      filePath,
      cronJobId: parsed.cronJobId,
      startedAt: parsed.startedAt,
      mtimeMs: stat.mtimeMs,
      sizeBytes: stat.size,
    });
  }
  return out;
}

export interface ListSessionsFilter {
  profile?: Profile | "all";
  cronJobId?: string;
  /** epoch ms — only include sessions whose mtime is after this */
  sinceMs?: number;
  limit?: number;
}

export async function listSessions(
  filter: ListSessionsFilter = {},
): Promise<SessionSummary[]> {
  const which: Profile[] =
    !filter.profile || filter.profile === "all"
      ? [...KNOWN_PROFILES]
      : [filter.profile];

  const arrs = await Promise.all(which.map(listOne));
  let all = arrs.flat();

  if (filter.cronJobId) {
    all = all.filter((s) => s.cronJobId === filter.cronJobId);
  }
  if (typeof filter.sinceMs === "number") {
    all = all.filter((s) => s.mtimeMs >= filter.sinceMs!);
  }

  // Newest first.
  all.sort((a, b) => b.mtimeMs - a.mtimeMs);

  if (filter.limit && filter.limit > 0) {
    all = all.slice(0, filter.limit);
  }
  return all;
}

interface RawMessage {
  role?: string;
  content?:
    | string
    | Array<
        | { type: "text"; text?: string }
        | {
            type: "tool_use";
            id?: string;
            name?: string;
            input?: unknown;
          }
        | {
            type: "tool_result";
            tool_use_id?: string;
            content?: string | Array<{ type: string; text?: string }>;
          }
        | { type: string; [k: string]: unknown }
      >;
  reasoning?: string;
  finish_reason?: string;
}

function summarizeContent(c: RawMessage["content"]): {
  text: string;
  isToolUse: boolean;
  isToolResult: boolean;
  toolName?: string;
  toolArgs?: unknown;
  toolResult?: string;
} {
  if (typeof c === "string") {
    return { text: c, isToolUse: false, isToolResult: false };
  }
  if (!Array.isArray(c)) {
    return { text: "", isToolUse: false, isToolResult: false };
  }
  let text = "";
  let isToolUse = false;
  let isToolResult = false;
  let toolName: string | undefined;
  let toolArgs: unknown;
  let toolResult: string | undefined;

  for (const part of c) {
    if (!part || typeof part !== "object") continue;
    const t = (part as { type?: string }).type;
    if (t === "text") {
      const txt = (part as { text?: string }).text ?? "";
      text += (text ? "\n\n" : "") + txt;
    } else if (t === "tool_use") {
      isToolUse = true;
      toolName = (part as { name?: string }).name;
      toolArgs = (part as { input?: unknown }).input;
    } else if (t === "tool_result") {
      isToolResult = true;
      const inner = (part as { content?: unknown }).content;
      if (typeof inner === "string") {
        toolResult = inner;
      } else if (Array.isArray(inner)) {
        toolResult = inner
          .map((x) => {
            if (x && typeof x === "object" && "text" in x) {
              return (x as { text?: string }).text ?? "";
            }
            return JSON.stringify(x);
          })
          .join("\n");
      } else if (inner != null) {
        toolResult = JSON.stringify(inner);
      }
    }
  }
  return { text, isToolUse, isToolResult, toolName, toolArgs, toolResult };
}

export async function readSession(
  profile: Profile,
  id: string,
): Promise<SessionDoc | null> {
  const dir = sessionsDirFor(profile);
  const filePath = path.join(dir, `session_${id}.json`);
  let raw: string;
  try {
    raw = await fs.readFile(filePath, "utf-8");
  } catch {
    return null;
  }
  let data: {
    session_id?: string;
    model?: string;
    session_start?: string;
    last_updated?: string;
    system_prompt?: string;
    messages?: RawMessage[];
  };
  try {
    data = JSON.parse(raw);
  } catch {
    return null;
  }
  const msgs = (data.messages ?? []).map((m, i): SessionMessage => {
    const sum = summarizeContent(m.content);
    return {
      index: i,
      role: m.role ?? "?",
      textContent: sum.text,
      isToolUse: sum.isToolUse,
      isToolResult: sum.isToolResult,
      toolName: sum.toolName,
      toolArgs: sum.toolArgs,
      toolResult: sum.toolResult,
      finishReason: m.finish_reason,
      reasoning: m.reasoning,
    };
  });

  return {
    id,
    profile,
    filePath,
    model: data.model,
    sessionStart: data.session_start,
    lastUpdated: data.last_updated,
    systemPromptPreview:
      typeof data.system_prompt === "string"
        ? data.system_prompt.slice(0, 500)
        : undefined,
    messages: msgs,
  };
}

/**
 * Find the correct profile + id for a bare session id (where the caller
 * doesn't know which profile holds it). Used by /runs/[id] which routes
 * by id alone to keep URLs short.
 */
export async function locateSession(
  id: string,
): Promise<{ profile: Profile; doc: SessionDoc } | null> {
  for (const profile of KNOWN_PROFILES) {
    const doc = await readSession(profile, id);
    if (doc) return { profile, doc };
  }
  return null;
}
