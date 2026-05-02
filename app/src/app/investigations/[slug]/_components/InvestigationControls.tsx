"use client";

import Link from "next/link";
import { useState, useTransition } from "react";
import {
  pauseInvestigationAction,
  resumeInvestigationAction,
  runInvestigationNowAction,
  triggerInvestigationAction,
} from "../actions";

type Action = "pause" | "resume" | "trigger" | "run-now";

export interface InvestigationControlsProps {
  slug: string;
  status: string; // "active" | "paused" | "complete" | "archived" | ...
  cronJobId?: string | null;
}

export function InvestigationControls({
  slug,
  status,
  cronJobId,
}: InvestigationControlsProps) {
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);
  const [runsHref, setRunsHref] = useState<string | null>(null);
  const [isPending, startTransition] = useTransition();

  function call(action: Action) {
    setError(null);
    setSuccess(null);
    setRunsHref(null);
    const fd = new FormData();
    fd.set("slug", slug);

    // Capture click time so the runs page can filter to "new since this click."
    const clickedAtMs = Date.now();

    startTransition(async () => {
      try {
        const result =
          action === "pause"
            ? await pauseInvestigationAction(fd)
            : action === "resume"
              ? await resumeInvestigationAction(fd)
              : action === "run-now"
                ? await runInvestigationNowAction(fd)
                : await triggerInvestigationAction(fd);

        if (action === "trigger") {
          setSuccess(
            "Trigger queued. The investigator will pick this up on its next tick (≤ ~4h).",
          );
        } else if (action === "run-now") {
          setSuccess(
            "Running now in the background. Open the run viewer to watch reasoning + tool calls live.",
          );
          // Build a /runs filter that surfaces sessions started after this click.
          const params = new URLSearchParams({
            profile: "investigator",
            since: String(clickedAtMs - 5_000), // small back-buffer for clock skew
          });
          if (cronJobId) params.set("cronJobId", cronJobId);
          setRunsHref(`/runs?${params.toString()}`);
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

  // Static "view runs" link — always available, filtered to this investigation
  // when we know the cron_job_id.
  const allRunsHref = cronJobId
    ? `/runs?profile=investigator&cronJobId=${encodeURIComponent(cronJobId)}`
    : `/runs?profile=investigator`;

  return (
    <div className="space-y-2">
      <div className="flex flex-wrap gap-2">
        {!isTerminal && (
          <button
            type="button"
            disabled={isPending}
            onClick={() => call("run-now")}
            title="Fire the investigator immediately. Runs in the background."
            className="border border-emerald-500/50 bg-emerald-50 px-3 py-1.5 text-sm text-emerald-800 transition hover:bg-emerald-100 font-smallcaps tracking-wider disabled:opacity-50"
          >
            {isPending ? "Working…" : "▶ Run Now"}
          </button>
        )}
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
            title="Drop a request into the investigator's inbox. Queued for the next tick."
            className="border border-primary/40 bg-primary/5 px-3 py-1.5 text-sm text-primary transition hover:bg-primary/10 font-smallcaps tracking-wider disabled:opacity-50"
          >
            Queue Trigger
          </button>
        )}
        <Link
          href={allRunsHref}
          className="border border-border bg-card px-3 py-1.5 text-sm text-foreground transition hover:border-primary/40 hover:text-primary font-smallcaps tracking-wider"
        >
          📜 Run history
        </Link>
      </div>
      {success && (
        <div className="border border-emerald-300 bg-emerald-50 p-2 text-xs text-emerald-700 whitespace-pre-wrap">
          {success}
          {runsHref && (
            <>
              {" "}
              <Link
                href={runsHref}
                className="font-semibold underline hover:text-emerald-900"
              >
                Open the new run →
              </Link>
            </>
          )}
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
