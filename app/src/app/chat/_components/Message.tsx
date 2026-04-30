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
        <div className="max-w-[85%] whitespace-pre-wrap border border-primary/20 bg-accent px-4 py-2 text-sm">
          {message.content}
        </div>
      </div>
    );
  }
  return (
    <div className="flex flex-col items-start gap-1">
      <div className="font-smallcaps text-[0.6rem] tracking-[0.15em] text-primary">
        The Editor
      </div>
      <div className="max-w-full border border-border bg-card px-4 py-3">
        {message.content ? (
          <ClientMarkdown
            source={message.content}
            className="prose-broadsheet prose-wide prose-compact"
          />
        ) : (
          <span className="text-sm text-muted-foreground italic">…</span>
        )}
        {streaming && message.content && (
          <span className="ml-1 inline-block h-3 w-1.5 animate-pulse bg-primary/60 align-middle" />
        )}
      </div>
    </div>
  );
}
