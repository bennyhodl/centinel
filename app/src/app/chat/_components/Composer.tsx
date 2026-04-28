"use client";

import { useEffect, useRef } from "react";

export interface ComposerProps {
  value: string;
  onChange: (v: string) => void;
  onSubmit: () => void;
  onStop: () => void;
  isStreaming: boolean;
  disabled?: boolean;
}

const MAX_ROWS = 6;
const LINE_HEIGHT = 22; // px, matches text-sm leading

export default function Composer({
  value,
  onChange,
  onSubmit,
  onStop,
  isStreaming,
  disabled,
}: ComposerProps) {
  const ref = useRef<HTMLTextAreaElement>(null);

  // Auto-grow.
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    const max = MAX_ROWS * LINE_HEIGHT + 16;
    el.style.height = Math.min(el.scrollHeight, max) + "px";
  }, [value]);

  function handleKey(e: React.KeyboardEvent<HTMLTextAreaElement>) {
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
      e.preventDefault();
      if (!isStreaming && value.trim()) onSubmit();
    }
  }

  return (
    <div className="border-t border-white/10 bg-black/40 px-3 py-3 backdrop-blur sm:px-4">
      <form
        onSubmit={(e) => {
          e.preventDefault();
          if (!isStreaming && value.trim()) onSubmit();
        }}
        className="mx-auto flex w-full max-w-3xl items-end gap-2"
      >
        <textarea
          ref={ref}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={handleKey}
          disabled={disabled}
          rows={1}
          placeholder="Ask the Editor…  (⌘/Ctrl+Enter to send)"
          className="flex-1 resize-none rounded-md border border-white/10 bg-white/[0.03] px-3 py-2 text-sm placeholder:text-white/30 focus:border-tampa-cyan/50 focus:outline-none focus:ring-1 focus:ring-tampa-cyan/40"
        />
        {isStreaming ? (
          <button
            type="button"
            onClick={onStop}
            className="rounded-md border border-red-400/40 bg-red-400/10 px-3 py-2 text-sm font-medium text-red-300 transition hover:bg-red-400/20"
          >
            Stop
          </button>
        ) : (
          <button
            type="submit"
            disabled={disabled || !value.trim()}
            className="rounded-md border border-tampa-cyan/40 bg-tampa-cyan/10 px-3 py-2 text-sm font-medium text-tampa-cyan transition hover:bg-tampa-cyan/20 disabled:cursor-not-allowed disabled:opacity-40"
          >
            Send
          </button>
        )}
      </form>
    </div>
  );
}
