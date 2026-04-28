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
      <header className="mb-6">
        <div className="text-xs text-muted-foreground italic">
          <Link href="/entities" className="hover:text-primary">
            Entities
          </Link>{" "}
          /{" "}
          <Link
            href={`/entities/${type}`}
            className="hover:text-primary"
          >
            {ENTITY_TYPE_LABELS[type]}
          </Link>
        </div>
        <h1 className="mt-2 masthead text-3xl text-foreground">
          {(fm.title as string | undefined) ?? slug}
        </h1>
        <hr className="rule-double" />
        <div className="mt-2 flex flex-wrap items-center gap-2">
          <span className="rounded bg-secondary px-1.5 py-0.5 font-mono text-[10px] uppercase tracking-wider text-primary">
            {type}
          </span>
          <span className="font-mono text-[11px] text-muted-foreground">{slug}</span>
        </div>

        {aliases.length > 0 && (
          <ul className="mt-3 flex flex-wrap gap-1.5">
            {aliases.map((a) => (
              <li
                key={a}
                className="bg-secondary px-2 py-0.5 text-[11px] text-foreground/80"
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
              className="border border-border bg-card px-3 py-2"
            >
              <div className="text-[10px] uppercase tracking-wider text-muted-foreground">
                {s.label}
              </div>
              <div className="mt-0.5 font-mono text-sm">{s.value}</div>
            </div>
          ))}
        </dl>
      )}

      <article className="border border-border bg-card p-5">
        <MarkdownView source={doc.body} />
      </article>
    </section>
  );
}
