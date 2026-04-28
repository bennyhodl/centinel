"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import Message, { type ChatMessage } from "./Message";
import Composer from "./Composer";

export default function ChatClient({ intro }: { intro: string }) {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState("");
  const [isStreaming, setIsStreaming] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const abortRef = useRef<AbortController | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);

  // Auto-scroll on new content.
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

  const send = useCallback(async () => {
    const text = input.trim();
    if (!text || isStreaming) return;

    setError(null);
    setInput("");

    const next: ChatMessage[] = [
      ...messages,
      { role: "user", content: text },
      { role: "assistant", content: "" },
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
          messages: next
            .slice(0, -1) // omit the empty assistant placeholder
            .map((m) => ({ role: m.role, content: m.content })),
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
      let acc = "";
      while (true) {
        const { value, done } = await reader.read();
        if (done) break;
        acc += decoder.decode(value, { stream: true });
        setMessages((prev) => {
          const copy = prev.slice();
          const last = copy[copy.length - 1];
          if (last && last.role === "assistant") {
            copy[copy.length - 1] = { ...last, content: acc };
          }
          return copy;
        });
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
  }, [input, isStreaming, messages]);

  return (
    <div className="-mx-4 -my-6 flex h-[calc(100vh-9rem)] flex-col sm:h-[calc(100vh-10rem)]">
      <div ref={scrollRef} className="flex-1 overflow-y-auto px-3 py-4 sm:px-4">
        <div className="mx-auto flex w-full max-w-3xl flex-col gap-4">
          {messages.length === 0 ? (
            <Message
              message={{ role: "assistant", content: intro }}
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
