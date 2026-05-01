"use client";

import { useRouter } from "next/navigation";
import { useState, useTransition } from "react";
import type { SitemapEntry } from "@/lib/sitemap";

export function LeafActions({
  entry,
  investigations,
}: {
  entry: SitemapEntry;
  investigations: { slug: string; title: string }[];
}) {
  const router = useRouter();
  const [pending, startTransition] = useTransition();
  const [seedSlug, setSeedSlug] = useState("");
  const [seedNote, setSeedNote] = useState("");
  const [showSeed, setShowSeed] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);

  async function triage(next: "active" | "excluded") {
    setErr(null);
    setMsg(null);
    const res = await fetch("/api/sitemap/triage", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ url: entry.url, status: next }),
    });
    if (!res.ok) {
      setErr(`triage failed: ${res.status}`);
      return;
    }
    setMsg(`marked ${next}`);
    startTransition(() => router.refresh());
  }

  async function seed() {
    setErr(null);
    setMsg(null);
    if (!seedSlug) {
      setErr("pick an investigation");
      return;
    }
    const res = await fetch("/api/sitemap/seed", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        url: entry.url,
        investigation_slug: seedSlug,
        note: seedNote,
      }),
    });
    if (!res.ok) {
      setErr(`seed failed: ${res.status}`);
      return;
    }
    setMsg(`seeded into ${seedSlug}`);
    setSeedSlug("");
    setSeedNote("");
    setShowSeed(false);
    startTransition(() => router.refresh());
  }

  function chatAboutPage(refresh: boolean) {
    const sessionId = `sitemap:${slugifyUrl(entry.url)}`;
    const params = new URLSearchParams({
      session: sessionId,
      seedUrl: entry.url,
    });
    if (refresh) params.set("refresh", "1");
    window.location.href = `/chat?${params.toString()}`;
  }

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap gap-2 text-xs">
        {entry.status === "needs_review" && (
          <>
            <button
              type="button"
              disabled={pending}
              onClick={() => triage("active")}
              className="border border-emerald-700/40 bg-emerald-50 px-3 py-1.5 text-emerald-900 hover:bg-emerald-100 disabled:opacity-50"
            >
              ✓ approve
            </button>
            <button
              type="button"
              disabled={pending}
              onClick={() => triage("excluded")}
              className="border border-foreground/20 bg-secondary px-3 py-1.5 text-foreground/70 hover:bg-foreground/10 disabled:opacity-50"
            >
              ✗ exclude
            </button>
          </>
        )}
        <button
          type="button"
          onClick={() => setShowSeed((v) => !v)}
          className="border border-primary/40 bg-primary/5 px-3 py-1.5 text-primary hover:bg-primary/10"
        >
          + seed investigation
        </button>
        <button
          type="button"
          onClick={() => chatAboutPage(false)}
          className="border border-border bg-card px-3 py-1.5 text-foreground hover:bg-accent"
        >
          💬 chat about this page
        </button>
        <button
          type="button"
          onClick={() => chatAboutPage(true)}
          className="border border-border bg-card px-3 py-1.5 text-foreground hover:bg-accent"
          title="Re-fetch from Tavily before opening chat"
        >
          ⟳ refresh & chat
        </button>
      </div>

      {showSeed && (
        <div className="border border-primary/20 bg-primary/5 p-3 space-y-2">
          <select
            value={seedSlug}
            onChange={(e) => setSeedSlug(e.target.value)}
            className="w-full border border-border bg-card px-2 py-1.5 text-sm"
          >
            <option value="">— pick an investigation —</option>
            {investigations.map((i) => (
              <option key={i.slug} value={i.slug}>
                {i.title} ({i.slug})
              </option>
            ))}
          </select>
          <input
            type="text"
            value={seedNote}
            onChange={(e) => setSeedNote(e.target.value)}
            placeholder="optional note: why this URL matters"
            className="w-full border border-border bg-card px-2 py-1.5 text-sm"
          />
          <div className="flex gap-2">
            <button
              type="button"
              onClick={seed}
              className="bg-primary px-3 py-1.5 text-xs text-primary-foreground hover:opacity-90"
            >
              seed
            </button>
            <button
              type="button"
              onClick={() => setShowSeed(false)}
              className="text-xs text-muted-foreground hover:text-foreground italic"
            >
              cancel
            </button>
          </div>
        </div>
      )}

      {msg && <p className="text-xs text-emerald-800 italic">{msg}</p>}
      {err && <p className="text-xs text-red-800 italic">{err}</p>}
    </div>
  );
}

function slugifyUrl(url: string): string {
  try {
    const u = new URL(url);
    return `${u.host}${u.pathname}`.replace(/[^a-zA-Z0-9/-]/g, "-").replace(/-+/g, "-");
  } catch {
    return url.replace(/[^a-zA-Z0-9/-]/g, "-");
  }
}
