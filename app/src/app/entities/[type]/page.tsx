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
      <header className="mb-6">
        <div className="text-xs text-muted-foreground">
          <Link href="/entities" className="hover:text-primary italic">
            &larr; Entities
          </Link>
        </div>
        <h1 className="mt-2 masthead text-3xl text-foreground">
          {ENTITY_TYPE_LABELS[type]}
        </h1>
        <hr className="rule-double" />
        <p className="text-sm text-muted-foreground italic">
          {items.length} {ENTITY_TYPE_LABELS[type].toLowerCase()}
        </p>
      </header>

      {items.length === 0 ? (
        <EmptyState title={`No ${ENTITY_TYPE_LABELS[type].toLowerCase()} yet`}>
          <p>
            None have been extracted yet. They populate{" "}
            <code className="font-mono text-primary">
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
                className="block border border-border bg-card p-4 transition hover:border-primary/40 hover:bg-accent"
              >
                <div className="font-mono text-[11px] text-primary">
                  {it.slug}
                </div>
                <h2 className="mt-1 text-base font-semibold">
                  {(it.frontmatter.title as string | undefined) ?? it.slug}
                </h2>
                {it.excerpt && (
                  <p className="mt-1 text-sm text-muted-foreground">{it.excerpt}</p>
                )}
              </Link>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
