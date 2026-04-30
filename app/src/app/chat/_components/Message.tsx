"use client";

import { useState } from "react";
import ClientMarkdown from "@/app/status/_components/ClientMarkdown";

// ─── types ──────────────────────────────────────────────────────────────────

export type MessagePart =
  | { kind: "text"; text: string }
  | {
      kind: "tool";
      id: string;
      name: string;
      args: Record<string, unknown> | string;
      output?: string;
      truncated?: boolean;
      fullLength?: number;
      running: boolean;
    };

export interface ChatMessage {
  role: "user" | "assistant";
  parts: MessagePart[];
}

// ─── thinking dots (animated) ───────────────────────────────────────────────

function ThinkingDots() {
  return (
    <span aria-label="thinking" className="inline-flex items-center gap-1">
      <span className="block h-1.5 w-1.5 rounded-full bg-primary/70 animate-thinking-1" />
      <span className="block h-1.5 w-1.5 rounded-full bg-primary/70 animate-thinking-2" />
      <span className="block h-1.5 w-1.5 rounded-full bg-primary/70 animate-thinking-3" />
    </span>
  );
}

// ─── tool card ──────────────────────────────────────────────────────────────

function previewArgs(args: Record<string, unknown> | string): string {
  if (typeof args === "string") return args.slice(0, 120);
  const entries = Object.entries(args).slice(0, 4);
  if (entries.length === 0) return "";
  return entries
    .map(([k, v]) => {
      const s = typeof v === "string" ? v : JSON.stringify(v);
      return `${k}: ${s.length > 80 ? s.slice(0, 77) + "…" : s}`;
    })
    .join(" · ");
}

function ToolCard({ part }: { part: Extract<MessagePart, { kind: "tool" }> }) {
  const [open, setOpen] = useState(false);
  const preview = previewArgs(part.args);
  const dotClass = part.running
    ? "bg-amber-400 animate-pulse"
    : "bg-emerald-500";

  return (
    <div className="border border-border bg-secondary/60 px-3 py-2 text-xs">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex w-full items-center gap-2 text-left"
      >
        <span className={`inline-block h-2 w-2 rounded-full ${dotClass}`} />
        <span className="font-smallcaps tracking-[0.08em] text-[0.65rem] text-muted-foreground">
          tool
        </span>
        <code className="font-mono text-foreground/90">{part.name}</code>
        {preview && (
          <span className="ml-1 truncate font-mono text-muted-foreground">
            {preview}
          </span>
        )}
        <span className="ml-auto text-muted-foreground">
          {open ? "▾" : "▸"}
        </span>
      </button>

      {open && (
        <div className="mt-2 grid gap-2">
          <div>
            <div className="font-smallcaps text-[0.55rem] tracking-[0.12em] text-muted-foreground">
              args
            </div>
            <pre className="mt-1 max-h-48 overflow-auto whitespace-pre-wrap break-words border border-border bg-card p-2 font-mono text-[0.7rem] leading-snug">
              {typeof part.args === "string"
                ? part.args
                : JSON.stringify(part.args, null, 2)}
            </pre>
          </div>
          {part.running && !part.output ? (
            <div className="flex items-center gap-2 text-muted-foreground">
              <ThinkingDots />
              <span className="italic">running…</span>
            </div>
          ) : part.output !== undefined ? (
            <div>
              <div className="font-smallcaps text-[0.55rem] tracking-[0.12em] text-muted-foreground">
                output
                {part.truncated && part.fullLength != null && (
                  <span className="ml-1 normal-case tracking-normal italic">
                    (truncated · full {part.fullLength.toLocaleString()} chars)
                  </span>
                )}
              </div>
              <pre className="mt-1 max-h-64 overflow-auto whitespace-pre-wrap break-words border border-border bg-card p-2 font-mono text-[0.7rem] leading-snug">
                {part.output}
              </pre>
            </div>
          ) : null}
        </div>
      )}
    </div>
  );
}

// ─── message ────────────────────────────────────────────────────────────────

export default function Message({
  message,
  streaming,
}: {
  message: ChatMessage;
  streaming?: boolean;
}) {
  if (message.role === "user") {
    const text = message.parts.map((p) => (p.kind === "text" ? p.text : "")).join("");
    return (
      <div className="flex justify-end">
        <div className="max-w-[85%] whitespace-pre-wrap border border-primary/20 bg-accent px-4 py-2 text-sm">
          {text}
        </div>
      </div>
    );
  }

  // Assistant — render parts in order.
  const hasAnyContent = message.parts.some(
    (p) => (p.kind === "text" && p.text.length > 0) || p.kind === "tool",
  );

  return (
    <div className="flex flex-col items-start gap-2">
      <div className="font-smallcaps text-[0.6rem] tracking-[0.15em] text-primary">
        The Editor
      </div>

      <div className="w-full max-w-full space-y-2 border border-border bg-card px-4 py-3">
        {message.parts.map((part, i) => {
          if (part.kind === "tool") {
            return <ToolCard key={i} part={part} />;
          }
          if (!part.text) return null;
          return (
            <ClientMarkdown
              key={i}
              source={part.text}
              className="prose-broadsheet prose-wide prose-compact"
            />
          );
        })}

        {streaming && !hasAnyContent && (
          <div className="flex items-center gap-2 text-muted-foreground">
            <ThinkingDots />
            <span className="text-xs italic">thinking…</span>
          </div>
        )}

        {streaming &&
          hasAnyContent &&
          message.parts[message.parts.length - 1]?.kind === "text" && (
            <span className="ml-1 inline-block h-3 w-1.5 animate-pulse bg-primary/60 align-middle" />
          )}
      </div>
    </div>
  );
}
