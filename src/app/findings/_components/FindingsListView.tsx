import Link from "next/link";
import {
  type FindingStack,
  type FindingSummary,
  listAllFindings,
  listFindings,
} from "@/lib/findings";
import { EmptyState } from "@/components/EmptyState";
import { Pill, statusTone } from "@/components/Pill";

const TABS = [
  { key: "all", label: "All" },
  { key: "raw", label: "Raw" },
  { key: "published", label: "Published" },
  { key: "draft", label: "Draft" },
] as const;

type TabKey = (typeof TABS)[number]["key"];

function isTab(s: string | undefined): s is TabKey {
  return !!s && TABS.some((t) => t.key === s);
}

export default async function FindingsListView({
  activeTab,
  showDraftBanner = false,
}: {
  activeTab: TabKey;
  showDraftBanner?: boolean;
}) {
  let items: FindingSummary[];
  if (activeTab === "all") {
    items = await listAllFindings();
  } else {
    items = await listFindings(activeTab as FindingStack);
  }

  return (
    <section className="space-y-6">
      <header>
        <h1 className="text-2xl font-semibold">Findings</h1>
        <p className="mt-1 text-sm opacity-60">
          Auto-published facts (raw), operator-promoted narratives (published),
          and editor-drafted pieces (draft).
        </p>
      </header>

      <nav className="flex flex-wrap gap-1.5 border-b border-white/10">
        {TABS.map((t) => {
          const href =
            t.key === "draft"
              ? "/findings/draft"
              : t.key === "all"
                ? "/findings"
                : `/findings?stack=${t.key}`;
          const active = t.key === activeTab;
          return (
            <Link
              key={t.key}
              href={href}
              className={`rounded-t-md px-3 py-1.5 text-sm transition ${
                active
                  ? "border border-b-0 border-white/10 bg-white/[0.04] text-tampa-cyan"
                  : "opacity-70 hover:opacity-100"
              }`}
            >
              {t.label}
            </Link>
          );
        })}
      </nav>

      {showDraftBanner && (
        <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 p-3 text-sm text-amber-200">
          <strong className="uppercase tracking-wider text-amber-300">
            Draft —
          </strong>{" "}
          not yet reviewed by editor or counsel. Do not cite.
        </div>
      )}

      {items.length === 0 ? (
        <EmptyState title="No findings yet">
          <p>
            Findings are produced by the civic-investigator and promoted by the
            Editor.{" "}
            {activeTab === "raw"
              ? "Raw findings populate as the investigator runs."
              : activeTab === "published"
                ? "Published findings appear after operator review."
                : activeTab === "draft"
                  ? "Drafts appear when the Editor stages a narrative."
                  : "Launch one from /chat to begin."}
          </p>
        </EmptyState>
      ) : (
        <ul className="grid gap-3">
          {items.map((it) => (
            <li key={`${it.stack}/${it.slug}`}>
              <Link
                href={`/findings/${it.slug}`}
                className="block rounded-lg border border-white/10 bg-white/[0.02] p-4 transition hover:border-tampa-cyan/40 hover:bg-white/[0.04]"
              >
                <div className="flex flex-wrap items-baseline gap-x-2 gap-y-1">
                  <Pill tone={statusTone(it.stack)}>{it.stack}</Pill>
                  <span className="font-mono text-[11px] text-tampa-cyan">
                    {it.slug}
                  </span>
                </div>
                <h2 className="mt-2 text-base font-semibold">
                  {(it.frontmatter.title as string | undefined) ?? it.slug}
                </h2>
                {(it.frontmatter.summary as string | undefined) && (
                  <p className="mt-1 text-sm opacity-80">
                    {String(it.frontmatter.summary)}
                  </p>
                )}
                {it.excerpt && !it.frontmatter.summary && (
                  <p className="mt-1 text-sm opacity-70">{it.excerpt}</p>
                )}
                <footer className="mt-3 flex flex-wrap gap-x-4 gap-y-1 text-[11px] opacity-50">
                  {it.frontmatter.published_at != null && (
                    <span>published {String(it.frontmatter.published_at)}</span>
                  )}
                  {it.frontmatter.drafted_at != null && (
                    <span>drafted {String(it.frontmatter.drafted_at)}</span>
                  )}
                  {it.frontmatter.generated_at != null && (
                    <span>generated {String(it.frontmatter.generated_at)}</span>
                  )}
                  {it.frontmatter.generated_by != null && (
                    <span>by {String(it.frontmatter.generated_by)}</span>
                  )}
                </footer>
              </Link>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

export { isTab, type TabKey };
