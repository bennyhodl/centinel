import Link from "next/link";
import { notFound } from "next/navigation";
import { extractFindingLinks, readInvestigation } from "@/lib/investigations";
import MarkdownView from "@/components/MarkdownView";
import { Pill, statusTone } from "@/components/Pill";
import { resolveWikilink } from "@/lib/wiki";

export const dynamic = "force-dynamic";

export default async function InvestigationDetailPage({
  params,
}: {
  params: Promise<{ slug: string }>;
}) {
  const { slug } = await params;
  const doc = await readInvestigation(slug);
  if (!doc) notFound();

  const fm = doc.frontmatter;
  const status = (fm.status as string | undefined) ?? "active";
  const seeds = Array.isArray(fm.seeds) ? (fm.seeds as unknown[]) : [];
  const findingLinks = extractFindingLinks(doc.body);

  return (
    <section className="space-y-6">
      <header>
        <div className="text-xs opacity-50">
          <Link href="/investigations" className="hover:text-tampa-cyan">
            ← Investigations
          </Link>
        </div>
        <h1 className="mt-2 text-2xl font-semibold">
          {(fm.title as string | undefined) ?? slug}
        </h1>
        <div className="mt-2 flex flex-wrap items-center gap-2">
          <Pill tone={statusTone(status)}>{status}</Pill>
          <span className="font-mono text-[11px] text-tampa-cyan">{slug}</span>
        </div>
      </header>

      {status === "paused" && (
        <div className="rounded-lg border border-zinc-500/30 bg-zinc-500/10 p-3 text-sm">
          This investigation is <strong>paused</strong>. No scheduled runs will
          fire until it is resumed.
        </div>
      )}

      {/* Config strip */}
      <dl className="grid grid-cols-2 gap-3 sm:grid-cols-4">
        <Stat label="schedule" value={fm.schedule ? String(fm.schedule) : "—"} />
        <Stat label="depth" value={fm.depth != null ? String(fm.depth) : "—"} />
        <Stat label="seeds" value={String(seeds.length)} />
        <Stat
          label="last run"
          value={fm.last_run ? String(fm.last_run) : "—"}
        />
      </dl>

      <div className="grid gap-6 md:grid-cols-[1fr_240px]">
        <article className="rounded-lg border border-white/10 bg-white/[0.02] p-5">
          <MarkdownView source={doc.body} />
        </article>

        <aside className="space-y-4">
          {seeds.length > 0 && (
            <div className="rounded-lg border border-white/10 bg-white/[0.02] p-4">
              <h2 className="text-xs font-semibold uppercase tracking-wider opacity-60">
                Seeds
              </h2>
              <ul className="mt-2 space-y-1 break-all font-mono text-[11px]">
                {seeds.map((s, i) => (
                  <li key={i} className="opacity-80">
                    {String(s)}
                  </li>
                ))}
              </ul>
            </div>
          )}

          <div className="rounded-lg border border-white/10 bg-white/[0.02] p-4">
            <h2 className="text-xs font-semibold uppercase tracking-wider opacity-60">
              Linked findings
            </h2>
            {findingLinks.length === 0 ? (
              <p className="mt-2 text-xs opacity-50">
                No [[Findings/…]] wikilinks found in body.
              </p>
            ) : (
              <ul className="mt-2 space-y-1 text-sm">
                {findingLinks.map((t) => (
                  <li key={t}>
                    <Link
                      href={resolveWikilink(t)}
                      className="text-tampa-cyan hover:underline"
                    >
                      {t}
                    </Link>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </aside>
      </div>
    </section>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-md border border-white/10 bg-white/[0.02] px-3 py-2">
      <div className="text-[10px] uppercase tracking-wider opacity-60">
        {label}
      </div>
      <div className="mt-0.5 truncate font-mono text-sm">{value}</div>
    </div>
  );
}
