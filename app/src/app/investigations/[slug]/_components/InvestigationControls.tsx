"use client";

import { useState, useTransition } from "react";
import {
  pauseInvestigationAction,
  resumeInvestigationAction,
  triggerInvestigationAction,
} from "../actions";

type Action = "pause" | "resume" | "trigger";

export interface InvestigationControlsProps {
  slug: string;
  status: string; // "active" | "paused" | "complete" | "archived" | ...
}

export function InvestigationControls({
  slug,
  status,
}: InvestigationControlsProps) {
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);
  const [isPending, startTransition] = useTransition();

  function call(action: Action) {
    setError(null);
    setSuccess(null);
    const fd = new FormData();
    fd.set("slug", slug);

    startTransition(async () => {
      try {
        const result =
          action === "pause"
            ? await pauseInvestigationAction(fd)
            : action === "resume"
              ? await resumeInvestigationAction(fd)
              : await triggerInvestigationAction(fd);

        if (action === "trigger") {
          setSuccess(
            "Trigger queued. Investigator will pick this up on its next tick (≤ ~4h). For an immediate run, use centinel-investigator from the terminal.",
          );
        } else {
          setSuccess(result.output.trim().split("\n").slice(-3).join("\n"));
        }
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        setError(msg);
      }
    });
  }

  const isActive = status === "active";
  const isPaused = status === "paused";
  const isTerminal = status === "complete" || status === "archived";

  return (
    <div className="space-y-2">
      <div className="flex flex-wrap gap-2">
        {!isTerminal && isActive && (
          <button
            type="button"
            disabled={isPending}
            onClick={() => call("pause")}
            className="border border-amber-300 bg-amber-50 px-3 py-1.5 text-sm text-amber-700 transition hover:bg-amber-100 font-smallcaps tracking-wider disabled:opacity-50"
          >
            Pause
          </button>
        )}
        {!isTerminal && isPaused && (
          <button
            type="button"
            disabled={isPending}
            onClick={() => call("resume")}
            className="border border-emerald-400/40 bg-emerald-50 px-3 py-1.5 text-sm text-emerald-700 transition hover:bg-emerald-100 font-smallcaps tracking-wider disabled:opacity-50"
          >
            Resume
          </button>
        )}
        {!isTerminal && (
          <button
            type="button"
            disabled={isPending}
            onClick={() => call("trigger")}
            title="Drops a request into the investigator's inbox; runs on next cron tick."
            className="border border-primary/40 bg-primary/5 px-3 py-1.5 text-sm text-primary transition hover:bg-primary/10 font-smallcaps tracking-wider disabled:opacity-50"
          >
            {isPending ? "Working…" : "Trigger Run"}
          </button>
        )}
      </div>
      {success && (
        <div className="border border-emerald-300 bg-emerald-50 p-2 text-xs text-emerald-700 whitespace-pre-wrap">
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
