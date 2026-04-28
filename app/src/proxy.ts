import { NextResponse, type NextRequest } from "next/server";

/**
 * Basic-auth gate for ALL routes (v0.1).
 *
 * The /vault static-asset route is also gated — viewers need the password.
 * Setup wizard is gated too; it lives behind the same password.
 */
export function proxy(req: NextRequest) {
  const expected = process.env.CENTINEL_PASSWORD;

  // If no password is configured, fail closed.
  if (!expected) {
    return new NextResponse("Server misconfigured: CENTINEL_PASSWORD unset", {
      status: 500,
    });
  }

  const header = req.headers.get("authorization");
  if (header?.startsWith("Basic ")) {
    try {
      const decoded = atob(header.slice(6));
      const idx = decoded.indexOf(":");
      const pw = idx === -1 ? decoded : decoded.slice(idx + 1);
      if (pw === expected) {
        return NextResponse.next();
      }
    } catch {
      // fall through
    }
  }

  return new NextResponse("Auth required", {
    status: 401,
    headers: { "WWW-Authenticate": 'Basic realm="Centinel"' },
  });
}

export const config = {
  // Skip Next internals + favicon. Everything else is gated.
  matcher: ["/((?!_next/|favicon.ico).*)"],
};
