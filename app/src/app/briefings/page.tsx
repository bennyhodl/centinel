import Link from "next/link";
import { listBriefings } from "@/lib/briefings";
import { EmptyState } from "@/components/EmptyState";

export const dynamic = "force-dynamic";

export default async function BriefingsPage() {
  const items = await listBriefings();

  return (
    <section>
      <header className="mb-6">
        <h1 className="masthead text-3xl text-foreground">
          Weekly Briefings
        </h1>
        <hr className="rule-double" />
        <p className="text-sm text-muted-foreground italic">
          Weekly digests of what changed across the city&apos;s surface.
        </p>
      </header>

      {items.length === 0 ? (
        <EmptyState title="No briefings yet">
          <p>
            Briefings are produced weekly by the digest agent and land in{" "}
            <code className="font-mono text-primary text-xs">
              &lt;wiki&gt;/Briefings/
            </code>
            .
          </p>
          <pre className="mx-auto mt-4 overflow-auto border border-border bg-secondary p-3 text-left font-mono text-xs text-foreground/80">
            hermes -s humanized-writing chat -q "weekly digest"
          </pre>
        </EmptyState>
      ) : (
        <ul className="divide-y divide-border">
          {items.map((b) => (
            <li key={b.slug} className="py-5 px-3">
              <div className="font-smallcaps text-[0.6rem] tracking-[0.15em] text-muted-foreground">
                {b.date}
              </div>
              <h2 className="mt-1 font-display text-xl font-semibold">
                {b.headline}
              </h2>
              {b.excerpt && (
                <p className="mt-2 text-sm text-muted-foreground leading-relaxed">
                  {b.excerpt}
                </p>
              )}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
