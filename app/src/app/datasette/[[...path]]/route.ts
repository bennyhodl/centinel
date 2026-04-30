// Reverse-proxy to the internal Datasette container. Lets the browser hit
// Datasette through the same origin as the Next.js app — no second port to
// expose, no CORS, works identically behind a reverse proxy or by raw IP.
//
// Datasette is configured with `base_url=/datasette/` so its internal links
// (static assets, table URLs, redirects) already include the /datasette
// prefix and route back through here.

import { NextResponse, type NextRequest } from "next/server";

export const dynamic = "force-dynamic";
// Streaming proxy — Node runtime only.
export const runtime = "nodejs";

function internalBase(): string {
  return (
    process.env.DATASETTE_INTERNAL_URL ||
    process.env.DATASETTE_URL ||
    "http://127.0.0.1:8001"
  ).replace(/\/$/, "");
}

// Hop-by-hop headers per RFC 7230 §6.1 — never forward.
const HOP_BY_HOP = new Set([
  "connection",
  "keep-alive",
  "proxy-authenticate",
  "proxy-authorization",
  "te",
  "trailer",
  "transfer-encoding",
  "upgrade",
  "host",
  "content-length",
]);

function filterHeaders(src: Headers): Headers {
  const out = new Headers();
  src.forEach((value, key) => {
    if (!HOP_BY_HOP.has(key.toLowerCase())) out.append(key, value);
  });
  return out;
}

async function proxy(req: NextRequest, segs: string[] | undefined) {
  const base = internalBase();
  const subpath = (segs ?? []).map(encodeURIComponent).join("/");
  // Datasette is mounted at /datasette/ via its base_url setting, so we
  // forward to <internal>/datasette/<rest>.
  const url = new URL(req.url);
  const target = `${base}/datasette/${subpath}${url.search}`;

  const init: RequestInit = {
    method: req.method,
    headers: filterHeaders(req.headers),
    redirect: "manual",
    // Body for non-GET/HEAD. Next.js gives us a web ReadableStream.
    body:
      req.method === "GET" || req.method === "HEAD"
        ? undefined
        : (req.body as unknown as BodyInit | null | undefined),
    // Required when forwarding a streaming body in Node fetch.
    // @ts-expect-error duplex is valid but missing from RequestInit types
    duplex: "half",
  };

  let upstream: Response;
  try {
    upstream = await fetch(target, init);
  } catch (err) {
    return new NextResponse(
      `datasette proxy: upstream unreachable (${(err as Error).message})`,
      { status: 502, headers: { "Content-Type": "text/plain; charset=utf-8" } },
    );
  }

  const headers = filterHeaders(upstream.headers);
  // Rewrite Location for any redirect Datasette emits — base_url should
  // already prefix them, but be defensive about absolute upstream URLs.
  const loc = upstream.headers.get("location");
  if (loc) {
    try {
      const u = new URL(loc, target);
      if (u.origin === new URL(base).origin) {
        headers.set("location", u.pathname + u.search);
      }
    } catch {
      // leave as-is
    }
  }

  return new NextResponse(upstream.body, {
    status: upstream.status,
    statusText: upstream.statusText,
    headers,
  });
}

export async function GET(
  req: NextRequest,
  ctx: { params: Promise<{ path?: string[] }> },
) {
  const { path } = await ctx.params;
  return proxy(req, path);
}

export async function HEAD(
  req: NextRequest,
  ctx: { params: Promise<{ path?: string[] }> },
) {
  const { path } = await ctx.params;
  return proxy(req, path);
}

export async function POST(
  req: NextRequest,
  ctx: { params: Promise<{ path?: string[] }> },
) {
  const { path } = await ctx.params;
  return proxy(req, path);
}
