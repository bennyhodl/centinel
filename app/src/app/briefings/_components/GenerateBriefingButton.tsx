"use client";

import Link from "next/link";
import { useState, useTransition } from "react";
import { generateBriefingNowAction } from "../actions";

export function GenerateBriefingButton({ compact = false }: { compact?: boolean }) {
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);
  const [isPending, startTransition] = useTransition();

  function fire() {
    setError(null);
    setSuccess(null);
    startTransition(async () => {
      try {
        await generateBriefingNowAction();
        setSuccess(
          "Generating now in the background. Watch the Status page for progress — this typically takes 1–3 minutes. The new briefing will appear here when it's ready.",
        );
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      }
    });
  }

  return (
    <div className={compact ? "" : "space-y-3"}>
      <div className="flex flex-wrap items-center gap-2">
        <button
          type="button"
          onClick={fire}
          disabled={isPending}
          className="border border-emerald-500/50 bg-emerald-50 px-3 py-1.5 text-sm text-emerald-800 transition hover:bg-emerald-100 font-smallcaps tracking-wider disabled:opacity-50"
        >
          {isPending ? "Working…" : "▶ Generate Briefing Now"}
        </button>
        <Link
          href="/status"
          className="text-xs text-primary hover:underline italic"
        >
          watch progress on /status →
        </Link>
      </div>
      {success && (
        <div className="border border-emerald-300 bg-emerald-50 p-2 text-xs text-emerald-700">
          {success}
        </div>
      )}
      {error && (
        <div className="border border-destructive/40 bg-destructive/5 p-2 text-xs text-destructive whitespace-pre-wrap">
          {error}
        </div>
      )}
    </div>
  );
}
