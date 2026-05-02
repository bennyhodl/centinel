import { NextRequest } from "next/server";
import { locateSession } from "@/lib/sessions";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

/**
 * GET /api/sessions/[id]
 *
 * Returns the parsed session document. Used by /runs/[id] page (initial
 * render) and by the live-poll loop on the same page (which fetches every
 * 2-5s while the session is still being written).
 */
export async function GET(
  _req: NextRequest,
  ctx: { params: Promise<{ id: string }> },
) {
  const { id } = await ctx.params;
  const found = await locateSession(id);
  if (!found) {
    return Response.json(
      { error: "session not found", id },
      { status: 404, headers: { "Cache-Control": "no-store" } },
    );
  }
  return Response.json(
    { ...found.doc, profile: found.profile },
    { headers: { "Cache-Control": "no-store" } },
  );
}
