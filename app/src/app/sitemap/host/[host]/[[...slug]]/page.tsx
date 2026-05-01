import Link from "next/link";
import { notFound } from "next/navigation";
import { loadSitemap, type SitemapDoc } from "@/lib/sitemap";
import {
  buildHostTrees,
  resolveNode,
} from "@/lib/sitemap-tree";
import { SitemapEmptyState } from "../../../_components/EmptyState";
import { LeafDetail } from "./_components/LeafDetail";
import { BranchView } from "./_components/BranchView";

export const dynamic = "force-dynamic";

interface Props {
  params: Promise<{ host: string; slug?: string[] }>;
}

function collectSitemapUrls(doc: SitemapDoc): Set<string> {
  return new Set(doc.entries.map((e) => e.url));
}

export default async function SitemapNodePage({ params }: Props) {
  const { host: hostRaw, slug } = await params;
  const host = decodeURIComponent(hostRaw);
  const segments = (slug ?? []).map((s) => decodeURIComponent(s));

  const doc = await loadSitemap();
  if (!doc) {
    return (
      <section>
        <BackLink host={host} segments={segments} />
        <SitemapEmptyState />
      </section>
    );
  }

  const trees = buildHostTrees(doc);
  const tree = trees.get(host);
  if (!tree) notFound();

  const node = resolveNode(tree, segments);
  if (!node) notFound();

  const knownUrls = collectSitemapUrls(doc);
  const hasChildren = node.children.size > 0;

  return (
    <section className="space-y-6">
      <BackLink host={host} segments={segments} />
      <Breadcrumbs host={host} segments={segments} />

      {/* If this node has its own crawled entry, show its detail card */}
      {node.entry && (
        <LeafDetail entry={node.entry} sitemapUrls={knownUrls} />
      )}

      {/* If branch (has children), show child list */}
      {hasChildren && (
        <BranchView host={host} node={node} />
      )}

      {/* No entry, no children — shouldn't happen, but be defensive */}
      {!node.entry && !hasChildren && (
        <p className="border border-dashed border-border bg-card p-6 text-center text-sm text-muted-foreground">
          Empty node.
        </p>
      )}
    </section>
  );
}

function BackLink({ host, segments }: { host: string; segments: string[] }) {
  if (segments.length === 0) {
    return (
      <Link
        href="/sitemap"
        className="inline-block text-xs text-primary hover:underline italic"
      >
        &larr; back to sitemap
      </Link>
    );
  }
  const parent = segments.slice(0, -1);
  const href =
    parent.length === 0
      ? `/sitemap/host/${encodeURIComponent(host)}`
      : `/sitemap/host/${encodeURIComponent(host)}/${parent
          .map(encodeURIComponent)
          .join("/")}`;
  return (
    <Link href={href} className="inline-block text-xs text-primary hover:underline italic">
      &larr; up to /{parent.join("/") || host}
    </Link>
  );
}

function Breadcrumbs({ host, segments }: { host: string; segments: string[] }) {
  const crumbs = [
    { label: host, href: `/sitemap/host/${encodeURIComponent(host)}` },
    ...segments.map((seg, i) => ({
      label: seg,
      href: `/sitemap/host/${encodeURIComponent(host)}/${segments
        .slice(0, i + 1)
        .map(encodeURIComponent)
        .join("/")}`,
    })),
  ];
  return (
    <header>
      <h1 className="masthead text-2xl text-foreground break-all">
        {crumbs.map((c, i) => (
          <span key={i}>
            {i > 0 && <span className="text-muted-foreground/60">/</span>}
            {i < crumbs.length - 1 ? (
              <Link href={c.href} className="hover:text-primary transition-colors">
                {c.label}
              </Link>
            ) : (
              <span>{c.label}</span>
            )}
          </span>
        ))}
      </h1>
      <hr className="rule-double" />
    </header>
  );
}
