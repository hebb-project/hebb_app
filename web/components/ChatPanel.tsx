"use client";

import { useEffect, useRef, useState } from "react";

type Msg = { role: "user" | "core"; text: string };

const SEED_CHAT: Msg[] = [
  { role: "core", text: "core online · 142 neurons · 487 edges · awaiting input." },
  { role: "user", text: "show me what you're paying attention to right now" },
  { role: "core", text: "attention vector → cluster 3 (curiosity +0.18). 4 free-energy spikes in last 12s. nothing salient yet." },
  { role: "user", text: "what would you like to do?" },
  { role: "core", text: "rest, mostly. homeostasis drive at 0.71. but curiosity is climbing — if you have anything novel, i'd take it." },
];

type Props = {
  width: number;
  onSend?: (text: string) => void;
};

export function ChatPanel({ width, onSend }: Props) {
  const [messages, setMessages] = useState<Msg[]>(SEED_CHAT);
  const [draft, setDraft] = useState("");
  const threadRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (threadRef.current) {
      threadRef.current.scrollTop = threadRef.current.scrollHeight;
    }
  }, [messages]);

  const send = () => {
    const text = draft.trim();
    if (!text) return;
    setMessages((m) => [...m, { role: "user", text }]);
    setDraft("");
    onSend?.(text);
    setTimeout(() => {
      setMessages((m) => [
        ...m,
        {
          role: "core",
          text: `received. propagating through 142 neurons · 487 edges. free-energy ↑ ${(Math.random() * 0.2 + 0.1).toFixed(3)}.`,
        },
      ]);
    }, 900);
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
              <div className="bubble core mono">{m.text}</div>
            </div>
          )
        )}
      </div>
      <div className="chat-compose">
        <div className="chat-input-row">
          <input
            className="chat-input"
            placeholder="ask the core anything…"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") send();
            }}
          />
          <button className="btn-send mono" onClick={send}>
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
