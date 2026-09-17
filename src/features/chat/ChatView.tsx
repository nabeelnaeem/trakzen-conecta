import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { useChat } from "./store";
import { Avatar } from "../mail/Avatar";
import { bytes, shortDate, timeOnly } from "../../lib/format";
import { chat as chatIpc } from "../../lib/ipc";
import { renderMarkdown } from "../../lib/markdown";
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
              className={`flex cursor-pointer items-center gap-3 px-3 py-2.5 ${
                p.id === s.activePeerId ? "bg-white shadow-[inset_3px_0_0_#2563eb]" : "hover:bg-gray-100"
              }`}
            >
              <div className="relative">
                <Avatar name={p.displayName} seed={p.peerId ?? p.host} size={40} />
                <span
                  className={`absolute -right-0.5 -bottom-0.5 h-3 w-3 rounded-full border-2 border-gray-50 ${
                    p.online ? "bg-green-500" : "bg-gray-400"
                  }`}
                  title={p.online ? "Online" : "Offline"}
                />
              </div>
              <div className="min-w-0 flex-1">
                <div className="flex items-baseline justify-between gap-2">
                  <span className={`truncate ${p.unread ? "font-semibold" : "font-medium"}`}>{p.displayName}</span>
                  {p.lastMessageAt && <span className="shrink-0 text-[11px] text-gray-500">{shortDate(p.lastMessageAt)}</span>}
                </div>
                <div className="flex items-center justify-between gap-2">
                  <span className={`truncate text-xs ${p.unread ? "text-gray-800" : "text-gray-500"}`}>
                    {p.lastMessage ?? `${p.host}:${p.port}`}
                  </span>
                  {p.unread > 0 && <span className="rounded-full bg-blue-600 px-1.5 text-[11px] text-white">{p.unread}</span>}
                </div>
              </div>
            </li>
          ))}
          {s.peers.length === 0 && (
            <li className="px-4 py-8 text-center text-xs text-gray-500">
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
          <Conversation key={active.id} peerId={active.id} name={active.displayName} seed={active.peerId ?? active.host} host={`${active.host}:${active.port}`} online={active.online} />
        ) : (
          <div className="flex flex-1 flex-col items-center justify-center gap-2 text-sm text-gray-400">
            <span className="text-4xl">💬</span>
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
      <div className="flex items-center gap-3">
        <Avatar name={identity.displayName} seed={identity.peerId} size={36} />
        <div className="min-w-0 flex-1">
          <div className="truncate font-medium">{identity.displayName}</div>
          <div className={`text-[11px] ${listening ? "text-green-700" : "text-red-700"}`}>
            {listening ? `● listening on :${identity.port}` : "● not listening"}
          </div>
        </div>
      </div>
      <div className="mt-2 text-[11px] text-gray-600">
        Your address{identity.addresses.length > 1 ? "es" : ""}:{" "}
        <span className="select-all font-mono">{identity.addresses.length ? identity.addresses.join(", ") : "unknown"}</span>
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
          + Add peer by IP
        </button>
      </div>
    );
  }
  return (
    <div className="space-y-2 border-b border-gray-200 p-2">
      <input className="input" placeholder="Name (optional)" value={name} onChange={(e) => setName(e.target.value)} />
      <div className="flex gap-2">
        <input className="input" placeholder="192.168.1.20" value={host} autoFocus onChange={(e) => setHost(e.target.value)} onKeyDown={(e) => e.key === "Enter" && void submit()} />
        <input className="input w-24" placeholder={String(identity?.port ?? 47800)} value={port} onChange={(e) => setPort(e.target.value.replace(/\D/g, ""))} />
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

function Conversation({ peerId, name, seed, host, online }: { peerId: number; name: string; seed: string; host: string; online: boolean }) {
  const { messages, transfers, sendText, sendFile, removePeer } = useChat();
  const [text, setText] = useState("");
  const [dragging, setDragging] = useState(false);
  const bottom = useRef<HTMLDivElement>(null);
  const area = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    bottom.current?.scrollIntoView({ block: "end" });
  }, [messages.length, peerId]);

  // Tauri delivers OS drag-and-drop as paths, which is exactly what the
  // transfer needs; no need to read the files in the webview.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void getCurrentWebview()
      .onDragDropEvent((e) => {
        if (e.payload.type === "enter" || e.payload.type === "over") setDragging(true);
        else if (e.payload.type === "leave") setDragging(false);
        else if (e.payload.type === "drop") {
          setDragging(false);
          for (const p of e.payload.paths) void sendFile(p);
        }
      })
      .then((u) => (unlisten = u));
    return () => unlisten?.();
  }, [sendFile]);

  const submit = async () => {
    const body = text.trim();
    if (!body) return;
    setText("");
    await sendText(body);
    area.current?.focus();
  };

  const pick = async () => {
    const picked = await open({ multiple: true, title: "Send files" });
    if (!picked) return;
    for (const p of Array.isArray(picked) ? picked : [picked]) await sendFile(p);
  };

  const onPaste = async (e: React.ClipboardEvent) => {
    const files = Array.from(e.clipboardData.files);
    if (files.length === 0) return;
    e.preventDefault();
    for (const f of files) {
      const ext = f.type.split("/")[1] ?? "bin";
      const name = f.name && f.name !== "image.png" ? f.name : `pasted-${new Date().toISOString().replace(/[:.]/g, "-")}.${ext}`;
      const path = await chatIpc.stashBlob(name, new Uint8Array(await f.arrayBuffer()));
      await sendFile(path);
    }
  };

  // Wrap the selection (or insert a placeholder) with Markdown markers.
  const wrap = (before: string, after = before, placeholder = "text") => {
    const el = area.current;
    if (!el) return;
    const start = el.selectionStart;
    const end = el.selectionEnd;
    const sel = text.slice(start, end) || placeholder;
    const next = text.slice(0, start) + before + sel + after + text.slice(end);
    setText(next);
    requestAnimationFrame(() => {
      el.focus();
      el.setSelectionRange(start + before.length, start + before.length + sel.length);
    });
  };

  const onKey = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      // Inside an open ``` fence, Enter should add a line.
      const fences = (text.match(/```/g) ?? []).length;
      if (fences % 2 === 1) return;
      e.preventDefault();
      void submit();
      return;
    }
    if (e.ctrlKey || e.metaKey) {
      if (e.key === "b") (e.preventDefault(), wrap("**"));
      else if (e.key === "i") (e.preventDefault(), wrap("_"));
      else if (e.key === "e") (e.preventDefault(), wrap("`", "`", "code"));
      else if (e.key === "E") (e.preventDefault(), wrap("```\n", "\n```", "code"));
    }
  };

  let lastDay = "";
  let lastFrom: string | null = null;
  let lastAt = 0;

  return (
    <div className="relative flex h-full min-h-0 flex-col">
      <div className="flex items-center gap-3 border-b border-gray-200 px-4 py-2">
        <Avatar name={name} seed={seed} size={36} />
        <div className="min-w-0">
          <div className="truncate font-medium">{name}</div>
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

      <div className="flex-1 overflow-y-auto px-4 py-3">
        {messages.map((m) => {
          const day = new Date(m.createdAt).toDateString();
          const showDay = day !== lastDay;
          const grouped = !showDay && lastFrom === m.direction && m.createdAt - lastAt < 3 * 60_000;
          lastDay = day;
          lastFrom = m.direction;
          lastAt = m.createdAt;
          return (
            <div key={m.msgId}>
              {showDay && (
                <div className="my-3 flex items-center gap-3 text-[11px] text-gray-400">
                  <span className="h-px flex-1 bg-gray-200" />
                  {new Date(m.createdAt).toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" })}
                  <span className="h-px flex-1 bg-gray-200" />
                </div>
              )}
              <Bubble m={m} grouped={grouped} progress={transfers[m.msgId]} />
            </div>
          );
        })}
        <div ref={bottom} />
      </div>

      <div className="border-t border-gray-200 px-3 pt-2 pb-3">
        <div className="mb-1 flex items-center gap-1 text-xs text-gray-500">
          <FmtButton title="Bold (Ctrl+B)" onClick={() => wrap("**")}><b>B</b></FmtButton>
          <FmtButton title="Italic (Ctrl+I)" onClick={() => wrap("_")}><i>I</i></FmtButton>
          <FmtButton title="Strikethrough" onClick={() => wrap("~~")}><s>S</s></FmtButton>
          <FmtButton title="Inline code (Ctrl+E)" onClick={() => wrap("`", "`", "code")}><code>{"<>"}</code></FmtButton>
          <FmtButton title="Code block (Ctrl+Shift+E)" onClick={() => wrap("```\n", "\n```", "code")}>{"{ }"}</FmtButton>
          <FmtButton title="Bulleted list" onClick={() => wrap("- ", "", "item")}>•</FmtButton>
          <FmtButton title="Quote" onClick={() => wrap("> ", "", "quote")}>❝</FmtButton>
          <span className="ml-auto">Enter to send · Shift+Enter for a new line · paste or drop files</span>
        </div>
        <div className="flex items-end gap-2">
          <button className="btn" onClick={() => void pick()} title="Send files">
            📎
          </button>
          <textarea
            ref={area}
            className="input max-h-48 min-h-[40px] flex-1 resize-none font-sans"
            rows={1}
            placeholder={online ? "Message…" : "Peer is offline; messages will fail until it comes back"}
            value={text}
            onChange={(e) => {
              setText(e.target.value);
              e.target.style.height = "auto";
              e.target.style.height = `${Math.min(192, e.target.scrollHeight)}px`;
            }}
            onKeyDown={onKey}
            onPaste={(e) => void onPaste(e)}
          />
          <button className="btn btn-primary" onClick={() => void submit()} disabled={!text.trim()}>
            Send
          </button>
        </div>
      </div>

      {dragging && (
        <div className="pointer-events-none absolute inset-0 z-10 flex items-center justify-center bg-blue-600/10 backdrop-blur-[1px]">
          <div className="rounded-xl border-2 border-dashed border-blue-500 bg-white/90 px-8 py-6 text-lg font-medium text-blue-800">
            Drop to send to {name}
          </div>
        </div>
      )}
    </div>
  );
}

function FmtButton({ children, title, onClick }: { children: React.ReactNode; title: string; onClick: () => void }) {
  return (
    <button className="rounded px-1.5 py-0.5 hover:bg-gray-200" title={title} onMouseDown={(e) => e.preventDefault()} onClick={onClick}>
      {children}
    </button>
  );
}

function Bubble({ m, grouped, progress }: { m: ChatMessage; grouped: boolean; progress?: { bytesDone: number; bytesTotal: number } }) {
  const mine = m.direction === "out";
  const statusText =
    m.status === "sending" ? "sending…"
    : m.status === "receiving" ? "receiving…"
    : m.status === "failed" ? "failed"
    : m.status === "delivered" ? "✓✓"
    : "";
  return (
    <div className={`flex ${mine ? "justify-end" : "justify-start"} ${grouped ? "mt-0.5" : "mt-2"}`}>
      <div
        className={`max-w-[72%] px-3 py-1.5 text-sm ${
          mine ? "rounded-2xl rounded-br-md bg-blue-600 text-white" : "rounded-2xl rounded-bl-md bg-gray-100 text-gray-900"
        } ${m.status === "failed" ? "opacity-60 ring-1 ring-red-400" : ""}`}
      >
        {m.kind === "text" ? <MarkdownBody body={m.body} mine={mine} /> : <FileCard m={m} mine={mine} progress={progress} />}
        <div className={`mt-0.5 text-right text-[10px] ${mine ? "text-blue-100" : "text-gray-500"}`}>
          {timeOnly(m.createdAt)}
          {statusText && ` ${statusText}`}
        </div>
      </div>
    </div>
  );
}

function MarkdownBody({ body, mine }: { body: string; mine: boolean }) {
  const html = renderMarkdown(body);
  return (
    <div
      className={`md break-words ${mine ? "md-mine" : ""}`}
      onClick={(e) => {
        const a = (e.target as HTMLElement).closest("a");
        if (a?.href) {
          e.preventDefault();
          void openUrl(a.href);
        }
      }}
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}

function FileCard({ m, mine, progress }: { m: ChatMessage; mine: boolean; progress?: { bytesDone: number; bytesTotal: number } }) {
  const [preview, setPreview] = useState<string | null>(null);
  const done = m.status === "unread" || m.status === "received" || m.status === "delivered" || (mine && m.status !== "failed");
  useEffect(() => {
    if (!m.filePath || !done) return;
    let alive = true;
    chatIpc.filePreview(m.filePath).then((p) => alive && setPreview(p)).catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [m.filePath, done]);
  const pct = progress && progress.bytesTotal > 0 ? Math.round((progress.bytesDone / progress.bytesTotal) * 100) : null;
  return (
    <div className="min-w-[200px]">
      {preview ? (
        <img
          src={preview}
          alt={m.fileName ?? ""}
          className="mb-1 max-h-72 cursor-pointer rounded-lg"
          onClick={() => m.filePath && void chatIpc.openFile(m.filePath, false)}
        />
      ) : (
        <div className="flex items-center gap-2">
          <span className="text-xl">📄</span>
          <span className="truncate font-medium" title={m.fileName ?? ""}>
            {m.fileName}
          </span>
        </div>
      )}
      <div className={`flex items-center gap-2 text-xs ${mine ? "text-blue-100" : "text-gray-500"}`}>
        {preview && <span className="truncate">{m.fileName}</span>}
        <span>{m.fileSize !== null ? bytes(m.fileSize) : ""}</span>
        {pct !== null && <span>· {pct}%</span>}
        {done && m.filePath && (
          <>
            <button className="underline" onClick={() => void chatIpc.openFile(m.filePath!, false)}>Open</button>
            <button className="underline" onClick={() => void chatIpc.openFile(m.filePath!, true)}>Show in folder</button>
          </>
        )}
      </div>
      {pct !== null && (
        <div className={`mt-1 h-1 overflow-hidden rounded ${mine ? "bg-blue-400" : "bg-gray-300"}`}>
          <div className={`h-full ${mine ? "bg-white" : "bg-blue-600"}`} style={{ width: `${pct}%` }} />
        </div>
      )}
    </div>
  );
}
