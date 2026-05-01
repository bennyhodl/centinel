import fs from "node:fs/promises";
import path from "node:path";
import crypto from "node:crypto";
import { wikiPath } from "./config";

/**
 * Tavily extract + cache layer.
 *
 * Cached at <wiki>/Sitemap/cache/<sha256(url)>.json. Every refresh appends
 * to a `history` log so we keep prior fetches for diff/audit.
 */

export interface TavilyExtractResult {
  url: string;
  fetched_at: string;
  title?: string;
  raw_content: string;
  history: { fetched_at: string; content_hash: string }[];
}

function cacheDir(): string {
  return path.join(wikiPath(), "Sitemap", "cache");
}

function cachePath(url: string): string {
  const sha = crypto.createHash("sha256").update(url).digest("hex").slice(0, 32);
  return path.join(cacheDir(), `${sha}.json`);
}

function sha8(s: string): string {
  return crypto.createHash("sha256").update(s).digest("hex").slice(0, 16);
}

export async function readCachedExtract(
  url: string,
): Promise<TavilyExtractResult | null> {
  try {
    const raw = await fs.readFile(cachePath(url), "utf-8");
    return JSON.parse(raw) as TavilyExtractResult;
  } catch (err) {
    const e = err as NodeJS.ErrnoException;
    if (e.code === "ENOENT") return null;
    throw err;
  }
}

/**
 * Calls Tavily extract API for `url`, merges into the cache (appending the
 * previous fetch to `history`), writes back, and returns the new result.
 *
 * If `TAVILY_API_KEY` is not set, throws — callers should surface a useful
 * error to the user telling them to set the key in `.env`.
 */
export async function refreshExtract(url: string): Promise<TavilyExtractResult> {
  const apiKey = process.env.TAVILY_API_KEY;
  if (!apiKey) {
    throw new Error(
      "TAVILY_API_KEY not set in .env — required for page extraction",
    );
  }

  const resp = await fetch("https://api.tavily.com/extract", {
    method: "POST",
    headers: {
      "content-type": "application/json",
      authorization: `Bearer ${apiKey}`,
    },
    body: JSON.stringify({
      urls: [url],
      // request basic; advanced is 2x credits
      extract_depth: "basic",
      include_images: false,
    }),
  });

  if (!resp.ok) {
    const body = await resp.text().catch(() => "");
    throw new Error(`Tavily extract failed: ${resp.status} ${body.slice(0, 200)}`);
  }

  const data = (await resp.json()) as {
    results?: Array<{ url: string; raw_content: string; title?: string }>;
    failed_results?: Array<{ url: string; error: string }>;
  };

  const hit = data.results?.[0];
  if (!hit) {
    const fail = data.failed_results?.[0]?.error ?? "no result";
    throw new Error(`Tavily returned no content for ${url}: ${fail}`);
  }

  const now = new Date().toISOString();
  const prev = await readCachedExtract(url);
  const history = prev
    ? [
        ...prev.history,
        { fetched_at: prev.fetched_at, content_hash: sha8(prev.raw_content) },
      ]
    : [];

  const next: TavilyExtractResult = {
    url,
    fetched_at: now,
    title: hit.title,
    raw_content: hit.raw_content,
    history,
  };

  await fs.mkdir(cacheDir(), { recursive: true });
  await fs.writeFile(cachePath(url), JSON.stringify(next, null, 2));
  return next;
}

/**
 * Returns the cached extract if present, otherwise refreshes.
 */
export async function getOrRefreshExtract(
  url: string,
): Promise<TavilyExtractResult> {
  const cached = await readCachedExtract(url);
  if (cached) return cached;
  return refreshExtract(url);
}
