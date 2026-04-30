"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import Message, { type ChatMessage, type MessagePart } from "./Message";
import Composer from "./Composer";

/**
 * Chat client — sends the latest user message to /chat/api and streams an
 * NDJSON response that mixes assistant content deltas with tool-call /
 * tool-output frames. The client builds structured `MessagePart[]` per
 * message so tools render as proper styled cards rather than inline italics.
 */
const DEFAULT_SESSION_ID = "centinel-web-chat";
const SESSION_STORAGE_KEY = "centinel.chat.sessionId";

export default function ChatClient({ intro }: { intro: string }) {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState("");
  const [isStreaming, setIsStreaming] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [historyLoaded, setHistoryLoaded] = useState(false);
  // Session id is persistent across page reloads via localStorage so the
  // browser keeps talking to the same Hermes session it was last using.
  const [sessionId, setSessionId] = useState<string>(DEFAULT_SESSION_ID);
  const abortRef = useRef<AbortController | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);

  // On mount: pick up any persisted session id, then load its history.
  useEffect(() => {
    let cancelled = false;
    let activeSid = DEFAULT_SESSION_ID;
    try {
      const saved = localStorage.getItem(SESSION_STORAGE_KEY);
      if (saved && saved.length > 0 && saved.length <= 128) {
        activeSid = saved;
      }
    } catch {
      /* SSR / privacy modes — ignore */
    }
    setSessionId(activeSid);

    (async () => {
      try {
        const res = await fetch(
          `/chat/api/history?sessionId=${encodeURIComponent(activeSid)}`,
          { cache: "no-store" },
        );
        if (!res.ok) return;
        const j = (await res.json()) as {
          messages?: { role: "user" | "assistant"; content: string }[];
        };
        if (cancelled) return;
        const restored = (j.messages ?? []).map(
          (m) =>
            ({
              role: m.role,
              parts: [{ kind: "text", text: m.content }],
            }) as ChatMessage,
        );
        if (restored.length > 0) setMessages(restored);
      } catch {
        // soft-fail
      } finally {
        if (!cancelled) setHistoryLoaded(true);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const startNewConversation = useCallback(() => {
    if (isStreaming) return;
    const fresh = `${DEFAULT_SESSION_ID}-${Date.now().toString(36)}`;
    setSessionId(fresh);
    setMessages([]);
    setError(null);
    try {
      localStorage.setItem(SESSION_STORAGE_KEY, fresh);
    } catch {
      /* ignore */
    }
  }, [isStreaming]);

  // Auto-scroll to bottom whenever content grows. Snap is fine here — the
  // user is following live output.
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
  }, [messages, isStreaming]);

  const stop = useCallback(() => {
    abortRef.current?.abort();
    abortRef.current = null;
    setIsStreaming(false);
  }, []);

  /** Mutate the last assistant message's parts in place. */
  const updateAssistant = useCallback(
    (mutate: (parts: MessagePart[]) => MessagePart[]) => {
      setMessages((prev) => {
        const copy = prev.slice();
        const last = copy[copy.length - 1];
        if (!last || last.role !== "assistant") return prev;
        copy[copy.length - 1] = { ...last, parts: mutate(last.parts) };
        return copy;
      });
    },
    [],
  );

  /** Append text to the last text part, or start a new text part if the
   * last part is a tool. */
  const appendDelta = useCallback(
    (text: string) => {
      updateAssistant((parts) => {
        const last = parts[parts.length - 1];
        if (last && last.kind === "text") {
          const next = parts.slice(0, -1);
          next.push({ kind: "text", text: last.text + text });
          return next;
        }
        return [...parts, { kind: "text", text }];
      });
    },
    [updateAssistant],
  );

  const handleEvent = useCallback(
    (ev: unknown) => {
      if (!ev || typeof ev !== "object") return;
      const e = ev as Record<string, unknown>;
      switch (e.type) {
        case "delta": {
          if (typeof e.text === "string") appendDelta(e.text);
          break;
        }
        case "tool_call": {
          updateAssistant((parts) => [
            ...parts,
            {
              kind: "tool",
              id: String(e.id ?? Math.random()),
              name: String(e.name ?? "tool"),
              args: (e.args as Record<string, unknown> | string) ?? {},
              running: true,
            },
          ]);
          break;
        }
        case "tool_output": {
          const id = String(e.id ?? "");
          updateAssistant((parts) =>
            parts.map((p) => {
              if (p.kind !== "tool") return p;
              if (p.id !== id && id !== "") return p;
              // Match by id, OR (when id is empty) the last running tool.
              return {
                ...p,
                running: false,
                output: typeof e.text === "string" ? e.text : "",
                truncated: Boolean(e.truncated),
                fullLength:
                  typeof e.fullLength === "number" ? e.fullLength : undefined,
              };
            }),
          );
          break;
        }
        case "error": {
          if (typeof e.message === "string") setError(e.message);
          break;
        }
        case "done":
          break;
      }
    },
    [appendDelta, updateAssistant],
  );

  const send = useCallback(async () => {
    const text = input.trim();
    if (!text || isStreaming) return;

    setError(null);
    setInput("");

    const next: ChatMessage[] = [
      ...messages,
      { role: "user", parts: [{ kind: "text", text }] },
      { role: "assistant", parts: [] },
    ];
    setMessages(next);
    setIsStreaming(true);

    const controller = new AbortController();
    abortRef.current = controller;

    try {
      const res = await fetch("/chat/api", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          messages: [{ role: "user", content: text }],
          sessionId,
        }),
        signal: controller.signal,
      });

      if (!res.ok || !res.body) {
        let detail = `HTTP ${res.status}`;
        try {
          const j = await res.json();
          if (j?.error) detail = `${j.error}${j.detail ? `: ${j.detail}` : ""}`;
        } catch {
          /* ignore */
        }
        setError(detail);
        setMessages((m) => m.slice(0, -1)); // drop empty assistant
        setIsStreaming(false);
        return;
      }

      const reader = res.body.getReader();
      const decoder = new TextDecoder();
      let buf = "";
      while (true) {
        const { value, done } = await reader.read();
        if (done) break;
        buf += decoder.decode(value, { stream: true });
        let nl: number;
        while ((nl = buf.indexOf("\n")) !== -1) {
          const line = buf.slice(0, nl).trim();
          buf = buf.slice(nl + 1);
          if (!line) continue;
          try {
            handleEvent(JSON.parse(line));
          } catch {
            // ignore malformed line
          }
        }
      }
      // Drain any tail without a trailing newline.
      const tail = buf.trim();
      if (tail) {
        try {
          handleEvent(JSON.parse(tail));
        } catch {
          /* ignore */
        }
      }
    } catch (e) {
      const aborted = e instanceof Error && e.name === "AbortError";
      if (!aborted) {
        const msg = e instanceof Error ? e.message : String(e);
        setError(msg);
      }
    } finally {
      abortRef.current = null;
      setIsStreaming(false);
    }
  }, [input, isStreaming, messages, sessionId, handleEvent]);

  return (
    // h-full assumes the parent gives us full height. The page sets that up.
    <div className="flex h-full min-h-0 flex-col">
      {/* Slim header — session indicator + new conversation. */}
      <div className="flex items-center justify-between gap-3 border-b border-border bg-secondary/40 px-3 py-2 text-xs sm:px-4">
        <div className="flex items-center gap-2 text-muted-foreground">
          <span className="font-smallcaps tracking-[0.12em]">session</span>
          <code
            className="truncate font-mono text-[0.7rem] text-foreground/80"
            title={sessionId}
          >
            {sessionId}
          </code>
        </div>
        <button
          type="button"
          onClick={startNewConversation}
          disabled={isStreaming}
          className="border border-border bg-card px-2 py-1 text-[0.7rem] text-foreground transition hover:border-primary/40 hover:text-primary disabled:cursor-not-allowed disabled:opacity-40"
          title="Start a fresh conversation (history kept under previous id)"
        >
          + New conversation
        </button>
      </div>
      <div ref={scrollRef} className="flex-1 min-h-0 overflow-y-auto px-3 py-4 sm:px-4">
        <div className="mx-auto flex w-full max-w-3xl flex-col gap-4">
          {!historyLoaded ? (
            <div className="flex items-center justify-center py-10 text-sm text-muted-foreground italic">
              loading conversation…
            </div>
          ) : messages.length === 0 ? (
            <Message
              message={{
                role: "assistant",
                parts: [{ kind: "text", text: intro }],
              }}
            />
          ) : (
            messages.map((m, i) => (
              <Message
                key={i}
                message={m}
                streaming={
                  isStreaming &&
                  i === messages.length - 1 &&
                  m.role === "assistant"
                }
              />
            ))
          )}
          {error && (
            <div className="border border-red-300 bg-red-100 px-3 py-2 text-sm text-red-700">
              {error}
            </div>
          )}
        </div>
      </div>
      <Composer
        value={input}
        onChange={setInput}
        onSubmit={send}
        onStop={stop}
        isStreaming={isStreaming}
      />
    </div>
  );
}
