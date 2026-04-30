"use client";

import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import Link from "next/link";

// Client-safe duplicate of resolveWikilink — does NOT import wiki.ts (which
// pulls in node:fs via config). Keep heuristics in sync with src/lib/wiki.ts.
const ENTITY_TYPE_MAP: Record<string, string> = {
  contractors: "contractor",
  people: "person",
  orgs: "org",
  organizations: "org",
  projects: "project",
};

function resolveWikilink(target: string): string {
  const cleaned = target.trim().replace(/\\/g, "/");
  const [head, ...rest] = cleaned.split("/").filter(Boolean);
  if (!head) return "/sitemap";
  const lowerHead = head.toLowerCase();
  if (ENTITY_TYPE_MAP[lowerHead] && rest.length >= 1) {
    const slug = rest.map((r) => r.toLowerCase()).join("/");
    return `/entities/${ENTITY_TYPE_MAP[lowerHead]}/${slug}`;
  }
  const segs = [head, ...rest].map((s) => s.toLowerCase());
  return `/sitemap/${segs.join("/")}`;
}

function rewriteWikilinks(src: string): string {
  return src.replace(/\[\[([^\]\n]+?)\]\]/g, (_m, inner: string) => {
    const [target, label] = inner.split("|").map((s) => s.trim());
    const href = resolveWikilink(target);
    const text = label ?? target;
    return `[${text}](${href})`;
  });
}

export default function ClientMarkdown({
  source,
  className,
}: {
  source: string;
  className?: string;
}) {
  const rewritten = rewriteWikilinks(source);
  return (
    <div className={className ?? "prose-broadsheet prose-wide"}>
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          a: ({ href, children, ...rest }) => {
            const h = href ?? "#";
            if (h.startsWith("/")) {
              return (
                <Link href={h} {...rest}>
                  {children}
                </Link>
              );
            }
            return (
              <a href={h} target="_blank" rel="noreferrer noopener" {...rest}>
                {children}
              </a>
            );
          },
        }}
      >
        {rewritten}
      </ReactMarkdown>
    </div>
  );
}
