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
  pageUrl,
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
          <LinkRow key={i} link={l} pageUrl={pageUrl} />
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

function LinkRow({ link, pageUrl }: { link: SitemapLink; pageUrl: string }) {
  const [explainLoading, setExplainLoading] = useState(false);
  const [explanation, setExplanation] = useState<string | undefined>(
    link.llm_summary,
  );
  const [err, setErr] = useState<string | null>(null);

  async function explain() {
    setExplainLoading(true);
    setErr(null);
    try {
      const res = await fetch("/api/sitemap/explain-link", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          page_url: pageUrl,
          link_href: link.href,
          link_anchor: link.anchor,
        }),
      });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const data = (await res.json()) as { summary: string };
      setExplanation(data.summary);
    } catch (e) {
      setErr(e instanceof Error ? e.message : "explain failed");
    } finally {
      setExplainLoading(false);
    }
  }

  // Sitemap-link → drill into the in-app sitemap node
  let sitemapHref: string | null = null;
  if (link.kind === "sitemap") {
    try {
      const u = new URL(link.href);
      const segs = u.pathname.split("/").filter(Boolean);
      sitemapHref = `/sitemap/host/${encodeURIComponent(u.host)}${segs.length ? "/" + segs.map(encodeURIComponent).join("/") : ""}`;
    } catch {}
  }

  const cls = KIND_COLOR[link.kind] ?? KIND_COLOR.external;

  return (
    <div className="px-3 py-2 hover:bg-accent">
      <div className="flex flex-wrap items-baseline gap-2">
        <span
          className={`border px-1.5 py-0.5 font-smallcaps text-[0.55rem] tracking-[0.12em] uppercase ${cls}`}
        >
          {link.kind}
        </span>
        <span className="font-serif text-sm text-foreground flex-1 min-w-0">
          {link.anchor || <em className="text-muted-foreground">(no anchor)</em>}
        </span>
      </div>
      <div className="mt-1 flex flex-wrap items-baseline gap-2 text-xs">
        <a
          href={link.href}
          target="_blank"
          rel="noopener noreferrer"
          className="font-mono text-muted-foreground hover:text-primary break-all"
        >
          {link.href}
        </a>
        {sitemapHref && (
          <a
            href={sitemapHref}
            className="text-primary hover:underline italic"
          >
            open in sitemap →
          </a>
        )}
        {!explanation && (
          <button
            type="button"
            onClick={explain}
            disabled={explainLoading}
            className="text-primary hover:underline italic disabled:opacity-50"
          >
            {explainLoading ? "explaining…" : "explain"}
          </button>
        )}
      </div>
      {explanation && (
        <p className="mt-1 text-xs text-foreground/70 italic">
          {explanation}
        </p>
      )}
      {err && <p className="mt-1 text-xs text-red-800 italic">{err}</p>}
    </div>
  );
}
