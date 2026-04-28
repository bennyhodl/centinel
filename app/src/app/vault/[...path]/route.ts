import fs from "node:fs/promises";
import path from "node:path";
import { NextResponse, type NextRequest } from "next/server";
import { vaultPath } from "@/lib/config";

// Minimal mime map. The vault holds PDFs/HTML/PNG/JPEG/CSV/XLSX/etc.
const MIME: Record<string, string> = {
  ".pdf": "application/pdf",
  ".html": "text/html; charset=utf-8",
  ".htm": "text/html; charset=utf-8",
  ".txt": "text/plain; charset=utf-8",
  ".md": "text/markdown; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".csv": "text/csv; charset=utf-8",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".webp": "image/webp",
  ".gif": "image/gif",
  ".svg": "image/svg+xml",
  ".xlsx": "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
  ".xls": "application/vnd.ms-excel",
  ".docx": "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
  ".mp4": "video/mp4",
  ".mp3": "audio/mpeg",
  ".wav": "audio/wav",
};

export async function GET(
  req: NextRequest,
  ctx: { params: Promise<{ path: string[] }> },
) {
  const { path: segs } = await ctx.params;
  if (!segs?.length) return new NextResponse("Not found", { status: 404 });

  const root = vaultPath();
  const rel = segs.map(decodeURIComponent).join("/");
  const abs = path.resolve(root, rel);

  // Refuse path traversal.
  if (!abs.startsWith(root + path.sep)) {
    return new NextResponse("Forbidden", { status: 403 });
  }

  let stat: import("node:fs").Stats;
  try {
    stat = await fs.stat(abs);
  } catch {
    return new NextResponse("Not found", { status: 404 });
  }
  if (!stat.isFile()) {
    return new NextResponse("Not found", { status: 404 });
  }

  // Strong ETag = size-mtime; vault entries are immutable so this is enough.
  const etag = `"${stat.size}-${Math.floor(stat.mtimeMs)}"`;

  // Conditional GET — vault entries are immutable so equal ETag = 304.
  const ifNoneMatch = req.headers.get("if-none-match");
  if (ifNoneMatch && ifNoneMatch === etag) {
    return new NextResponse(null, { status: 304, headers: { ETag: etag } });
  }

  const ext = path.extname(abs).toLowerCase();
  const contentType = MIME[ext] ?? "application/octet-stream";
  const buf = await fs.readFile(abs);
  // Convert Node Buffer to Uint8Array view — NextResponse accepts BodyInit.
  const body = new Uint8Array(buf.buffer, buf.byteOffset, buf.byteLength);

  return new NextResponse(body, {
    status: 200,
    headers: {
      "Content-Type": contentType,
      "Content-Length": String(stat.size),
      "Cache-Control": "public, max-age=31536000, immutable",
      ETag: etag,
    },
  });
}
