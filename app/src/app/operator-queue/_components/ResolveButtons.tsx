"use client";

import Link from "next/link";
import { useState, useTransition } from "react";
import { resolveItemAction } from "../actions";

type Bucket =
  | "entity-merges"
  | "watch-tuning"
  | "findings-draft-aging"
  | "broken-watches";

type Decision = "approve" | "reject" | "dismiss" | "acknowledge" | "snooze";

interface ButtonSpec {
  decision: Decision;
  label: string;
  className: string;
  needsConfirm: boolean;
  needsAgent?: boolean;
  description?: string;
}

const APPROVE: ButtonSpec = {
  decision: "approve",
  label: "Approve",
  className:
    "border border-emerald-400/40 bg-emerald-50 text-emerald-700 hover:bg-emerald-100",
  needsConfirm: true,
  needsAgent: true,
  description: "Sends a directive to the responsible agent to perform the work.",
};

const REJECT: ButtonSpec = {
  decision: "reject",
  label: "Reject",
  className:
    "border border-red-300 bg-red-50 text-red-700 hover:bg-red-100",
  needsConfirm: true,
  description: "Closes this item; agents will not re-flag the same condition.",
};

const DISMISS: ButtonSpec = {
  decision: "dismiss",
  label: "Dismiss",
  className:
    "border border-zinc-300 bg-zinc-50 text-zinc-700 hover:bg-zinc-100",
  needsConfirm: true,
  description: "Closes this item without action.",
};

const ACKNOWLEDGE: ButtonSpec = {
  decision: "acknowledge",
  label: "Acknowledge",
  className:
    "border border-cyan-300 bg-cyan-50 text-cyan-700 hover:bg-cyan-100",
  needsConfirm: false,
  description: "Marks as seen; agent picks up from `acknowledged` next tick.",
};

const SNOOZE: ButtonSpec = {
  decision: "snooze",
  label: "Snooze 7d",
  className:
    "border border-amber-300 bg-amber-50 text-amber-700 hover:bg-amber-100",
  needsConfirm: false,
  description: "Hides until 7 days from now.",
};

const BUTTONS_BY_BUCKET: Record<Bucket, ButtonSpec[]> = {
  "entity-merges": [APPROVE, REJECT, SNOOZE],
  "watch-tuning": [APPROVE, REJECT, SNOOZE],
  "findings-draft-aging": [DISMISS, SNOOZE],
  "broken-watches": [ACKNOWLEDGE, DISMISS, SNOOZE],
};

function snoozeIso(days: number): string {
  const d = new Date();
  d.setDate(d.getDate() + days);
  return d.toISOString().slice(0, 10);
}

export interface ResolveButtonsProps {
  bucket: Bucket;
  slug: string;
  status: string;
}

export function ResolveButtons({ bucket, slug, status }: ResolveButtonsProps) {
  const [pendingDecision, setPendingDecision] = useState<Decision | null>(null);
  const [reason, setReason] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);
  const [isPending, startTransition] = useTransition();

  // Already-resolved items: surface status, no buttons.
  if (status !== "open" && status !== "snoozed") {
    return (
      <div className="text-[11px] uppercase tracking-wider text-muted-foreground">
        resolved · <span className="font-mono">{status}</span>
      </div>
    );
  }

  const buttons = BUTTONS_BY_BUCKET[bucket] ?? [];

  function submit(decision: Decision) {
    setError(null);
    setSuccess(null);
    const fd = new FormData();
    fd.set("bucket", bucket);
    fd.set("slug", slug);
    fd.set("decision", decision);
    if (reason.trim()) fd.set("reason", reason.trim());
    if (decision === "snooze") fd.set("snoozeUntil", snoozeIso(7));

    startTransition(async () => {
      try {
        const res = await resolveItemAction(fd);
        const msg = res.needsAgent
          ? `Marked ${res.status}. Directive queued for agent — see /status feed.`
          : `Marked ${res.status}.`;
        setSuccess(msg);
        setPendingDecision(null);
        setReason("");
      } catch (e) {
        const m = e instanceof Error ? e.message : String(e);
        setError(m);
      }
    });
  }

  if (pendingDecision) {
    const spec = buttons.find((b) => b.decision === pendingDecision);
    return (
      <div className="space-y-2 border border-border bg-secondary p-3">
        <div className="text-xs">
          <strong className="font-smallcaps tracking-wider">
            {spec?.label}?
          </strong>
          {spec?.description && (
            <span className="ml-1 text-muted-foreground">
              {spec.description}
            </span>
          )}
        </div>
        <textarea
          value={reason}
          onChange={(e) => setReason(e.target.value)}
          placeholder="Optional note (audit trail)"
          rows={2}
          className="w-full border border-border bg-background px-2 py-1 text-xs font-mono"
          disabled={isPending}
        />
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={() => submit(pendingDecision)}
            disabled={isPending}
            className={`px-3 py-1 text-xs font-smallcaps tracking-wider transition disabled:opacity-50 ${spec?.className}`}
          >
            {isPending ? "Submitting…" : `Confirm ${spec?.label}`}
          </button>
          <button
            type="button"
            onClick={() => {
              setPendingDecision(null);
              setReason("");
            }}
            disabled={isPending}
            className="border border-border bg-background px-3 py-1 text-xs font-smallcaps tracking-wider disabled:opacity-50"
          >
            Cancel
          </button>
        </div>
        {error && (
          <div className="border border-destructive/40 bg-destructive/5 p-2 text-xs text-destructive">
            {error}
          </div>
        )}
      </div>
    );
  }

  return (
    <div className="space-y-2">
      <div className="flex flex-wrap gap-1.5">
        {bucket === "findings-draft-aging" && (
          <Link
            href={`/findings/${slug}`}
            className="border border-primary/40 bg-primary/5 px-3 py-1 text-xs text-primary transition hover:bg-primary/10 font-smallcaps tracking-wider"
          >
            Review draft →
          </Link>
        )}
        {buttons.map((b) => (
          <button
            key={b.decision}
            type="button"
            disabled={isPending}
            onClick={() => {
              if (b.needsConfirm) {
                setPendingDecision(b.decision);
              } else {
                submit(b.decision);
              }
            }}
            title={b.description}
            className={`px-3 py-1 text-xs font-smallcaps tracking-wider transition disabled:opacity-50 ${b.className}`}
          >
            {b.label}
          </button>
        ))}
      </div>
      {success && (
        <div className="border border-emerald-300 bg-emerald-50 p-2 text-xs text-emerald-700">
          {success}
        </div>
      )}
      {error && (
        <div className="border border-destructive/40 bg-destructive/5 p-2 text-xs text-destructive">
          {error}
        </div>
      )}
    </div>
  );
}
