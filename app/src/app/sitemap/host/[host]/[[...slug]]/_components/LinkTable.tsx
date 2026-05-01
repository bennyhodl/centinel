"use client";

import { useState } from "react";
import type { SitemapLink } from "@/lib/sitemap";

const KIND_COLOR: Record<string, string> = {
  sitemap: "border-primary/30 bg-primary/5 text-primary",
  internal: "border-foreground/20 bg-secondary text-foreground/70",
  external: "border-foreground/20 bg-card text-muted-foreground",
  document: "border-amber-700/30 bg-amber-50 text-amber-900",
  mailto: "border-foreground/20 bg-card text-muted-foreground italic",
  tel: "border-foreground/20 bg-card text-muted-foreground italic",
  anchor: "border-foreground/20 bg-card text-muted-foreground/70 italic",
};

export function LinkTable({
  links,
  pageUrl: _pageUrl,
}: {
  links: SitemapLink[];
  pageUrl: string;
}) {
  const [filter, setFilter] = useState<string>("all");
  const counts = links.reduce<Record<string, number>>((acc, l) => {
    acc[l.kind] = (acc[l.kind] ?? 0) + 1;
    return acc;
  }, {});
  const visible = filter === "all" ? links : links.filter((l) => l.kind === filter);

  return (
    <div>
      {/* Filter chips */}
      <div className="flex flex-wrap gap-1.5 mb-2 text-[0.65rem]">
        <FilterChip
          label={`all ${links.length}`}
          active={filter === "all"}
          onClick={() => setFilter("all")}
        />
        {Object.entries(counts).map(([kind, n]) => (
          <FilterChip
            key={kind}
            label={`${kind} ${n}`}
            active={filter === kind}
            onClick={() => setFilter(kind)}
          />
        ))}
      </div>

      <div className="border border-border bg-card divide-y divide-border">
        {visible.map((l, i) => (
          <LinkRow key={i} link={l} />
        ))}
      </div>
    </div>
  );
}

function FilterChip({
  label,
  active,
  onClick,
}: {
  label: string;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`border px-1.5 py-0.5 transition ${
        active
          ? "border-primary bg-primary/10 text-primary"
          : "border-border bg-secondary text-muted-foreground hover:bg-accent"
      }`}
    >
      {label}
    </button>
  );
}

/**
 * Resolve the in-app sitemap drill-in path for a sitemap-kind link.
 * Returns null if the URL doesn't parse cleanly.
 */
function sitemapDrillHref(link: SitemapLink): string | null {
  if (link.kind !== "sitemap") return null;
  try {
    const u = new URL(link.href);
    const segs = u.pathname.split("/").filter(Boolean);
    return `/sitemap/host/${encodeURIComponent(u.host)}${segs.length ? "/" + segs.map(encodeURIComponent).join("/") : ""}`;
  } catch {
    return null;
  }
}

function LinkRow({ link }: { link: SitemapLink }) {
  const cls = KIND_COLOR[link.kind] ?? KIND_COLOR.external;
  const drillHref = sitemapDrillHref(link);

  // Anchor text is now the primary clickable element.
  // - sitemap-kind → drill into the in-app sitemap (same window)
  // - mailto/tel/anchor → keep their semantic href
  // - everything else → external (new tab)
  const anchorText = link.anchor || "(no anchor)";
  const titleHrefProps = drillHref
    ? { href: drillHref }
    : link.kind === "anchor"
      ? { href: link.href }
      : link.kind === "mailto" || link.kind === "tel"
        ? { href: link.href }
        : { href: link.href, target: "_blank", rel: "noopener noreferrer" as const };

  return (
    <div className="px-3 py-2 hover:bg-accent">
      <div className="flex flex-wrap items-baseline gap-2">
        <span
          className={`border px-1.5 py-0.5 font-smallcaps text-[0.55rem] tracking-[0.12em] uppercase ${cls}`}
        >
          {link.kind}
        </span>
        <a
          {...titleHrefProps}
          className={`font-serif text-sm flex-1 min-w-0 ${
            link.anchor
              ? "text-foreground hover:text-primary hover:underline"
              : "text-muted-foreground italic"
          } ${drillHref ? "cursor-pointer" : ""}`}
        >
          {anchorText}
          {drillHref && (
            <span className="ml-1 text-primary/70 text-xs not-italic">→</span>
          )}
        </a>
      </div>
      <div className="mt-1 flex flex-wrap items-baseline gap-2 text-xs">
        {/* URL shown muted as the underlying destination — not the primary action */}
        <span className="font-mono text-muted-foreground/70 break-all">
          {link.href}
        </span>
        {/* Open external in new tab — even when title drills into sitemap, give a way out to the live page */}
        {link.kind !== "anchor" && link.kind !== "mailto" && link.kind !== "tel" && (
          <a
            href={link.href}
            target="_blank"
            rel="noopener noreferrer"
            className="font-smallcaps text-[0.55rem] tracking-[0.12em] text-muted-foreground hover:text-primary uppercase"
            title="open the live URL in a new tab"
          >
            open ↗
          </a>
        )}
      </div>
      {link.llm_summary && (
        <p className="mt-1 text-xs text-foreground/70 italic">
          {link.llm_summary}
        </p>
      )}
    </div>
  );
}
