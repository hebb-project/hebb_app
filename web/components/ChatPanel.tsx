"use client";

import { useEffect, useRef, useState } from "react";
import { postChat, type CortexChatResponse } from "@/lib/cortex-api";

type Msg = {
  role: "user" | "core";
  text: string;
  // Optional metadata shown subtly under core messages in live mode.
  meta?: { stimulated?: string[]; activated?: { label: string; spike_count: number }[] };
};

const SEED_CHAT: Msg[] = [
  { role: "core", text: "core online · awaiting input." },
];

type Props = {
  width: number;
  onSend?: (text: string) => void;
  /**
   * Optional explicit base URL. If unset, talks to the same Rust core
   * the rest of the UI uses (NEXT_PUBLIC_CORTEX_HTTP, default
   * http://127.0.0.1:7654). The legacy Python bridge no longer
   * participates by default — Rust-side /api/chat handles it.
   */
  coreBase?: string;
  /** When true, posts to the core; falls back to a mock reply on error. */
  live?: boolean;
};

async function sendChat(coreBase: string | undefined, message: string): Promise<CortexChatResponse> {
  return postChat(message, coreBase);
}

export function ChatPanel({
  width,
  onSend,
  coreBase,
  live = true,
}: Props) {
  const [messages, setMessages] = useState<Msg[]>(SEED_CHAT);
  const [draft, setDraft] = useState("");
  const [pending, setPending] = useState(false);
  const threadRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (threadRef.current) {
      threadRef.current.scrollTop = threadRef.current.scrollHeight;
    }
  }, [messages]);

  const send = async () => {
    const text = draft.trim();
    if (!text || pending) return;
    setMessages((m) => [...m, { role: "user", text }]);
    setDraft("");
    onSend?.(text);

    if (!live) {
      // Mock fallback for development without the bridge.
      setTimeout(() => {
        setMessages((m) => [
          ...m,
          {
            role: "core",
            text: `received. propagating through the connectome. free-energy ↑ ${(Math.random() * 0.2 + 0.1).toFixed(3)}.`,
          },
        ]);
      }, 700);
      return;
    }

    setPending(true);
    try {
      const reply = await sendChat(coreBase, text);
      setMessages((m) => [
        ...m,
        {
          role: "core",
          text: reply.reply,
          meta: {
            stimulated: reply.stimulated.map((s) => s.label),
            activated: reply.activated,
          },
        },
      ]);
    } catch (err) {
      setMessages((m) => [
        ...m,
        {
          role: "core",
          text: `core offline (${err instanceof Error ? err.message : "unknown"}). is the rust core running and a vault ingested?`,
        },
      ]);
    } finally {
      setPending(false);
    }
  };

  return (
    <aside className="panel chat" style={{ width }}>
      <div className="panel-hd mono">
        <span>input · chat</span>
        <span className="panel-hd-meta">{messages.length} msgs</span>
      </div>
      <div className="chat-thread" ref={threadRef}>
        {messages.map((m, i) =>
          m.role === "user" ? (
            <div key={i} className="bubble-row right">
              <div className="bubble user">{m.text}</div>
            </div>
          ) : (
            <div key={i} className="bubble-row left">
              <div className="bubble-meta mono">core</div>
              <div className="bubble core mono">
                <div>{m.text}</div>
                {m.meta?.stimulated && m.meta.stimulated.length > 0 && (
                  <div style={{
                    marginTop: 6, fontSize: 11, opacity: 0.6,
                    borderTop: "1px solid rgba(125,249,255,0.12)", paddingTop: 4,
                  }}>
                    stim: {m.meta.stimulated.join(", ")}
                  </div>
                )}
                {m.meta?.activated && m.meta.activated.length > 0 && (
                  <div style={{ fontSize: 11, opacity: 0.6 }}>
                    fired: {m.meta.activated.map(a => `${a.label}×${a.spike_count}`).join("  ")}
                  </div>
                )}
              </div>
            </div>
          ),
        )}
        {pending && (
          <div className="bubble-row left">
            <div className="bubble-meta mono">core</div>
            <div className="bubble core mono" style={{ opacity: 0.6 }}>
              stimulating…
            </div>
          </div>
        )}
      </div>
      <div className="chat-compose">
        <div className="chat-input-row">
          <input
            className="chat-input"
            placeholder={pending ? "cortex stimulating…" : "ask the core anything…"}
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter") send(); }}
            disabled={pending}
          />
          <button
            className="btn-send mono"
            onClick={send}
            disabled={pending || !draft.trim()}
          >
            SEND
          </button>
        </div>
        <div className="chat-future-btns">
          <button className="btn-stub mono" disabled title="coming soon">
            <span className="stub-dot" /> MIC
          </button>
          <button className="btn-stub mono" disabled title="coming soon">
            <span className="stub-dot" /> IMAGE
          </button>
          <button className="btn-stub mono" disabled title="coming soon">
            <span className="stub-dot" /> CAM
          </button>
        </div>
      </div>
    </aside>
  );
}
