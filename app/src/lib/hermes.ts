import { config } from "./config";

/**
 * Helper: one-shot, non-streaming Hermes chat completion. Returns the
 * assistant's content string. Throws on transport / HTTP error.
 *
 * Use for short utility calls (link explainer, page summarizer). For the
 * main user chat use the streaming route at /chat/api/route.
 */
export async function hermesComplete(opts: {
  prompt: string;
  sessionId?: string;
  systemPrompt?: string;
  maxTokens?: number;
}): Promise<string> {
  const baseURL = config.hermesApiUrl();
  const apiKey = config.hermesApiKey();
  if (!baseURL) throw new Error("HERMES_API_URL not configured");

  const url = baseURL.replace(/\/$/, "") + "/chat/completions";
  const messages: { role: string; content: string }[] = [];
  if (opts.systemPrompt) {
    messages.push({ role: "system", content: opts.systemPrompt });
  }
  messages.push({ role: "user", content: opts.prompt });

  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    Authorization: `Bearer ${apiKey || "missing"}`,
  };
  if (opts.sessionId) headers["X-Hermes-Session-Id"] = opts.sessionId;

  const resp = await fetch(url, {
    method: "POST",
    headers,
    body: JSON.stringify({
      model: process.env.HERMES_MODEL ?? "hermes-default",
      messages,
      max_tokens: opts.maxTokens ?? 256,
      stream: false,
    }),
  });
  if (!resp.ok) {
    const text = await resp.text().catch(() => "");
    throw new Error(`Hermes ${resp.status}: ${text.slice(0, 300)}`);
  }
  const data = (await resp.json()) as {
    choices?: Array<{ message?: { content?: string } }>;
  };
  return data.choices?.[0]?.message?.content?.trim() ?? "";
}
