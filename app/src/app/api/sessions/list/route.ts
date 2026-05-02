import { NextRequest } from "next/server";
import {
  listSessions,
  KNOWN_PROFILES,
  type Profile,
} from "@/lib/sessions";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

/**
 * GET /api/sessions/list?profile=<name>&cronJobId=<id>&sinceMs=<ms>&limit=<n>
 *
 * Lightweight (filename-only) listing of Hermes sessions across the four
 * Centinel role profiles. Used by /runs to render the run picker without
 * loading every session JSON.
 */
export async function GET(req: NextRequest) {
  const url = new URL(req.url);
  const profileParam = url.searchParams.get("profile");
  const cronJobId = url.searchParams.get("cronJobId") ?? undefined;
  const sinceMsRaw = url.searchParams.get("sinceMs");
  const limitRaw = url.searchParams.get("limit");

  const profile: Profile | "all" =
    !profileParam || profileParam === "all"
      ? "all"
      : (KNOWN_PROFILES as readonly string[]).includes(profileParam)
        ? (profileParam as Profile)
        : "all";

  const items = await listSessions({
    profile,
    cronJobId,
    sinceMs: sinceMsRaw ? Number(sinceMsRaw) : undefined,
    limit: limitRaw ? Math.min(500, Math.max(1, Number(limitRaw))) : 100,
  });

  return Response.json(
    { items },
    { headers: { "Cache-Control": "no-store" } },
  );
}
