import ClientMarkdown from "@/app/status/_components/ClientMarkdown";

export interface ChatMessage {
  role: "user" | "assistant";
  content: string;
}

export default function Message({
  message,
  streaming,
}: {
  message: ChatMessage;
  streaming?: boolean;
}) {
  if (message.role === "user") {
    return (
      <div className="flex justify-end">
        <div className="max-w-[85%] whitespace-pre-wrap rounded-lg bg-tampa-cyan/10 px-3 py-2 text-sm ring-1 ring-tampa-cyan/20">
          {message.content}
        </div>
      </div>
    );
  }
  return (
    <div className="flex flex-col items-start gap-1">
      <div className="text-xs uppercase tracking-wider text-tampa-cyan/80">
        Editor
      </div>
      <div className="max-w-full rounded-lg bg-white/[0.02] px-3 py-2 ring-1 ring-white/10">
        {message.content ? (
          <ClientMarkdown
            source={message.content}
            className="prose prose-sm prose-invert max-w-none prose-p:my-2 prose-headings:mt-3 prose-headings:mb-2 prose-pre:bg-black/40 prose-code:text-tampa-cyan"
          />
        ) : (
          <span className="text-sm opacity-60">…</span>
        )}
        {streaming && message.content && (
          <span className="ml-1 inline-block h-3 w-1.5 animate-pulse bg-tampa-cyan/60 align-middle" />
        )}
      </div>
    </div>
  );
}
