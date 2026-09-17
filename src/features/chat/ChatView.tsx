import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useChat } from "./store";
import { bytes, initials, shortDate, timeOnly } from "../../lib/format";
import { chat as chatIpc } from "../../lib/ipc";
import type { ChatMessage } from "../../lib/types";

export function ChatView() {
  const s = useChat();

  useEffect(() => {
    void s.init();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const active = s.peers.find((p) => p.id === s.activePeerId) ?? null;

  return (
    <div className="flex h-full">
      <aside className="flex w-72 shrink-0 flex-col border-r border-gray-200 bg-gray-50">
        <IdentityCard />
        <AddPeer />
        <ul className="flex-1 overflow-y-auto">
          {s.peers.map((p) => (
            <li
              key={p.id}
              onClick={() => void s.selectPeer(p.id)}
              className={`flex cursor-pointer items-center gap-2 px-3 py-2 ${
                p.id === s.activePeerId ? "bg-blue-100" : "hover:bg-gray-200"
              }`}
            >
              <div className="relative">
                <div className="flex h-9 w-9 items-center justify-center rounded-full bg-gray-300 text-xs font-semibold text-gray-700">
                  {initials(p.displayName)}
                </div>
                <span
                  className={`absolute -right-0.5 -bottom-0.5 h-3 w-3 rounded-full border-2 border-gray-50 ${
                    p.online ? "bg-green-500" : "bg-gray-400"
                  }`}
                  title={p.online ? "Online" : "Offline"}
                />
              </div>
              <div className="min-w-0 flex-1">
                <div className="flex items-baseline justify-between gap-2">
                  <span className="truncate font-medium">{p.displayName}</span>
                  {p.lastMessageAt && (
                    <span className="shrink-0 text-xs text-gray-500">{shortDate(p.lastMessageAt)}</span>
                  )}
                </div>
                <div className="flex items-center justify-between gap-2">
                  <span className="truncate text-xs text-gray-500">
                    {p.lastMessage ?? `${p.host}:${p.port}`}
                  </span>
                  {p.unread > 0 && (
                    <span className="rounded-full bg-blue-600 px-1.5 text-xs text-white">{p.unread}</span>
                  )}
                </div>
              </div>
            </li>
          ))}
          {s.peers.length === 0 && (
            <li className="px-3 py-6 text-center text-xs text-gray-500">
              No peers yet. Add one by IP address above, or ask someone on your network to add yours.
            </li>
          )}
        </ul>
      </aside>

      <section className="flex min-w-0 flex-1 flex-col bg-white">
        {s.error && (
          <div className="flex items-start justify-between gap-3 border-b border-red-200 bg-red-50 px-3 py-2 text-sm text-red-800">
            <span>{s.error}</span>
            <button onClick={s.clearError} aria-label="Dismiss">
              ✕
            </button>
          </div>
        )}
        {active ? (
          <Conversation peerId={active.id} name={active.displayName} host={`${active.host}:${active.port}`} online={active.online} />
        ) : (
          <div className="flex flex-1 items-center justify-center text-sm text-gray-400">
            Pick a peer to start chatting
          </div>
        )}
      </section>
    </div>
  );
}

function IdentityCard() {
  const { identity, status } = useChat();
  if (!identity) return null;
  const listening = status ? status.listening : identity.listening;
  return (
    <div className="border-b border-gray-200 p-3">
      <div className="flex items-center justify-between">
        <span className="font-medium">{identity.displayName}</span>
        <span className={`text-xs ${listening ? "text-green-700" : "text-red-700"}`}>
          {listening ? `listening :${identity.port}` : "not listening"}
        </span>
      </div>
      <div className="mt-1 text-xs text-gray-600">
        Your address{identity.addresses.length > 1 ? "es" : ""}:{" "}
        {identity.addresses.length ? identity.addresses.join(", ") : "unknown"}
      </div>
      {status?.error && <div className="mt-1 text-xs text-red-700">{status.error}</div>}
    </div>
  );
}

function AddPeer() {
  const { addPeer, identity } = useChat();
  const [name, setName] = useState("");
  const [host, setHost] = useState("");
  const [port, setPort] = useState("");
  const [openForm, setOpenForm] = useState(false);

  const submit = async () => {
    if (!host.trim()) return;
    const p = port.trim() ? Number(port) : undefined;
    await addPeer(name, host, p);
    setName("");
    setHost("");
    setPort("");
    setOpenForm(false);
  };

  if (!openForm) {
    return (
      <div className="p-2">
        <button className="btn w-full justify-center" onClick={() => setOpenForm(true)}>
          Add peer by IP
        </button>
      </div>
    );
  }
  return (
    <div className="space-y-2 border-b border-gray-200 p-2">
      <input className="input" placeholder="Name (optional)" value={name} onChange={(e) => setName(e.target.value)} />
      <div className="flex gap-2">
        <input
          className="input"
          placeholder="192.168.1.20"
          value={host}
          autoFocus
          onChange={(e) => setHost(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && void submit()}
        />
        <input
          className="input w-24"
          placeholder={String(identity?.port ?? 47800)}
          value={port}
          onChange={(e) => setPort(e.target.value.replace(/\D/g, ""))}
        />
      </div>
      <div className="flex gap-2">
        <button className="btn btn-primary flex-1 justify-center" onClick={() => void submit()} disabled={!host.trim()}>
          Add
        </button>
        <button className="btn" onClick={() => setOpenForm(false)}>
          Cancel
        </button>
      </div>
    </div>
  );
}

function Conversation({ peerId, name, host, online }: { peerId: number; name: string; host: string; online: boolean }) {
  const { messages, transfers, sendText, sendFile, removePeer } = useChat();
  const [text, setText] = useState("");
  const bottom = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottom.current?.scrollIntoView({ block: "end" });
  }, [messages.length, peerId]);

  const submit = async () => {
    const body = text.trim();
    if (!body) return;
    setText("");
    await sendText(body);
  };

  const pick = async () => {
    const picked = await open({ multiple: true, title: "Send files" });
    if (!picked) return;
    for (const p of Array.isArray(picked) ? picked : [picked]) await sendFile(p);
  };

  return (
    <>
      <div className="flex items-center gap-3 border-b border-gray-200 px-4 py-2">
        <div>
          <div className="font-medium">{name}</div>
          <div className="text-xs text-gray-500">
            {host} · {online ? <span className="text-green-700">online</span> : "offline"}
          </div>
        </div>
        <div className="flex-1" />
        <button
          className="btn btn-ghost text-xs text-red-700"
          onClick={() => {
            if (confirm(`Remove ${name} and the chat history?`)) void removePeer(peerId);
          }}
        >
          Remove
        </button>
      </div>

      <div className="flex-1 space-y-1 overflow-y-auto px-4 py-3">
        {messages.map((m) => (
          <Bubble key={m.msgId} m={m} progress={transfers[m.msgId]} />
        ))}
        <div ref={bottom} />
      </div>

      <div className="flex items-end gap-2 border-t border-gray-200 p-3">
        <button className="btn" onClick={() => void pick()} title="Send a file">
          📎
        </button>
        <textarea
          className="input max-h-40 min-h-[38px] flex-1 resize-none"
          rows={1}
          placeholder={online ? "Message… (Enter to send, Shift+Enter for newline)" : "Peer is offline; messages will fail until it comes back"}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              void submit();
            }
          }}
        />
        <button className="btn btn-primary" onClick={() => void submit()} disabled={!text.trim()}>
          Send
        </button>
      </div>
    </>
  );
}

