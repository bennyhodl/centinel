import Link from "next/link";
import { listBriefings } from "@/lib/briefings";
import { EmptyState } from "@/components/EmptyState";

export const dynamic = "force-dynamic";

export default async function BriefingsPage() {
  const items = await listBriefings();

  return (
    <section className="space-y-6">
      <header>
        <h1 className="text-2xl font-semibold">Briefings</h1>
        <p className="mt-1 text-sm opacity-60">
          Weekly digests of what changed across the city&apos;s surface.
        </p>
      </header>

      {items.length === 0 ? (
        <EmptyState title="No briefings yet">
          <p>
            Briefings are produced weekly by the digest agent and land in{" "}
            <code className="font-mono text-tampa-cyan">
              &lt;wiki&gt;/Briefings/
            </code>
            .
          </p>
          <pre className="mx-auto mt-4 overflow-auto rounded bg-black/40 p-3 text-left font-mono text-xs text-tampa-cyan">
            hermes session run weekly-digest
          </pre>
        </EmptyState>
      ) : (
        <ul className="grid gap-3">
          {items.map((b) => (
            <li
              key={b.slug}
              className="rounded-lg border border-white/10 bg-white/[0.02] p-4"
            >
              <div className="font-mono text-[11px] text-tampa-cyan">
                {b.date}
              </div>
              <h2 className="mt-1 text-base font-semibold">{b.headline}</h2>
              {b.excerpt && (
                <p className="mt-1 text-sm opacity-75">{b.excerpt}</p>
              )}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
