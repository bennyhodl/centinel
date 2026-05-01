import Link from "next/link";
import { listBriefings } from "@/lib/briefings";
import { EmptyState } from "@/components/EmptyState";
import { GenerateBriefingButton } from "./_components/GenerateBriefingButton";

export const dynamic = "force-dynamic";

export default async function BriefingsPage() {
  const items = await listBriefings();

  return (
    <section>
      <header className="mb-6 flex flex-wrap items-baseline justify-between gap-4">
        <div>
          <h1 className="masthead text-3xl text-foreground">
            Weekly Briefings
          </h1>
          <hr className="rule-double" />
          <p className="text-sm text-muted-foreground italic">
            Weekly digests of what changed across the city&apos;s surface.
          </p>
        </div>
        {items.length > 0 && <GenerateBriefingButton compact />}
      </header>

      {items.length === 0 ? (
        <EmptyState title="No briefings yet">
          <p className="mb-2">
            Briefings are produced weekly by the digest agent and land in{" "}
            <code className="font-mono text-primary text-xs">
              &lt;wiki&gt;/Briefings/
            </code>
            . You don&apos;t have to wait — generate one now from whatever
            findings, outbox notes, and run logs already exist.
          </p>
          <p className="mb-4 text-xs text-muted-foreground">
            Generation takes 1–3 minutes. You can keep using the rest of the
            app while it runs.
          </p>
          <div className="flex justify-center">
            <GenerateBriefingButton />
          </div>
          <details className="mt-4 text-xs text-muted-foreground">
            <summary className="cursor-pointer">CLI alternative</summary>
            <pre className="mx-auto mt-2 overflow-auto border border-border bg-secondary p-3 text-left font-mono text-xs text-foreground/80">
              bin/centinel briefing run-now
            </pre>
            <p className="mt-2 italic">
              You can also see live agent activity on the{" "}
              <Link href="/status" className="text-primary hover:underline">
                status page
              </Link>
              .
            </p>
          </details>
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
