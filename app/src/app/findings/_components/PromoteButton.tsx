"use client";

import { useState, useTransition } from "react";
import { promoteFindingAction } from "../actions";

export function PromoteButton({ slug }: { slug: string }) {
  const [error, setError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [isPending, startTransition] = useTransition();

  function handleSubmit(formData: FormData) {
    setError(null);
    startTransition(async () => {
      try {
        await promoteFindingAction(formData);
      } catch (e) {
        const message = e instanceof Error ? e.message : String(e);
        // next/navigation redirect throws a special error — let it propagate.
        if (message.includes("NEXT_REDIRECT")) throw e;
        setError(message);
        setConfirming(false);
      }
    });
  }

  if (!confirming) {
    return (
      <button
        type="button"
        onClick={() => setConfirming(true)}
        className="border border-emerald-400/40 bg-emerald-50 px-3 py-1.5 text-sm text-emerald-700 transition hover:bg-emerald-100 font-smallcaps tracking-wider"
      >
        Promote to Published
      </button>
    );
  }

  return (
    <div className="space-y-2">
      <p className="text-xs text-amber-800">
        This moves the file from <code>Findings/draft/</code> to{" "}
        <code>Findings/published/</code> and stamps{" "}
        <code>published_at</code>. Auto-publishes immediately. Continue?
      </p>
      <form action={handleSubmit} className="flex items-center gap-2">
        <input type="hidden" name="slug" value={slug} />
        <button
          type="submit"
          disabled={isPending}
          className="border border-emerald-400/60 bg-emerald-100 px-3 py-1.5 text-sm text-emerald-800 transition hover:bg-emerald-200 font-smallcaps tracking-wider disabled:opacity-50"
        >
          {isPending ? "Publishing…" : "Yes, publish"}
        </button>
        <button
          type="button"
          onClick={() => setConfirming(false)}
          disabled={isPending}
          className="border border-border bg-background px-3 py-1.5 text-sm font-smallcaps tracking-wider disabled:opacity-50"
        >
          Cancel
        </button>
      </form>
      {error && (
        <div className="border border-destructive/40 bg-destructive/5 p-2 text-xs text-destructive">
          {error}
        </div>
      )}
    </div>
  );
}
