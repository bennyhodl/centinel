import Link from "next/link";
import { notFound } from "next/navigation";
import { locateSession } from "@/lib/sessions";
import { RunViewer } from "../_components/RunViewer";

export const dynamic = "force-dynamic";

interface PageProps {
  params: Promise<{ id: string }>;
}

export default async function RunDetailPage({ params }: PageProps) {
  const { id } = await params;
  const located = await locateSession(decodeURIComponent(id));
  if (!located) notFound();
  const { doc, profile } = located;

  return (
    <section>
      <header className="mb-4">
        <Link
          href="/runs"
          className="text-xs text-primary hover:underline italic"
        >
          ← all runs
        </Link>
        <div className="mt-2 flex flex-wrap items-baseline gap-2">
          <span className="border border-border bg-secondary px-2 py-0.5 font-smallcaps text-[0.6rem] tracking-[0.12em] uppercase text-muted-foreground">
            {profile}
          </span>
          <h1 className="masthead text-2xl text-foreground break-all">
            {doc.id}
          </h1>
        </div>
        <hr className="rule-double" />
        <div className="flex flex-wrap gap-x-4 gap-y-1 text-[0.7rem] text-muted-foreground">
          {doc.model && <span>model: <code className="font-mono">{doc.model}</code></span>}
          {doc.sessionStart && (
            <span>started {new Date(doc.sessionStart).toLocaleString()}</span>
          )}
          {doc.lastUpdated && (
            <span>updated {new Date(doc.lastUpdated).toLocaleString()}</span>
          )}
          <span>{doc.messages.length} messages</span>
        </div>
      </header>

      <RunViewer id={doc.id} initialDoc={doc} />
    </section>
  );
}
