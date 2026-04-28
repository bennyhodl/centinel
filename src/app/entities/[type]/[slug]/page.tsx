import Link from "next/link";
import { notFound } from "next/navigation";
import {
  ENTITY_TYPE_LABELS,
  isEntityType,
  readEntity,
} from "@/lib/entities";
import MarkdownView from "@/components/MarkdownView";

export const dynamic = "force-dynamic";

export default async function EntityDetailPage({
  params,
}: {
  params: Promise<{ type: string; slug: string }>;
}) {
  const { type, slug } = await params;
  if (!isEntityType(type)) notFound();
  const doc = await readEntity(type, slug);
  if (!doc) notFound();

  const fm = doc.frontmatter;
  const aliases = Array.isArray(fm.aliases)
    ? (fm.aliases as unknown[]).map(String)
    : [];

  const stats: { label: string; value: string }[] = [];
  if (fm.first_seen != null)
    stats.push({ label: "first seen", value: String(fm.first_seen) });
  if (fm.last_seen != null)
    stats.push({ label: "last seen", value: String(fm.last_seen) });
  if (fm.mentions_count != null)
    stats.push({ label: "mentions", value: String(fm.mentions_count) });
  if (fm.findings_count != null)
    stats.push({ label: "findings", value: String(fm.findings_count) });

  return (
    <section className="space-y-6">
      <header>
        <div className="text-xs opacity-50">
          <Link href="/entities" className="hover:text-tampa-cyan">
            Entities
          </Link>{" "}
          /{" "}
          <Link
            href={`/entities/${type}`}
            className="hover:text-tampa-cyan"
          >
            {ENTITY_TYPE_LABELS[type]}
          </Link>
        </div>
        <h1 className="mt-2 text-2xl font-semibold">
          {(fm.title as string | undefined) ?? slug}
        </h1>
        <div className="mt-2 flex flex-wrap items-center gap-2">
          <span className="rounded bg-white/5 px-1.5 py-0.5 font-mono text-[10px] uppercase tracking-wider text-tampa-cyan">
            {type}
          </span>
          <span className="font-mono text-[11px] opacity-60">{slug}</span>
        </div>

        {aliases.length > 0 && (
          <ul className="mt-3 flex flex-wrap gap-1.5">
            {aliases.map((a) => (
              <li
                key={a}
                className="rounded-full bg-white/5 px-2 py-0.5 text-[11px] opacity-80"
              >
                {a}
              </li>
            ))}
          </ul>
        )}
      </header>

      {stats.length > 0 && (
        <dl className="grid grid-cols-2 gap-3 sm:grid-cols-4">
          {stats.map((s) => (
            <div
              key={s.label}
              className="rounded-md border border-white/10 bg-white/[0.02] px-3 py-2"
            >
              <div className="text-[10px] uppercase tracking-wider opacity-60">
                {s.label}
              </div>
              <div className="mt-0.5 font-mono text-sm">{s.value}</div>
            </div>
          ))}
        </dl>
      )}

      <article className="rounded-lg border border-white/10 bg-white/[0.02] p-5">
        <MarkdownView source={doc.body} />
      </article>
    </section>
  );
}
