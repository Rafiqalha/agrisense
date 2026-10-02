export interface ChatMessage {
  role: "system" | "user" | "assistant";
  content: string;
}

export interface ChatOptions {
  model: string;
  temperature: number;
  maxTokens: number;
}

function base(url: string): string {
  return url.replace(/\/+$/, "");
}

export interface RagHit {
  source: string;
  file: string;
  title: string;
  cuplikan: string;
  skor: number;
}

function ragBase(url: string): string {
  return url.replace(/\/+$/, "");
}

export async function checkRag(
  ragURL: string,
  signal?: AbortSignal
): Promise<{ ok: boolean; rows: number }> {
  const res = await fetch(`${ragBase(ragURL)}/api/rag/health`, { signal });
  if (!res.ok) throw new Error(`HTTP ${res.status} ${res.statusText}`);
  return (await res.json()) as { ok: boolean; rows: number };
}

export async function searchRag(
  ragURL: string,
  query: string,
  topK: number,
  signal: AbortSignal
): Promise<RagHit[]> {
  const res = await fetch(`${ragBase(ragURL)}/api/rag/search`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    signal,
    body: JSON.stringify({ query, top_k: topK }),
  });
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(`RAG HTTP ${res.status}${text ? ` — ${text.slice(0, 160)}` : ""}`);
  }
  const data = await res.json();
  if (data?.error) throw new Error(`RAG: ${data.error}`);
  return (Array.isArray(data?.results) ? data.results : []) as RagHit[];
}

export async function listModels(
  baseURL: string,
  apiKey: string,
  signal?: AbortSignal
): Promise<string[]> {
  const headers: Record<string, string> = {};
  if (apiKey.trim()) headers["Authorization"] = `Bearer ${apiKey.trim()}`;
  const res = await fetch(`${base(baseURL)}/models`, { headers, signal });
  if (!res.ok) throw new Error(`HTTP ${res.status} ${res.statusText}`);
  const data = await res.json();
  const arr = Array.isArray(data?.data) ? data.data : [];
  return arr.map((m: { id?: string }) => String(m?.id ?? "")).filter(Boolean);
}

export async function streamChat(
  baseURL: string,
  apiKey: string,
  messages: ChatMessage[],
  opts: ChatOptions,
  onToken: (t: string) => void,
  signal: AbortSignal
): Promise<void> {
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  if (apiKey.trim()) headers["Authorization"] = `Bearer ${apiKey.trim()}`;
  const res = await fetch(`${base(baseURL)}/chat/completions`, {
    method: "POST",
    headers,
    signal,
    body: JSON.stringify({
      model: opts.model,
      messages,
      temperature: opts.temperature,
      max_tokens: opts.maxTokens,
      stream: true,
    }),
  });
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(`HTTP ${res.status} ${res.statusText}${text ? ` — ${text.slice(0, 200)}` : ""}`);
  }
  if (!res.body) throw new Error("Respons tanpa body stream");

  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buf = "";
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    buf += decoder.decode(value, { stream: true });
    const lines = buf.split("\n");
    buf = lines.pop() ?? "";
    for (const line of lines) {
      const t = line.trim();
      if (!t.startsWith("data:")) continue;
      const payload = t.slice(5).trim();
      if (payload === "[DONE]") return;
      try {
        const json = JSON.parse(payload);
        const delta: string = json?.choices?.[0]?.delta?.content ?? "";
        if (delta) onToken(delta);
      } catch {
        continue;
      }
    }
  }
}
