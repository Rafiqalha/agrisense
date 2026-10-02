import { useEffect, useRef, useState } from "react";
import "./index.css";
import {
  checkRag,
  listModels,
  searchRag,
  streamChat,
  type ChatMessage,
  type RagHit,
} from "./api";
import { Markdown } from "./md";

interface Settings {
  preset: string;
  baseURL: string;
  apiKey: string;
  model: string;
  system: string;
  temperature: number;
  maxTokens: number;
  ragEnabled: boolean;
  ragURL: string;
  topK: number;
  theme: "light" | "dark";
}

interface Msg {
  role: "system" | "user" | "assistant";
  content: string;
  sources?: RagHit[];
  stat?: string;
}

interface Chat {
  id: string;
  title: string;
  msgs: Msg[];
  updatedAt: number;
}

interface Attachment {
  name: string;
  text: string;
}

const PRESETS: Record<string, { baseURL: string; model: string; note: string }> = {
  ollama: {
    baseURL: "http://localhost:11434/v1",
    model: "qwen3:8b",
    note: "Ollama langsung di perangkat ini.",
  },
  "ollama-proxy": {
    baseURL: "/api/ollama/v1",
    model: "qwen3:8b",
    note: "Ollama via proxy dev (bebas CORS).",
  },
  lmstudio: { baseURL: "http://localhost:1234/v1", model: "qwen3-8b", note: "LM Studio langsung." },
  "lmstudio-proxy": {
    baseURL: "/api/lmstudio/v1",
    model: "qwen3-8b",
    note: "LM Studio via proxy dev.",
  },
  llamacpp: { baseURL: "http://localhost:8080/v1", model: "qwen3-8b", note: "llama.cpp server." },
  custom: { baseURL: "", model: "", note: "Endpoint OpenAI-compatible apa pun." },
};

const PRESET_LABELS: Record<string, string> = {
  ollama: "Ollama",
  "ollama-proxy": "Ollama (proxy)",
  lmstudio: "LM Studio",
  "lmstudio-proxy": "LM Studio (proxy)",
  llamacpp: "llama.cpp",
  custom: "Custom",
};

const DEFAULTS: Settings = {
  preset: "ollama",
  baseURL: PRESETS.ollama.baseURL,
  apiKey: "",
  model: PRESETS.ollama.model,
  system: "Kamu AgriSense, asisten pertanian Indonesia yang ramah dan akurat. Jawab ringkas dengan format markdown yang rapi.",
  temperature: 0.7,
  maxTokens: 1024,
  ragEnabled: false,
  ragURL: "http://localhost:8000",
  topK: 5,
  theme: "dark",
};

const SUGGESTIONS = [
  "Berapa produksi padi dan harga beras terkini?",
  "Bagaimana cara mendiagnosis penyakit daun pada melon?",
  "Jelaskan jadwal pemupukan jagung yang baik",
  "Berapa harga cabai dan bawang minggu ini?",
];

function loadSettings(): Settings {
  try {
    const raw = localStorage.getItem("test-llm-settings");
    if (raw) return { ...DEFAULTS, ...JSON.parse(raw) };
  } catch {
    /* abaikan */
  }
  return DEFAULTS;
}

function loadChats(): Chat[] {
  try {
    const raw = localStorage.getItem("test-llm-chats");
    if (raw) {
      const arr = JSON.parse(raw);
      if (Array.isArray(arr)) return arr;
    }
  } catch {
    /* abaikan */
  }
  return [];
}

function uid(): string {
  return `${Date.now().toString(36)}${Math.floor(Math.random() * 1e6).toString(36)}`;
}

