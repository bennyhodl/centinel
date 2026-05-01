import Link from "next/link";
import { sortedChildren, type TreeNode } from "@/lib/sitemap-tree";
import { StatusPill } from "../../../../_components/SitemapEntryCard";

export function BranchView({ host, node }: { host: string; node: TreeNode }) {
  const children = sortedChildren(node);
  return (
    <div>
      <div className="section-header">
        Children &middot; {children.length}
      </div>
      <div className="space-y-1">
        {children.map((c) => {
          const fullPath = c.path;
          const href = `/sitemap/host/${encodeURIComponent(host)}/${fullPath
            .split("/")
            .map(encodeURIComponent)
            .join("/")}`;
          return (
            <Link
              key={c.segment}
              href={href}
              className="flex items-baseline justify-between gap-3 border border-border bg-card px-3 py-2 transition hover:bg-accent group"
            >
              <div className="flex flex-wrap items-baseline gap-2 min-w-0">
                <span className="font-mono text-sm text-foreground group-hover:text-primary transition-colors truncate">
                  /{c.segment}
                  {c.children.size > 0 && (
                    <span className="text-muted-foreground/60">/</span>
                  )}
                </span>
                {c.entry && (
                  <>
                    <StatusPill status={c.entry.status} />
                    <span className="font-smallcaps text-[0.55rem] tracking-[0.12em] text-muted-foreground">
                      {c.entry.type}
                    </span>
                  </>
                )}
                {!c.entry && c.children.size > 0 && (
                  <span className="font-smallcaps text-[0.55rem] tracking-[0.12em] text-muted-foreground italic">
                    section
                  </span>
                )}
              </div>
              <div className="flex items-baseline gap-3 text-xs text-muted-foreground shrink-0">
                {c.children.size > 0 && (
                  <span>
                    {c.descendantCount} {c.descendantCount === 1 ? "page" : "pages"}
                  </span>
                )}
              </div>
            </Link>
          );
        })}
      </div>
    </div>
  );
}
