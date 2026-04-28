import Link from "next/link";
import { notFound } from "next/navigation";
import { findFindingAcrossStacks } from "@/lib/findings";
import MarkdownView from "@/components/MarkdownView";
import { Pill, statusTone } from "@/components/Pill";

export const dynamic = "force-dynamic";

function isExternalUrl(s: string): boolean {
  return /^https?:\/\//i.test(s);
}

function vaultHref(p: string): string {
  // strip leading "Vault/" if present, since /vault is rooted there
  const cleaned = p.replace(/^\/+/, "").replace(/^Vault\//, "");
  return `/vault/${cleaned}`;
}

export default async function FindingDetailPage({
  params,
}: {
  params: Promise<{ slug: string }>;
}) {
  const { slug } = await params;
  const doc = await findFindingAcrossStacks(slug);
  if (!doc) notFound();

  const fm = doc.frontmatter;
  const summary = fm.summary as string | undefined;
  const isDraft = doc.stack === "draft";

  return (
    <section className="space-y-6">
      <header>
        <div className="text-xs opacity-50">
          <Link href="/findings" className="hover:text-tampa-cyan">
            ← Findings
          </Link>
        </div>
        <div className="mt-2 flex flex-wrap items-center gap-2">
          <Pill tone={statusTone(doc.stack)}>{doc.stack}</Pill>
          <span className="font-mono text-[11px] text-tampa-cyan">{slug}</span>
        </div>
        <h1 className="mt-2 text-2xl font-semibold">
          {(fm.title as string | undefined) ?? slug}
        </h1>
        {summary && <p className="mt-2 text-base opacity-80">{summary}</p>}
      </header>

      {isDraft && (
        <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 p-3 text-sm text-amber-200">
          <strong className="uppercase tracking-wider text-amber-300">
            Draft —
          </strong>{" "}
          not yet reviewed by editor or counsel. Do not cite.
        </div>
      )}

      <article className="rounded-lg border border-white/10 bg-white/[0.02] p-5">
        <MarkdownView source={doc.body} />
      </article>

      <section>
        <h2 className="mb-3 text-sm font-semibold uppercase tracking-wider opacity-60">
          Sources
        </h2>
        {doc.sources.length === 0 ? (
          <p className="text-sm opacity-50">No sources recorded.</p>
        ) : (
          <ul className="space-y-2">
            {doc.sources.map((s, i) => {
              if (isExternalUrl(s)) {
                return (
                  <li key={i}>
                    <a
                      href={s}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="break-all font-mono text-sm text-tampa-cyan hover:underline"
                    >
                      {s}
                    </a>
                  </li>
                );
              }
              return (
                <li key={i}>
                  <Link
                    href={vaultHref(s)}
                    className="break-all font-mono text-sm text-tampa-cyan hover:underline"
                  >
                    {s}
                  </Link>
                </li>
              );
            })}
          </ul>
        )}
      </section>

      <footer className="flex flex-wrap gap-x-4 gap-y-1 border-t border-white/10 pt-4 text-[11px] opacity-50">
        {fm.generated_at != null && (
          <span>generated {String(fm.generated_at)}</span>
        )}
        {fm.generated_by != null && <span>by {String(fm.generated_by)}</span>}
        {fm.drafted_at != null && <span>drafted {String(fm.drafted_at)}</span>}
        {fm.published_at != null && (
          <span>published {String(fm.published_at)}</span>
        )}
      </footer>
    </section>
  );
}
