"use client";

import { useEffect, useRef, useState } from "react";
import type { SessionDoc, SessionMessage } from "@/lib/sessions";

type FullDoc = SessionDoc;

const ROLE_TONE: Record<string, string> = {
  user: "border-zinc-300 bg-zinc-50",
  assistant: "border-cyan-300/50 bg-cyan-50/40",
  system: "border-amber-300/50 bg-amber-50/40",
  tool: "border-emerald-300/50 bg-emerald-50/40",
};

function relTime(input: string | undefined): string {
  if (!input) return "";
  const t = Date.parse(input);
  if (Number.isNaN(t)) return "";
  const dt = Date.now() - t;
  if (dt < 60_000) return `${Math.floor(dt / 1000)}s ago`;
  if (dt < 3_600_000) return `${Math.floor(dt / 60_000)}m ago`;
  return `${Math.floor(dt / 3_600_000)}h ago`;
}

/**
 * Polling-based live session viewer. We poll /api/sessions/[id] every 3s
 * and only re-render when the message count grew. Stop polling when the
 * session hasn't grown for 60s (assumed complete) — operator can refresh
 * the page to resume polling if they want.
 */
export function RunViewer({
  id,
  initialDoc,
}: {
  id: string;
  initialDoc: FullDoc;
}) {
  const [doc, setDoc] = useState<FullDoc>(initialDoc);
  const [polling, setPolling] = useState(true);
  const lastGrownAt = useRef<number>(Date.now());
  const lastCount = useRef<number>(initialDoc.messages.length);

  useEffect(() => {
    if (!polling) return;
    const interval = setInterval(async () => {
      try {
        const res = await fetch(`/api/sessions/${encodeURIComponent(id)}`, {
          cache: "no-store",
        });
        if (!res.ok) return;
        const next = (await res.json()) as FullDoc;
        if (next.messages.length !== lastCount.current) {
          lastCount.current = next.messages.length;
          lastGrownAt.current = Date.now();
          setDoc(next);
        } else if (Date.now() - lastGrownAt.current > 60_000) {
          // 60s of no growth → assume the session is done
          setPolling(false);
        }
      } catch {
        /* ignore transient */
      }
    }, 3_000);
    return () => clearInterval(interval);
  }, [id, polling]);

  return (
    <div className="space-y-3">
      <div className="flex items-center gap-2 text-xs">
        <span
          className={`relative inline-flex h-2 w-2 ${
            polling ? "" : "opacity-40"
          }`}
        >
          {polling && (
            <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-emerald-400 opacity-60" />
          )}
          <span
            className={`relative inline-flex h-2 w-2 rounded-full ${
              polling ? "bg-emerald-500" : "bg-zinc-500"
            }`}
          />
        </span>
        <span className="font-smallcaps tracking-[0.12em] text-muted-foreground uppercase">
          {polling ? "live · polling every 3s" : "polling stopped"}
        </span>
        {!polling && (
          <button
            type="button"
            onClick={() => {
              lastGrownAt.current = Date.now();
              setPolling(true);
            }}
            className="ml-2 text-primary hover:underline italic"
          >
            resume
          </button>
        )}
        {doc.lastUpdated && (
          <>
            <span className="opacity-40">·</span>
            <span className="text-muted-foreground">
              last update {relTime(doc.lastUpdated)}
            </span>
          </>
        )}
      </div>

      {doc.messages.length === 0 ? (
        <div className="border border-dashed border-border bg-card p-6 text-center text-sm text-muted-foreground italic">
          No messages in this session yet.
        </div>
      ) : (
        <ol className="space-y-2">
          {doc.messages.map((m) => (
            <MessageCard key={m.index} m={m} />
          ))}
        </ol>
      )}
    </div>
  );
}

function MessageCard({ m }: { m: SessionMessage }) {
  const tone = ROLE_TONE[m.role] ?? "border-border bg-card";

  if (m.isToolUse) {
    return (
      <li className="border border-violet-300/40 bg-violet-50/30 px-3 py-2">
        <div className="flex flex-wrap items-baseline gap-2 text-[0.7rem]">
          <span className="font-smallcaps tracking-[0.12em] text-violet-700 uppercase">
            #{m.index} tool call
          </span>
          <code className="font-mono text-violet-900 font-semibold">
            {m.toolName ?? "?"}
          </code>
        </div>
        {m.toolArgs !== undefined && (
          <details className="mt-1.5">
            <summary className="cursor-pointer text-xs text-muted-foreground hover:text-foreground">
              args
            </summary>
            <pre className="mt-1 max-h-64 overflow-auto whitespace-pre-wrap break-words border border-border bg-card p-2 font-mono text-[0.7rem] leading-snug">
              {typeof m.toolArgs === "string"
                ? m.toolArgs
                : JSON.stringify(m.toolArgs, null, 2)}
            </pre>
          </details>
        )}
        {m.textContent && (
          <p className="mt-1 text-xs text-foreground/80 italic">
            {m.textContent}
          </p>
        )}
      </li>
    );
  }

  if (m.isToolResult) {
    return (
      <li className="border border-emerald-300/40 bg-emerald-50/30 px-3 py-2">
        <div className="flex items-baseline gap-2 text-[0.7rem]">
          <span className="font-smallcaps tracking-[0.12em] text-emerald-800 uppercase">
            #{m.index} result
          </span>
        </div>
        {m.toolResult && (
          <details className="mt-1.5" open={m.toolResult.length < 600}>
            <summary className="cursor-pointer text-xs text-muted-foreground hover:text-foreground">
              {m.toolResult.length > 600
                ? `output · ${m.toolResult.length.toLocaleString()} chars`
                : "output"}
            </summary>
            <pre className="mt-1 max-h-80 overflow-auto whitespace-pre-wrap break-words border border-border bg-card p-2 font-mono text-[0.7rem] leading-snug">
              {m.toolResult}
            </pre>
          </details>
        )}
      </li>
    );
  }

  return (
    <li className={`border ${tone} px-3 py-2`}>
      <div className="flex items-baseline gap-2 text-[0.7rem]">
        <span className="font-smallcaps tracking-[0.12em] text-muted-foreground uppercase">
          #{m.index} {m.role}
        </span>
        {m.finishReason && (
          <span className="font-smallcaps tracking-[0.1em] text-[0.55rem] text-muted-foreground">
            {m.finishReason}
          </span>
        )}
      </div>
      {m.reasoning && (
        <details className="mt-1.5">
          <summary className="cursor-pointer text-xs text-muted-foreground italic hover:text-foreground">
            reasoning
          </summary>
          <pre className="mt-1 max-h-64 overflow-auto whitespace-pre-wrap break-words border border-border bg-card p-2 font-mono text-[0.7rem] leading-snug text-muted-foreground">
            {m.reasoning}
          </pre>
        </details>
      )}
      {m.textContent && (
        <pre className="mt-1.5 whitespace-pre-wrap break-words font-serif text-sm text-foreground/90 leading-relaxed">
          {m.textContent}
        </pre>
      )}
    </li>
  );
}