function cleanTitle(t: string): string {
  return t
    .replace(/\.(xlsx?|csv|json|pdf|txt|md)$/i, "")
    .replace(/^[0-9]+[_\-\s.]*/, "")
    .replace(/[_-]+/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

export default function App() {
  const [s, setS] = useState<Settings>(loadSettings);
  const [chats, setChats] = useState<Chat[]>(loadChats);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState("");
  const [models, setModels] = useState<string[]>([]);
  const [notice, setNotice] = useState("");
  const [ragStatus, setRagStatus] = useState("");
  const [openSrc, setOpenSrc] = useState<number | null>(null);
  const [openStat, setOpenStat] = useState<number | null>(null);
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [attach, setAttach] = useState<Attachment | null>(null);
  const [listening, setListening] = useState(false);
  const abort = useRef<AbortController | null>(null);
  const recRef = useRef<{ stop: () => void } | null>(null);
  const bottom = useRef<HTMLDivElement>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const areaRef = useRef<HTMLTextAreaElement>(null);

  const active = chats.find((c) => c.id === activeId) ?? null;
  const msgs: Msg[] = active?.msgs ?? [];

  useEffect(() => {
    localStorage.setItem("test-llm-settings", JSON.stringify(s));
    document.documentElement.dataset.theme = s.theme;
  }, [s]);
  useEffect(() => {
    localStorage.setItem("test-llm-chats", JSON.stringify(chats.slice(0, 30)));
  }, [chats]);
  useEffect(() => {
    bottom.current?.scrollIntoView({ behavior: "smooth" });
  }, [msgs, activeId]);

  function updateMsgs(id: string, fn: (m: Msg[]) => Msg[]) {
    setChats((cs) =>
      cs.map((c) => (c.id === id ? { ...c, msgs: fn(c.msgs), updatedAt: Date.now() } : c))
    );
  }

  function newChat(): string {
    const id = uid();
    setChats((cs) => [{ id, title: "Percakapan baru", msgs: [], updatedAt: Date.now() }, ...cs]);
    setActiveId(id);
    setNotice("");
    setOpenSrc(null);
    setOpenStat(null);
    return id;
  }

  function ensureChat(): string {
    if (activeId) return activeId;
    return newChat();
  }

  async function testConn() {
    setStatus("menghubungi...");
    try {
      const list = await listModels(s.baseURL, s.apiKey);
      setModels(list);
      setStatus(`terhubung — ${list.length} model`);
      if (list.length > 0 && !list.includes(s.model)) {
        setS((v) => ({ ...v, model: list[0] }));
      }
    } catch (e) {
      setStatus(`gagal: ${e instanceof Error ? e.message : String(e)}`);
    }
  }

  async function checkRagConn() {
    setRagStatus("menghubungi...");
    try {
      const h = await checkRag(s.ragURL);
      setRagStatus(h.ok ? `RAG ok — ${h.rows} chunk` : "RAG bermasalah");
    } catch (e) {
      setRagStatus(`gagal: ${e instanceof Error ? e.message : String(e)}`);
    }
  }

  function onAttachFile(file: File | undefined) {
    if (!file) return;
    if (!/\.(csv|txt|md|json)$/i.test(file.name)) {
      setNotice("Format lampiran didukung: CSV, TXT, MD, JSON");
      return;
    }
    const reader = new FileReader();
    reader.onload = () => {
      const text = String(reader.result ?? "").slice(0, 30000);
      setAttach({ name: file.name, text });
      setNotice("");
    };
    reader.readAsText(file);
  }

  function toggleVoice() {
    const w = window as unknown as {
      SpeechRecognition?: new () => {
        lang: string;
        interimResults: boolean;
        onresult: ((e: { results: ArrayLike<ArrayLike<{ transcript: string }>> }) => void) | null;
        onend: (() => void) | null;
        start: () => void;
        stop: () => void;
      };
      webkitSpeechRecognition?: new () => {
        lang: string;
        interimResults: boolean;
        onresult: ((e: { results: ArrayLike<ArrayLike<{ transcript: string }>> }) => void) | null;
        onend: (() => void) | null;
        start: () => void;
        stop: () => void;
      };
    };
    const SR = w.SpeechRecognition ?? w.webkitSpeechRecognition;
    if (!SR) {
      setNotice("Browser tidak mendukung voice input");
      return;
    }
    if (recRef.current) {
      recRef.current.stop();
      recRef.current = null;
      setListening(false);
      return;
    }
    const rec = new SR();
    rec.lang = "id-ID";
    rec.interimResults = true;
    rec.onresult = (e) => {
      let t = "";
      for (let i = 0; i < e.results.length; i++) t += e.results[i][0]?.transcript ?? "";
      setInput(t);
    };
    rec.onend = () => {
      recRef.current = null;
      setListening(false);
    };
    recRef.current = rec;
    setListening(true);
    rec.start();
  }

  async function send(prefill?: string) {
    const text = (prefill ?? input).trim();
    if (!text || busy) return;
    setBusy(true);
    const id = ensureChat();
    const ctrl = new AbortController();
    abort.current = ctrl;

    let ragCtx = "";
    let hits: RagHit[] = [];
    if (s.ragEnabled) {
      setNotice("Mengambil konteks RAG…");
      try {
        hits = await searchRag(s.ragURL, text, s.topK, ctrl.signal);
      } catch (e) {
        updateMsgs(id, (v) => [
          ...v,
          { role: "user", content: text },
          {
            role: "assistant",
            content: `ERROR RAG: ${e instanceof Error ? e.message : String(e)} (jalankan: \`python3 scripts/rag_server.py\`)`,
          },
        ]);
        setBusy(false);
        abort.current = null;
        return;
      }
      if (hits.length > 0) {
        ragCtx =
          "Konteks data pertanian (gunakan bila relevan, sebutkan sumber bila memakai angka):\n" +
          hits.map((h, i) => `[${i + 1}] (${h.source}/${h.file}, skor ${h.skor})\n${h.cuplikan}`).join("\n\n");
      }
    }
    const attachCtx = attach
      ? `Lampiran pengguna (${attach.name}):\n${attach.text}`
      : "";
    const history: ChatMessage[] = [
      ...(s.system.trim() ? [{ role: "system" as const, content: s.system.trim() }] : []),
      ...(ragCtx ? [{ role: "system" as const, content: ragCtx }] : []),
      ...(attachCtx ? [{ role: "system" as const, content: attachCtx }] : []),
      ...msgs.filter((m) => m.role !== "system"),
      { role: "user", content: text },
    ];
    updateMsgs(id, (v) => [...v, { role: "user", content: text }]);
    if (active && active.title === "Percakapan baru") {
      const title = text.length > 28 ? `${text.slice(0, 28)}…` : text;
      setChats((cs) => cs.map((c) => (c.id === id ? { ...c, title } : c)));
    }
    setInput("");
    if (areaRef.current) areaRef.current.style.height = "auto";
    setNotice("");
    const t0 = performance.now();
    let first = 0;
    let acc = "";
    updateMsgs(id, (v) => [...v, { role: "assistant", content: "", sources: hits }]);
    try {
      await streamChat(
        s.baseURL,
        s.apiKey,
        history,
        { model: s.model, temperature: s.temperature, maxTokens: s.maxTokens },
        (t) => {
          if (!first) first = performance.now();
          acc += t;
          updateMsgs(id, (v) => {
            const next = [...v];
            next[next.length - 1] = { role: "assistant", content: acc, sources: hits };
            return next;
          });
        },
        ctrl.signal
      );
      const total = ((performance.now() - t0) / 1000).toFixed(1);
      const ttft = first ? ((first - t0) / 1000).toFixed(1) : "-";
      const detail = `${s.model} · TTFT ${ttft}s · total ${total}s · ${acc.length} karakter${
        hits.length > 0 ? ` · ${hits.length} sumber RAG` : ""
      }`;
      updateMsgs(id, (v) => {
        const next = [...v];
        next[next.length - 1] = { role: "assistant", content: acc, sources: hits, stat: detail };
        return next;
      });
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      if (ctrl.signal.aborted) {
        updateMsgs(id, (v) => {
          const next = [...v];
          next[next.length - 1] = { ...next[next.length - 1], stat: "Dihentikan" };
          return next;
        });
      } else {
        updateMsgs(id, (v) => {
          const next = [...v];
          next[next.length - 1] = { role: "assistant", content: `ERROR: ${msg}`, sources: hits };
          return next;
        });
      }
    } finally {
      setBusy(false);
      abort.current = null;
    }
  }

  function deleteChat(id: string) {
    setChats((cs) => cs.filter((c) => c.id !== id));
    if (activeId === id) {
      setActiveId(null);
      setNotice("");
      setOpenSrc(null);
      setOpenStat(null);
    }
  }

  const dark = s.theme === "dark";

  return (
    <div className="flex h-full">
      {/* ── Sidebar ─────────────────────────────── */}
      <aside
        className={`thin-scroll shrink-0 overflow-y-auto transition-all duration-300 ${
          sidebarOpen ? "w-[260px] opacity-100" : "w-0 opacity-0"
        }`}
        style={{ background: "var(--bg-soft)" }}
      >
        <div className="flex w-[260px] flex-col gap-2 p-4">
          <div className="flex items-center gap-2 px-1 pb-2">
            <span
              className="g-text text-xl font-semibold"
              style={{ fontSize: 22 }}
            >
              ✦
            </span>
            <span className="text-[15px] font-medium">AgriSense</span>
          </div>
          <button
            onClick={() => newChat()}
            className="history-item rounded-full px-4 py-2.5 text-left text-sm font-medium"
            style={{ background: "var(--bubble)", color: "var(--ink)" }}
          >
            ＋ Percakapan baru
          </button>
          <div
            className="mt-2 border-t px-2 pt-3 text-xs font-medium uppercase tracking-wide"
            style={{ borderColor: "var(--line)", color: "var(--ink-faint)" }}
          >
            Riwayat
          </div>
          <div className="flex flex-col gap-1">
            {chats.length === 0 && (
              <div className="px-2 text-sm" style={{ color: "var(--ink-faint)" }}>
                Belum ada riwayat.
              </div>
            )}
            {chats.map((c) => (
              <div
                key={c.id}
                onClick={() => setActiveId(c.id)}
                className="history-item group flex min-w-0 cursor-pointer items-center gap-2.5 rounded-xl px-3 py-2.5 text-sm"
                style={{
                  background: c.id === activeId ? "var(--bubble)" : "transparent",
                }}
              >
                <span className="shrink-0 text-[13px]" style={{ color: "var(--ink-faint)" }}>
                  💬
                </span>
                <span className="min-w-0 flex-1 truncate">{c.title}</span>
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    deleteChat(c.id);
                  }}
                  className="icon-btn rounded-md px-1 opacity-0 group-hover:opacity-100"
                  style={{ color: "var(--ink-faint)" }}
                  title="Hapus"
                >
                  ✕
                </button>
              </div>
            ))}
          </div>
          <div className="mt-auto flex flex-col gap-2 px-1 pt-3">
            <div
              className="flex flex-col gap-2 rounded-2xl p-3 text-xs"
              style={{ background: "var(--bg)", border: "1px solid var(--line)" }}
            >
              <div className="flex items-center gap-2">
                <span
                  className="inline-block h-2 w-2 shrink-0 rounded-full"
                  style={{ background: "var(--dot-online)" }}
                />
                <span className="truncate" style={{ color: "var(--ink-soft)" }}>
                  {s.model}
                </span>
              </div>
              <div className="flex items-center gap-2">
                <span
                  className="inline-block h-2 w-2 shrink-0 rounded-full"
                  style={{ background: s.ragEnabled ? "var(--dot-online)" : "var(--dot-off)" }}
                />
                <span style={{ color: "var(--ink-soft)" }}>
                  {s.ragEnabled ? `RAG aktif · top-${s.topK}` : "RAG nonaktif"}
                </span>
              </div>
            </div>
            <div className="flex items-center gap-1 px-1">
              <button
                onClick={() => setS((v) => ({ ...v, theme: v.theme === "dark" ? "light" : "dark" }))}
                className="icon-btn rounded-full p-2 text-base"
                style={{ color: "var(--ink-soft)" }}
                title="Mode gelap / terang"
              >
                {dark ? "☀" : "☾"}
              </button>
              <button
                onClick={() => setSettingsOpen((v) => !v)}
                className="icon-btn rounded-full p-2 text-base"
                style={{ color: "var(--ink-soft)" }}
                title="Pengaturan"
              >
                ⚙
              </button>
              <span className="ml-auto text-[11px]" style={{ color: "var(--ink-faint)" }}>
                v0.1 · lokal
              </span>
            </div>
          </div>
        </div>
      </aside>

      {/* ── Main ────────────────────────────────── */}
      <main className="relative flex min-w-0 flex-1 flex-col">
        <header className="flex items-center gap-2 px-4 py-3">
          <button
            onClick={() => setSidebarOpen((v) => !v)}
            className="icon-btn rounded-full p-2"
            style={{ color: "var(--ink-soft)" }}
            title="Sidebar"
          >
            ☰
          </button>
        </header>

        {/* Settings slide-over */}
        {settingsOpen && (
          <div
            className="anim-fade-up thin-scroll absolute right-4 top-14 z-20 max-h-[80vh] w-[330px] overflow-y-auto rounded-2xl p-4 shadow-xl"
            style={{ background: "var(--bg-soft)", border: "1px solid var(--line)" }}
          >
            <div className="mb-2 text-sm font-medium">Pengaturan</div>
            <label className="mb-1 block text-xs" style={{ color: "var(--ink-faint)" }}>
              Preset server
            </label>
            <select
              value={s.preset}
              onChange={(e) => {
                const name = e.target.value;
                const p = PRESETS[name];
                setS((v) => ({
                  ...v,
                  preset: name,
                  ...(name === "custom" ? {} : { baseURL: p.baseURL, model: p.model || v.model }),
                }));
                setModels([]);
                setStatus("");
              }}
              className="w-full rounded-lg px-2 py-2 text-sm"
              style={{ background: "var(--bg)", border: "1px solid var(--line)", color: "var(--ink)" }}
            >
              {Object.keys(PRESETS).map((k) => (
                <option key={k} value={k}>
                  {PRESET_LABELS[k]}
                </option>
              ))}
            </select>
            <label className="mb-1 mt-3 block text-xs" style={{ color: "var(--ink-faint)" }}>
              Base URL
            </label>
            <input
              value={s.baseURL}
              onChange={(e) => setS({ ...s, baseURL: e.target.value, preset: "custom" })}
              className="w-full rounded-lg px-2 py-2 text-sm"
              style={{ background: "var(--bg)", border: "1px solid var(--line)", color: "var(--ink)" }}
            />
            <label className="mb-1 mt-3 block text-xs" style={{ color: "var(--ink-faint)" }}>
              Model
            </label>
            {models.length > 0 ? (
              <select
                value={s.model}
                onChange={(e) => setS({ ...s, model: e.target.value })}
                className="w-full rounded-lg px-2 py-2 text-sm"
                style={{ background: "var(--bg)", border: "1px solid var(--line)", color: "var(--ink)" }}
              >
                {models.map((m) => (
                  <option key={m} value={m}>
                    {m}
                  </option>
                ))}
              </select>
            ) : (
              <input
                value={s.model}
                onChange={(e) => setS({ ...s, model: e.target.value })}
                className="w-full rounded-lg px-2 py-2 text-sm"
                style={{ background: "var(--bg)", border: "1px solid var(--line)", color: "var(--ink)" }}
              />
            )}
            <div className="mt-2 flex items-center gap-2">
              <button
                onClick={() => void testConn()}
                className="rounded-full px-3 py-1.5 text-xs font-medium"
                style={{ background: "var(--bubble)", color: "var(--ink)" }}
              >
                Test koneksi
              </button>
              {status && (
                <span className="text-xs" style={{ color: "var(--ink-faint)" }}>
                  {status}
                </span>
              )}
            </div>
            <div className="mt-3 text-xs" style={{ color: "var(--ink-faint)" }}>
              {PRESETS[s.preset]?.note ?? ""}
            </div>

            <label className="mb-1 mt-4 block text-xs" style={{ color: "var(--ink-faint)" }}>
              System prompt
            </label>
            <textarea
              rows={3}
              value={s.system}
              onChange={(e) => setS({ ...s, system: e.target.value })}
              className="w-full rounded-lg px-2 py-2 text-sm"
              style={{ background: "var(--bg)", border: "1px solid var(--line)", color: "var(--ink)" }}
            />
            <label className="mb-1 mt-3 block text-xs" style={{ color: "var(--ink-faint)" }}>
              Temperature: {s.temperature.toFixed(2)}
            </label>
            <input
              type="range"
              min={0}
              max={2}
              step={0.05}
              value={s.temperature}
              onChange={(e) => setS({ ...s, temperature: Number(e.target.value) })}
              className="w-full"
            />
            <label className="mb-1 mt-2 block text-xs" style={{ color: "var(--ink-faint)" }}>
              Max tokens: {s.maxTokens}
            </label>
            <input
              type="range"
              min={64}
              max={8192}
              step={64}
              value={s.maxTokens}
              onChange={(e) => setS({ ...s, maxTokens: Number(e.target.value) })}
              className="w-full"
            />

            <div className="mt-4 flex items-center gap-2">
              <input
                type="checkbox"
                checked={s.ragEnabled}
                onChange={(e) => setS({ ...s, ragEnabled: e.target.checked })}
              />
              <span className="text-sm font-medium">RAG database pertanian</span>
            </div>
            <label className="mb-1 mt-2 block text-xs" style={{ color: "var(--ink-faint)" }}>
              Sidecar URL
            </label>
            <input
              value={s.ragURL}
              onChange={(e) => setS({ ...s, ragURL: e.target.value })}
              className="w-full rounded-lg px-2 py-2 text-sm"
              style={{ background: "var(--bg)", border: "1px solid var(--line)", color: "var(--ink)" }}
            />
            <label className="mb-1 mt-2 block text-xs" style={{ color: "var(--ink-faint)" }}>
              Top-K: {s.topK}
            </label>
            <input
              type="range"
              min={1}
              max={10}
              step={1}
              value={s.topK}
              onChange={(e) => setS({ ...s, topK: Number(e.target.value) })}
              className="w-full"
            />
            <div className="mt-2 flex items-center gap-2">
              <button
                onClick={() => void checkRagConn()}
                className="rounded-full px-3 py-1.5 text-xs font-medium"
                style={{ background: "var(--bubble)", color: "var(--ink)" }}
              >
                Test RAG
              </button>
              {ragStatus && (
                <span className="text-xs" style={{ color: "var(--ink-faint)" }}>
                  {ragStatus}
                </span>
              )}
            </div>
          </div>
        )}

        {/* Chat stream */}
        <div className="thin-scroll flex-1 overflow-y-auto px-4 pb-4 md:px-8">
          <div className="mx-auto w-full max-w-[768px]">
            {msgs.length === 0 && (
              <div className="anim-fade-up flex min-h-[52vh] flex-col justify-center">
                <h1 className="font-normal" style={{ fontSize: 30, lineHeight: 1.3 }}>
                  Halo, <span className="g-text font-medium">Sobat Tani</span>
                </h1>
                <p className="mt-2.5 text-[14px]" style={{ color: "var(--ink-soft)" }}>
                  Tanya apa saja soal pertanian — {s.ragEnabled ? "dijawab dengan data RAG" : `didukung ${s.model}`}
                </p>
                <div className="mt-7 grid grid-cols-1 gap-2.5 sm:grid-cols-2">
                  {SUGGESTIONS.map((g) => (
                    <button
                      key={g}
                      onClick={() => void send(g)}
                      className="suggest-card min-h-[76px] rounded-2xl border px-4 py-3 text-left text-[13.5px] leading-snug"
                      style={{
                        background: "var(--bg)",
                        borderColor: "var(--line)",
                        color: "var(--ink-soft)",
                      }}
                    >
                      {g}
                    </button>
                  ))}
                </div>
              </div>
            )}
            {msgs.map((m, i) =>
              m.role === "user" ? (
                <div key={i} className="anim-fade-up mb-5 flex justify-end">
                  <div
                    className="max-w-[85%] text-right text-[15px] font-normal leading-relaxed"
                    style={{ color: "var(--ink-soft)" }}
                  >
                    {m.content}
                  </div>
                </div>
              ) : m.role === "system" ? null : (
                <div key={i} className="anim-fade-up mb-7 pl-1">
                  {m.content === "" && busy && i === msgs.length - 1 ? (
                    <div>
                      <div className="anim-thinking mb-3">
                        <span />
                        <span />
                        <span />
                      </div>
                      <div className="flex flex-col gap-2">
                        <div className="skeleton-line w-11/12" />
                        <div className="skeleton-line w-4/5" />
                        <div className="skeleton-line w-3/5" />
                      </div>
                    </div>
                  ) : (
                    <Markdown text={m.content} />
                  )}
                  <div className="mt-2 flex items-center gap-1.5">
                    {m.sources && m.sources.length > 0 && (
                      <button
                        onClick={() => setOpenSrc(openSrc === i ? null : i)}
                        className="icon-btn flex items-center gap-1.5 rounded-full px-2.5 py-1 text-[11px]"
                        style={{ color: "var(--ink-faint)", background: "var(--bg-soft)" }}
                      >
                        <span>▤</span>
                        <span>
                          {m.sources.length} sumber{openSrc === i ? " ▲" : ""}
                        </span>
                      </button>
                    )}
                    {m.stat && (
                      <button
                        onClick={() => setOpenStat(openStat === i ? null : i)}
                        className="icon-btn rounded-full px-2 py-1 text-[13px]"
                        style={{ color: "var(--ink-faint)" }}
                        title="Detail pemrosesan"
                      >
                        <span className="g-text font-semibold">✦</span>
                      </button>
                    )}
                  </div>
                  {openSrc === i && m.sources && m.sources.length > 0 && (
                    <div
                      className="anim-fade-up mt-2 flex flex-col gap-1 rounded-2xl p-2"
                      style={{ background: "var(--bg-soft)" }}
                    >
                      {m.sources.map((h, hi) => (
                        <div
                          key={hi}
                          className="flex items-baseline gap-2 rounded-xl px-3 py-2 text-[12.5px]"
                          title={`${h.file} · similarity ${h.skor}\n${h.cuplikan.slice(0, 220)}`}
                        >
                          <span
                            className="flex h-5 w-5 shrink-0 items-center justify-center rounded-full text-[10px] font-medium"
                            style={{ background: "var(--bubble)", color: "var(--ink-soft)" }}
                          >
                            {hi + 1}
                          </span>
                          <span className="truncate" style={{ color: "var(--ink-soft)" }}>
                            {cleanTitle(h.title || h.file)}
                          </span>
                        </div>
                      ))}
                    </div>
                  )}
                  {openStat === i && m.stat && (
                    <div
                      className="anim-fade-up mt-2 rounded-2xl px-3 py-2 text-[11.5px]"
                      style={{ background: "var(--bg-soft)", color: "var(--ink-faint)" }}
                    >
                      {m.stat}
                    </div>
                  )}
                </div>
              )
            )}
            <div ref={bottom} />
          </div>
        </div>

        {/* Dock */}
        <div className="px-4 pb-7 md:px-8" style={{ background: "var(--bg)" }}>
          <div className="mx-auto w-full max-w-[768px]">
            {notice && (
              <div className="mb-2.5 text-center text-xs" style={{ color: "var(--ink-faint)" }}>
                {notice}
              </div>
            )}
            {attach && (
              <div
                className="anim-fade-up mb-2 flex items-center gap-2 rounded-full px-4 py-1.5 text-xs"
                style={{ background: "var(--bg-soft)", color: "var(--ink-soft)" }}
              >
                <span className="truncate">
                  📎 {attach.name} ({attach.text.length} karakter)
                </span>
                <button onClick={() => setAttach(null)} className="icon-btn ml-auto" title="Hapus lampiran">
                  ✕
                </button>
              </div>
            )}
            <div
              className="g-ring flex items-center gap-1 rounded-full py-2 pl-2 pr-2 shadow-sm"
              style={{ background: "var(--dock)", border: "1px solid var(--line)", minHeight: 60 }}
            >
              <input
                ref={fileRef}
                type="file"
                accept=".csv,.txt,.md,.json"
                className="hidden"
                onChange={(e) => {
                  onAttachFile(e.target.files?.[0]);
                  e.target.value = "";
                }}
              />
              <button
                onClick={() => fileRef.current?.click()}
                className="icon-btn rounded-full p-2.5 text-lg"
                style={{ color: "var(--ink-soft)" }}
                title="Lampirkan dataset (CSV/TXT/MD/JSON)"
              >
                ＋
              </button>
              <button
                onClick={() => toggleVoice()}
                className="icon-btn rounded-full p-2.5 text-lg"
                style={{ color: listening ? "#ea4335" : "var(--ink-soft)" }}
                title="Input suara (id-ID)"
              >
                {listening ? "⏺" : "🎙"}
              </button>
              <textarea
                ref={areaRef}
                rows={1}
                value={input}
                onChange={(e) => {
                  setInput(e.target.value);
                  const el = areaRef.current;
                  if (el) {
                    el.style.height = "auto";
                    el.style.height = `${Math.min(el.scrollHeight, 140)}px`;
                  }
                }}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && !e.shiftKey) {
                    e.preventDefault();
                    void send();
                  }
                }}
                placeholder="Tanya AgriSense…"
                disabled={busy}
                className="thin-scroll max-h-[140px] flex-1 resize-none bg-transparent px-2 py-2.5 text-[15px] outline-none"
                style={{ color: "var(--ink)" }}
              />
              {busy ? (
                <button
                  onClick={() => abort.current?.abort()}
                  className="icon-btn shrink-0 rounded-full p-2.5 text-lg"
                  style={{ background: "var(--bubble)" }}
                  title="Stop"
                >
                  ⏹
                </button>
              ) : (
                <button
                  onClick={() => void send()}
                  disabled={!input.trim()}
                  className="send-btn flex h-10 w-10 shrink-0 items-center justify-center rounded-full text-xl disabled:opacity-40"
                  title="Kirim"
                >
                  ↑
                </button>
              )}
            </div>
            <div className="mt-3 text-center text-[11px]" style={{ color: "var(--ink-soft)" }}>
              {s.ragEnabled ? `RAG aktif · top-${s.topK} · ${s.model}` : s.model} · data lokal, tidak dikirim ke cloud
            </div>
          </div>
        </div>
      </main>
    </div>
  );
}