function Bubble({ m, progress }: { m: ChatMessage; progress?: { bytesDone: number; bytesTotal: number } }) {
  const mine = m.direction === "out";
  const statusText =
    m.status === "sending" ? "sending…"
    : m.status === "receiving" ? "receiving…"
    : m.status === "failed" ? "failed"
    : m.status === "delivered" ? "delivered"
    : "";
  return (
    <div className={`flex ${mine ? "justify-end" : "justify-start"}`}>
      <div
        className={`max-w-[70%] rounded-2xl px-3 py-1.5 text-sm ${
          mine ? "bg-blue-600 text-white" : "bg-gray-100 text-gray-900"
        } ${m.status === "failed" ? "opacity-60 ring-1 ring-red-400" : ""}`}
      >
        {m.kind === "text" ? (
          <div className="whitespace-pre-wrap break-words">{m.body}</div>
        ) : (
          <FileCard m={m} mine={mine} progress={progress} />
        )}
        <div className={`mt-0.5 text-[10px] ${mine ? "text-blue-100" : "text-gray-500"}`}>
          {timeOnly(m.createdAt)}
          {statusText && ` · ${statusText}`}
        </div>
      </div>
    </div>
  );
}

function FileCard({ m, mine, progress }: { m: ChatMessage; mine: boolean; progress?: { bytesDone: number; bytesTotal: number } }) {
  const pct = progress && progress.bytesTotal > 0 ? Math.round((progress.bytesDone / progress.bytesTotal) * 100) : null;
  const canOpen = !!m.filePath && (m.status === "unread" || m.status === "received" || m.status === "delivered" || mine);
  return (
    <div className="min-w-[180px]">
      <div className="flex items-center gap-2">
        <span>📄</span>
        <span className="truncate font-medium" title={m.fileName ?? ""}>
          {m.fileName}
        </span>
      </div>
      <div className={`text-xs ${mine ? "text-blue-100" : "text-gray-500"}`}>
        {m.fileSize !== null ? bytes(m.fileSize) : ""}
        {pct !== null && ` · ${pct}%`}
      </div>
      {pct !== null && (
        <div className={`mt-1 h-1 overflow-hidden rounded ${mine ? "bg-blue-400" : "bg-gray-300"}`}>
          <div className={`h-full ${mine ? "bg-white" : "bg-blue-600"}`} style={{ width: `${pct}%` }} />
        </div>
      )}
      {canOpen && (
        <div className="mt-1 flex gap-2 text-xs">
          <button className="underline" onClick={() => void chatIpc.openFile(m.filePath!, false)}>
            Open
          </button>
          <button className="underline" onClick={() => void chatIpc.openFile(m.filePath!, true)}>
            Show in folder
          </button>
        </div>
      )}
    </div>
  );
}
