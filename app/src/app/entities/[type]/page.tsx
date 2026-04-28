import Link from "next/link";
import { notFound } from "next/navigation";
import {
  ENTITY_TYPE_LABELS,
  isEntityType,
  listEntities,
} from "@/lib/entities";
import { EmptyState } from "@/components/EmptyState";

export const dynamic = "force-dynamic";

export default async function EntityTypePage({
  params,
}: {
  params: Promise<{ type: string }>;
}) {
  const { type } = await params;
  if (!isEntityType(type)) notFound();
  const items = await listEntities(type);

  return (
    <section className="space-y-6">
      <header>
        <div className="text-xs opacity-50">
          <Link href="/entities" className="hover:text-tampa-cyan">
            ← Entities
          </Link>
        </div>
        <h1 className="mt-2 text-2xl font-semibold">
          {ENTITY_TYPE_LABELS[type]}
        </h1>
        <p className="mt-1 text-sm opacity-60">
          {items.length} {ENTITY_TYPE_LABELS[type].toLowerCase()}
        </p>
      </header>

      {items.length === 0 ? (
        <EmptyState title={`No ${ENTITY_TYPE_LABELS[type].toLowerCase()} yet`}>
          <p>
            None have been extracted yet. They populate{" "}
            <code className="font-mono text-tampa-cyan">
              &lt;wiki&gt;/Entities/{type}/
            </code>
            .
          </p>
        </EmptyState>
      ) : (
        <ul className="grid gap-3 sm:grid-cols-2">
          {items.map((it) => (
            <li key={it.slug}>
              <Link
                href={`/entities/${type}/${it.slug}`}
                className="block rounded-lg border border-white/10 bg-white/[0.02] p-4 transition hover:border-tampa-cyan/40 hover:bg-white/[0.04]"
              >
                <div className="font-mono text-[11px] text-tampa-cyan">
                  {it.slug}
                </div>
                <h2 className="mt-1 text-base font-semibold">
                  {(it.frontmatter.title as string | undefined) ?? it.slug}
                </h2>
                {it.excerpt && (
                  <p className="mt-1 text-sm opacity-70">{it.excerpt}</p>
                )}
              </Link>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
