"use client";

import { useState, useTransition } from "react";
import { useRouter } from "next/navigation";

export function PageSummary({
  url,
  initialSummary,
  initialSummaryAt,
}: {
  url: string;
  initialSummary?: string;
  initialSummaryAt?: string;
}) {
  const router = useRouter();
  const [summary, setSummary] = useState<string | undefined>(initialSummary);
  const [summarizedAt, setSummarizedAt] = useState<string | undefined>(
    initialSummaryAt,
  );
  const [loading, setLoading] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [, startTransition] = useTransition();

  async function summarize(refresh: boolean) {
    setLoading(true);
    setErr(null);
    try {
      const res = await fetch("/api/sitemap/summarize-page", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ url, refresh }),
      });
      if (!res.ok) {
        const body = (await res.json().catch(() => null)) as
          | { error?: string; detail?: string }
          | null;
        throw new Error(body?.detail || body?.error || `HTTP ${res.status}`);
      }
      const data = (await res.json()) as {
        summary: string;
        summarized_at: string;
      };
      setSummary(data.summary);
      setSummarizedAt(data.summarized_at);
      startTransition(() => router.refresh());
    } catch (e) {
      setErr(e instanceof Error ? e.message : "summarize failed");
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="mt-4 border border-border bg-secondary/30 p-3">
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <div className="font-smallcaps text-[0.6rem] tracking-[0.12em] text-muted-foreground uppercase">
          Page summary
        </div>
        <div className="flex items-baseline gap-3 text-xs">
          {!summary && (
            <button
              type="button"
              disabled={loading}
              onClick={() => summarize(false)}
              className="border border-primary/40 bg-primary/5 px-2 py-1 text-primary hover:bg-primary/10 disabled:opacity-50"
            >
              {loading ? "summarizing…" : "✦ summarize page"}
            </button>
          )}
          {summary && (
            <button
              type="button"
              disabled={loading}
              onClick={() => summarize(true)}
              className="text-primary hover:underline italic disabled:opacity-50"
              title="Re-fetch from Tavily and re-summarize"
            >
              {loading ? "refreshing…" : "⟳ refresh"}
            </button>
          )}
        </div>
      </div>

      {summary ? (
        <p className="mt-2 text-sm leading-relaxed text-foreground/85">
          {summary}
        </p>
      ) : (
        !loading && (
          <p className="mt-2 text-xs text-muted-foreground italic">
            No summary yet. Click summarize to fetch the page and generate one.
          </p>
        )
      )}

      {summarizedAt && (
        <p className="mt-2 text-[0.6rem] text-muted-foreground">
          summarized {summarizedAt}
        </p>
      )}
      {err && <p className="mt-2 text-xs text-red-800 italic">{err}</p>}
    </div>
  );
}
