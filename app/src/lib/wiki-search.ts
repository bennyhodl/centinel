import { execFile } from "node:child_process";
import { promisify } from "node:util";

const run = promisify(execFile);

export interface QmdHit {
  path: string;          // wiki-relative path returned by QMD
  score: number;         // 0..1
  excerpt: string;       // short snippet
}

/**
 * Call `qmd query` for the given user question and return up to `limit` hits.
 *
 * QMD is the local hybrid (BM25 + vector + rerank) search engine over the
 * wiki. We shell out to the `qmd` CLI rather than reaching into its JS API
 * because QMD ships as a CLI binary and its API surface isn't stable.
 *
 * Output format: we ask QMD for JSON. If the binary is missing or fails,
 * we return [] so the chat still works in a degraded "no retrieval" mode.
 */
export async function qmdQuery(
  question: string,
  limit = 6,
): Promise<QmdHit[]> {
  if (!question.trim()) return [];

  const bin = process.env.QMD_BIN ?? "qmd";
  // `qmd query --json` emits a JSON array of {path, score, excerpt, ...}
  // Older qmd versions used `--format json`; we try the modern flag first
  // then fall back. Both modes share the same wrapper so failures don't
  // crash the chat — we just lose retrieval for that turn.
  const args = ["query", "--json", "--limit", String(limit), question];
  try {
    const { stdout } = await run(bin, args, { timeout: 15_000, maxBuffer: 4 * 1024 * 1024 });
    return parseQmdJson(stdout);
  } catch {
    try {
      const { stdout } = await run(bin, ["query", "--format", "json", "--limit", String(limit), question], {
        timeout: 15_000,
        maxBuffer: 4 * 1024 * 1024,
      });
      return parseQmdJson(stdout);
    } catch {
      return [];
    }
  }
}

function parseQmdJson(stdout: string): QmdHit[] {
  let parsed: unknown;
  try {
    parsed = JSON.parse(stdout);
  } catch {
    return [];
  }
  // Accept either a top-level array or { results: [...] }.
  const arr =
    Array.isArray(parsed)
      ? parsed
      : Array.isArray((parsed as { results?: unknown }).results)
        ? ((parsed as { results: unknown[] }).results)
        : [];

  const out: QmdHit[] = [];
  for (const raw of arr) {
    if (!raw || typeof raw !== "object") continue;
    const r = raw as Record<string, unknown>;
    const path =
      (typeof r.path === "string" && r.path) ||
      (typeof r.file === "string" && r.file) ||
      (typeof r.relPath === "string" && r.relPath) ||
      (typeof r.docid === "string" && r.docid) ||
      "";
    if (!path) continue;
    const score =
      typeof r.score === "number" ? r.score :
      typeof r.relevance === "number" ? r.relevance :
      0;
    const excerpt =
      (typeof r.excerpt === "string" && r.excerpt) ||
      (typeof r.snippet === "string" && r.snippet) ||
      (typeof r.preview === "string" && r.preview) ||
      "";
    out.push({ path, score, excerpt: excerpt.trim() });
  }
  return out;
}

/** Render hits as a markdown context block injected into the system prompt. */
export function renderHitsAsContext(hits: QmdHit[]): string {
  if (hits.length === 0) {
    return "_(QMD returned no hits. If you don't have a source for the answer, say so.)_";
  }
  const lines: string[] = [];
  lines.push("Retrieved context (top QMD hits, freshest from the wiki):");
  lines.push("");
  for (const h of hits) {
    const cite = `[[${h.path}]]`;
    const score = h.score.toFixed(2);
    lines.push(`### ${cite} (score ${score})`);
    if (h.excerpt) lines.push(h.excerpt);
    lines.push("");
  }
  return lines.join("\n");
}
