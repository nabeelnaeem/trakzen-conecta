import { useEffect, useMemo, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { useChat } from "./store";
import { Avatar } from "../mail/Avatar";
import { bytes, shortDate, timeOnly } from "../../lib/format";
import { chat as chatIpc, errorMessage } from "../../lib/ipc";
import { codeFromCopyButton, isOnlyCodeBlock, renderMarkdown } from "../../lib/markdown";
import { navigateTo } from "../../lib/navigate";
import { useMail } from "../mail/store";
import { Spinner } from "../../lib/Spinner";
import type { ChatMessage, TransferProgress } from "../../lib/types";
import { confirmDialog } from "../../lib/confirm";
import { Check, CheckCheck, Clock, MoreHorizontal, Paperclip, Pencil, QrCode, Search, Send, SmilePlus, X } from "lucide-react";

const QUICK_EMOJI = ["👍", "❤️", "😂", "😮", "😢", "🙏", "✅", "👀"];

export function ChatView() {
  const s = useChat();
  const [switcher, setSwitcher] = useState(false);

  useEffect(() => {
    void s.init();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Ctrl+K: jump to a peer.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setSwitcher(true);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const active = s.peers.find((p) => p.id === s.activePeerId) ?? null;
  const knownIds = new Set(s.peers.map((p) => p.peerId).filter(Boolean));
  const nearbyNew = s.nearby.filter((n) => !knownIds.has(n.peerId));

  return (
    <div className="flex h-full">
      <aside className="flex w-72 shrink-0 flex-col border-r border-gray-200 bg-gray-50">
        <IdentityCard />
        {nearbyNew.length > 0 && (
          <div className="border-b border-gray-200 px-2 pb-2">
            <div className="px-1 pt-2 pb-1 text-[11px] font-semibold uppercase tracking-wide text-gray-500">Nearby</div>
            {nearbyNew.map((n) => (
              <div key={n.peerId} className="flex items-center gap-2 rounded-md px-1 py-1 hover:bg-gray-100">
                <Avatar name={n.displayName} seed={n.peerId} size={28} />
                <div className="min-w-0 flex-1">
                  <div className="truncate text-sm">{n.displayName}</div>
                  <div className="truncate text-[11px] text-gray-500">{n.addresses[0]}</div>
                </div>
                <button className="btn btn-primary text-xs" onClick={() => void s.addNearby(n.peerId)}>
                  Add
                </button>
              </div>
            ))}
          </div>
        )}
        <AddPeer />
        <NewGroup />
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
                  className={`absolute -right-0.5 -bottom-0.5 h-3 w-3 rounded-full border-2 border-gray-50 ${p.online ? "bg-green-500" : "bg-gray-400"}`}
                  title={p.online ? "Online" : "Offline"}
                />
              </div>
              <div className="min-w-0 flex-1">
                <div className="flex items-baseline justify-between gap-2">
                  <span className={`truncate ${p.unread ? "font-semibold" : "font-medium"}`}>
                    {p.displayName}
                    {p.isGroup ? <span className="ml-1 text-[10px] font-normal text-gray-500">group</span> : null}
                  </span>
                  {p.lastMessageAt && <span className="shrink-0 text-[11px] text-gray-500">{shortDate(p.lastMessageAt)}</span>}
                </div>
                <div className="flex items-center justify-between gap-2">
                  <span className={`truncate text-xs ${p.unread ? "text-gray-800" : "text-gray-500"}`}>
                    {s.typing[p.id] ? <em className="text-blue-700">typing…</em> : (p.lastMessage ?? `${p.host}:${p.port}`)}
                  </span>
                  {p.unread > 0 && <span className="rounded-full bg-blue-600 px-1.5 text-[11px] text-on-accent">{p.unread}</span>}
                </div>
              </div>
            </li>
          ))}
          {s.peers.length === 0 && (
            <li className="px-4 py-8 text-center text-xs text-gray-500">
              {s.nearby.length > 0
                ? "Machines running Trakzen Conecta on this network appear under Nearby — click Add."
                : "No peers yet. Other machines on this network running the app will appear here automatically; you can also add one by IP."}
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
          <Conversation
            key={active.id}
            peerId={active.id}
            name={active.displayName}
            seed={active.peerId ?? active.host}
            host={`${active.host}:${active.port}`}
            online={active.online}
            typing={!!s.typing[active.id]}
          />
        ) : (
          <div className="flex flex-1 flex-col items-center justify-center gap-2 text-sm text-gray-400">
            <span className="text-4xl">💬</span>
            Pick a peer to start chatting
            <span className="text-xs">Ctrl+K jumps to a peer</span>
          </div>
        )}
      </section>

      {switcher && <PeerSwitcher onClose={() => setSwitcher(false)} />}
    </div>
  );
}

function PeerSwitcher({ onClose }: { onClose: () => void }) {
  const { peers, selectPeer } = useChat();
  const [q, setQ] = useState("");
  const [i, setI] = useState(0);
  const list = peers.filter((p) => p.displayName.toLowerCase().includes(q.toLowerCase()) || p.host.includes(q));
  const pick = (id: number) => {
    void selectPeer(id);
    onClose();
  };
  return (
    <div className="fixed inset-0 z-30 flex items-start justify-center bg-black/30 pt-24" onClick={onClose}>
      <div className="w-[420px] rounded-lg border border-gray-300 bg-white shadow-xl" onClick={(e) => e.stopPropagation()}>
        <input
          className="w-full border-b border-gray-200 px-4 py-3 text-sm outline-none"
          placeholder="Jump to a peer…"
          autoFocus
          value={q}
          onChange={(e) => {
            setQ(e.target.value);
            setI(0);
          }}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown") setI((x) => Math.min(list.length - 1, x + 1));
            else if (e.key === "ArrowUp") setI((x) => Math.max(0, x - 1));
            else if (e.key === "Enter" && list[i]) pick(list[i].id);
            else if (e.key === "Escape") onClose();
          }}
        />
        <ul className="max-h-72 overflow-y-auto py-1">
          {list.map((p, idx) => (
            <li
              key={p.id}
              className={`flex cursor-pointer items-center gap-2 px-4 py-2 text-sm ${idx === i ? "bg-blue-50" : ""}`}
              onMouseEnter={() => setI(idx)}
              onClick={() => pick(p.id)}
            >
              <span className={`h-2 w-2 rounded-full ${p.online ? "bg-green-500" : "bg-gray-400"}`} />
              <span className="flex-1 truncate">{p.displayName}</span>
              <span className="text-xs text-gray-500">{p.host}</span>
            </li>
          ))}
          {list.length === 0 && <li className="px-4 py-3 text-sm text-gray-500">No matches</li>}
        </ul>
      </div>
    </div>
  );
}

function IdentityCard() {
  const { identity, status } = useChat();
  const [showAll, setShowAll] = useState(false);
  const [qr, setQr] = useState<[string, string] | null>(null);
  if (!identity) return null;
  const listening = status ? status.listening : identity.listening;
  const primary = identity.addresses[0];
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
        <button
          className="btn btn-ghost text-xs"
          title="Show a QR code other machines can scan to add you"
          onClick={() => (qr ? setQr(null) : chatIpc.pairingQr().then(setQr).catch(() => undefined))}
        >
          <QrCode size={16} />
        </button>
      </div>
      <div className="mt-2 text-[11px] text-gray-600">
        Your address: <span className="select-all font-mono">{primary ?? "unknown"}</span>
        {identity.addresses.length > 1 && (
          <button className="ml-1 text-blue-700 hover:underline" onClick={() => setShowAll((v) => !v)}>
            {showAll ? "hide" : `+${identity.addresses.length - 1} more`}
          </button>
        )}
        {showAll && <div className="mt-1 font-mono text-gray-500">{identity.addresses.slice(1).join(", ")}</div>}
      </div>
      {qr && (
        <div className="fixed inset-0 z-30 flex items-center justify-center bg-black/40 p-4" onClick={() => setQr(null)}>
          <div className="w-[360px] rounded-lg border border-gray-300 bg-white p-5 text-center shadow-xl" onClick={(e) => e.stopPropagation()}>
            <div className="mb-1 font-medium">Pair with this machine</div>
            <p className="mb-3 text-xs text-gray-500">Scan from another device, or paste the link into “Add peer”.</p>
            <div dangerouslySetInnerHTML={{ __html: qr[1] }} className="mx-auto rounded bg-white p-2 [&>svg]:mx-auto [&>svg]:h-52 [&>svg]:w-52" />
            <div className="mt-2 select-all break-all rounded bg-gray-50 p-2 font-mono text-[10px] text-gray-600">{qr[0]}</div>
            <div className="mt-3 flex justify-center gap-2">
              <button className="btn text-xs" onClick={() => void navigator.clipboard.writeText(qr[0])}>Copy link</button>
              <button className="btn btn-ghost text-xs" onClick={() => setQr(null)}>Close</button>
            </div>
          </div>
        </div>
      )}
      {status?.error && <div className="mt-1 text-xs text-red-700">{status.error}</div>}
    </div>
  );
}

function AddPeer() {
  const { addPeer, identity, pendingPair, setPendingPair } = useChat();
  const [name, setName] = useState("");
  const [host, setHost] = useState("");
  const [port, setPort] = useState("");
  const [openForm, setOpenForm] = useState(false);
  // A conecta://pair link opened from outside lands here prefilled.
  useEffect(() => {
    if (pendingPair) {
      setHost(pendingPair);
      setOpenForm(true);
      setPendingPair(null);
    }
  }, [pendingPair, setPendingPair]);

  const submit = async () => {
    if (!host.trim()) return;
    let h = host.trim();
    let p = port.trim() ? Number(port) : undefined;
    let n = name;
    // Pairing links from the QR code paste straight in.
    if (h.startsWith("conecta://")) {
      const u = new URL(h.replace("conecta://", "http://x/"));
      h = u.searchParams.get("host") ?? h;
      p = Number(u.searchParams.get("port")) || p;
      n = n || (u.searchParams.get("name") ?? "");
    }
    await addPeer(n, h, p);
    setName("");
    setHost("");
    setPort("");
    setOpenForm(false);
  };

  if (!openForm) {
    return (
      <div className="p-2">
        <button className="btn w-full justify-center text-xs" onClick={() => setOpenForm(true)}>
          + Add peer by IP or pairing link
        </button>
      </div>
    );
  }
  return (
    <div className="space-y-2 border-b border-gray-200 p-2">
      <input className="input" placeholder="Name (optional)" value={name} onChange={(e) => setName(e.target.value)} />
      <div className="flex gap-2">
        <input className="input" placeholder="192.168.1.20 or conecta://…" value={host} autoFocus onChange={(e) => setHost(e.target.value)} onKeyDown={(e) => e.key === "Enter" && void submit()} />
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

function NewGroup() {
  const { peers, createGroup } = useChat();
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [picked, setPicked] = useState<number[]>([]);
  const people = peers.filter((p) => !p.isGroup);
  if (!open) {
    return (
      <div className="px-2 pb-2">
        <button className="btn w-full justify-center text-xs" onClick={() => setOpen(true)}>
          + New group
        </button>
      </div>
    );
  }
  return (
    <div className="space-y-2 border-b border-gray-200 p-2">
      <input className="input" placeholder="Group name" value={name} onChange={(e) => setName(e.target.value)} />
      <div className="max-h-28 overflow-y-auto text-xs">
        {people.map((p) => (
          <label key={p.id} className="flex items-center gap-2 py-0.5">
            <input
              type="checkbox"
              checked={picked.includes(p.id)}
              onChange={() => setPicked((cur) => (cur.includes(p.id) ? cur.filter((id) => id !== p.id) : [...cur, p.id]))}
            />
            {p.displayName}
          </label>
        ))}
      </div>
      <div className="flex gap-2">
        <button
          className="btn btn-primary flex-1 justify-center text-xs"
          disabled={!name.trim() || picked.length === 0}
          onClick={() => {
            void createGroup(name, picked);
            setOpen(false);
            setName("");
            setPicked([]);
          }}
        >
          Create
        </button>
        <button className="btn text-xs" onClick={() => setOpen(false)}>
          Cancel
        </button>
      </div>
    </div>
  );
}

interface Pending {
  path: string;
  name: string;
  preview: string | null;
}

const MAX_ROWS = 8;
const CODE_LANGS = ["", "typescript", "javascript", "python", "rust", "go", "java", "csharp", "sql", "bash", "json", "yaml", "html", "css"];

function Conversation({ peerId, name, seed, host, online, typing }: { peerId: number; name: string; seed: string; host: string; online: boolean; typing: boolean }) {
  const { messages, transfers, sendText, sendFile, removePeer, clearChat, edit } = useChat();
  const [text, setText] = useState("");
  const [editing, setEditing] = useState<ChatMessage | null>(null);
  const [pending, setPending] = useState<Pending[]>([]);
  const [replyTo, setReplyTo] = useState<ChatMessage | null>(null);
  const [dragging, setDragging] = useState(false);
  const [toolbar, setToolbar] = useState(false);
  // Slack-style code mode: monospace box, Enter inserts a line, Ctrl+Enter
  // sends, the text goes out wrapped in a fenced block.
  const [codeMode, setCodeMode] = useState(false);
  const [codeLang, setCodeLang] = useState("");
  const [menu, setMenu] = useState(false);
  const [search, setSearch] = useState<string | null>(null);
  const [results, setResults] = useState<ChatMessage[] | null>(null);
  const [searching, setSearching] = useState(false);
  const bottom = useRef<HTMLDivElement>(null);
  const area = useRef<HTMLTextAreaElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const lastTyping = useRef(0);

  const byId = useMemo(() => new Map(messages.map((m) => [m.msgId, m])), [messages]);

  useEffect(() => {
    const onClick = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) setMenu(false);
    };
    document.addEventListener("mousedown", onClick);
    return () => document.removeEventListener("mousedown", onClick);
  }, []);

  // Ctrl+F searches this conversation; Esc clears whatever is in progress.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f") {
        e.preventDefault();
        setSearch((v) => v ?? "");
        window.setTimeout(() => searchRef.current?.focus(), 0);
      } else if (e.key === "Escape") {
        if (search !== null) {
          setSearch(null);
          setResults(null);
        } else if (editing) {
          setEditing(null);
          setText("");
        } else if (replyTo || pending.length || text) {
          setReplyTo(null);
          setPending([]);
          setText("");
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [search, replyTo, pending.length, text, editing]);

  useEffect(() => {
    if (search === null) return;
    if (!search.trim()) {
      setResults(null);
      return;
    }
    setSearching(true);
    const t = window.setTimeout(() => {
      chatIpc
        .search(peerId, search)
        .then(setResults)
        .catch(() => setResults([]))
        .finally(() => setSearching(false));
    }, 200);
    return () => window.clearTimeout(t);
  }, [search, peerId]);

  // 1 line to start, grows to MAX_ROWS (more room in code mode).
  useEffect(() => {
    const el = area.current;
    if (!el) return;
    el.style.height = "auto";
    const line = 22;
    const rows = codeMode ? 18 : MAX_ROWS;
    el.style.height = `${Math.min(line * rows + 18, Math.max(codeMode ? 120 : 40, el.scrollHeight))}px`;
  }, [text, codeMode]);

  useEffect(() => {
    bottom.current?.scrollIntoView({ block: "end" });
  }, [messages.length, peerId, results === null]);

  const attach = async (paths: string[]) => {
    const fresh = paths.filter((p) => !pending.some((x) => x.path === p));
    const items: Pending[] = await Promise.all(
      fresh.map(async (path) => ({
        path,
        name: path.split(/[\\/]/).pop() ?? path,
        preview: await chatIpc.filePreview(path).catch(() => null),
      })),
    );
    setPending((cur) => [...cur, ...items]);
    area.current?.focus();
  };

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void getCurrentWebview()
      .onDragDropEvent((e) => {
        if (e.payload.type === "enter" || e.payload.type === "over") setDragging(true);
        else if (e.payload.type === "leave") setDragging(false);
        else if (e.payload.type === "drop") {
          setDragging(false);
          void attach(e.payload.paths);
        }
      })
      .then((u) => (unlisten = u));
    return () => unlisten?.();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [peerId]);

  const submit = async () => {
    if (editing) {
      const body = text.trim();
      if (!body) return;
      if (await edit(editing.msgId, body)) {
        setEditing(null);
        setText("");
      }
      return;
    }
    let body = codeMode ? text.replace(/^\n+|\n+$/g, "") : text.trim();
    const files = pending;
    if (!body && files.length === 0) return;
    if (codeMode && body) body = "```" + codeLang + "\n" + body + "\n```";
    const quote = replyTo?.msgId ?? null;
    setText("");
    setPending([]);
    setReplyTo(null);
    setCodeMode(false);
    if (body) await sendText(body, quote);
    for (const f of files) await sendFile(f.path);
    area.current?.focus();
  };

  const pick = async () => {
    const picked = await open({ multiple: true, title: "Attach files" });
    if (!picked) return;
    await attach(Array.isArray(picked) ? picked : [picked]);
  };

  const onPaste = async (e: React.ClipboardEvent) => {
    const files = Array.from(e.clipboardData.files);
    if (files.length === 0) return;
    e.preventDefault();
    const paths: string[] = [];
    for (const f of files) {
      const ext = f.type.split("/")[1] ?? "bin";
      const fname = f.name && f.name !== "image.png" ? f.name : `pasted-${new Date().toISOString().replace(/[:.]/g, "-")}.${ext}`;
      paths.push(await chatIpc.stashBlob(fname, new Uint8Array(await f.arrayBuffer())));
    }
    await attach(paths);
  };

  const wrap = (before: string, after = before, placeholder = "text") => {
    const el = area.current;
    if (!el) return;
    const start = el.selectionStart;
    const end = el.selectionEnd;
    const sel = text.slice(start, end) || placeholder;
    setText(text.slice(0, start) + before + sel + after + text.slice(end));
    requestAnimationFrame(() => {
      el.focus();
      el.setSelectionRange(start + before.length, start + before.length + sel.length);
    });
  };

  const onKey = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (codeMode) {
      if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
        e.preventDefault();
        void submit();
      } else if (e.key === "Tab") {
        e.preventDefault();
        const el = e.currentTarget;
        const start = el.selectionStart;
        const end = el.selectionEnd;
        setText(text.slice(0, start) + "  " + text.slice(end));
        requestAnimationFrame(() => el.setSelectionRange(start + 2, start + 2));
      }
      return;
    }
    if (e.key === "Enter" && !e.shiftKey) {
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
      else if (e.key === "E") (e.preventDefault(), setCodeMode(true));
    }
  };

  const onChange = (v: string) => {
    setText(v);
    const now = Date.now();
    if (v && now - lastTyping.current > 3000) {
      lastTyping.current = now;
      void chatIpc.typing(peerId);
    }
  };

  const shown = results ?? messages;
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
            {typing ? <em className="text-blue-700">typing…</em> : <>{host} · {online ? <span className="text-green-700">online</span> : "offline"}</>}
          </div>
        </div>
        <div className="flex-1" />
        <button
          className="btn btn-ghost"
          title="Search in chat (Ctrl+F)"
          onClick={() => {
            setSearch("");
            window.setTimeout(() => searchRef.current?.focus(), 0);
          }}
        >
          <Search size={16} />
        </button>
        <div className="relative" ref={menuRef}>
          <button className="btn btn-ghost" onClick={() => setMenu((v) => !v)} title="More">
            <MoreHorizontal size={16} />
          </button>
          {menu && (
            <div className="absolute right-0 z-10 mt-1 w-60 rounded-md border border-gray-200 bg-white py-1 shadow-lg">
              <MenuItem
                onClick={() => {
                  setMenu(false);
                  void confirmDialog({
                    title: "Clear this chat?",
                    message: `Messages and received files from ${name} are removed from this machine only. ${name} keeps their copy.`,
                    confirmLabel: "Clear chat",
                    danger: true,
                  }).then((ok) => {
                    if (ok) void clearChat();
                  });
                }}
              >
                Clear chat for me
              </MenuItem>
              <div className="my-1 border-t border-gray-100" />
              <MenuItem
                danger
                onClick={() => {
                  setMenu(false);
                  void confirmDialog({
                    title: `Remove ${name}?`,
                    message: "The peer and the whole chat history are deleted from this machine.",
                    confirmLabel: "Remove peer",
                    danger: true,
                  }).then((ok) => {
                    if (ok) void removePeer(peerId);
                  });
                }}
              >
                Remove peer
              </MenuItem>
            </div>
          )}
        </div>
      </div>

      {search !== null && (
        <div className="flex items-center gap-2 border-b border-gray-200 bg-gray-50 px-4 py-1.5">
          <input ref={searchRef} className="input" placeholder="Search messages…" value={search} onChange={(e) => setSearch(e.target.value)} />
          {searching && <Spinner size={14} className="text-blue-600" />}
          {results && <span className="shrink-0 text-xs text-gray-500">{results.length} found</span>}
          <button
            className="btn btn-ghost text-xs"
            onClick={() => {
              setSearch(null);
              setResults(null);
            }}
          >
            ✕
          </button>
        </div>
      )}

      <div className="flex-1 overflow-y-auto">
        <div className="mx-auto w-full max-w-[880px] px-4 py-3">
          {messages.filter((m) => m.pinned).length > 0 && (
            <div className="mb-3 rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-xs text-amber-900">
              <div className="mb-1 font-semibold">Pinned</div>
              {messages.filter((m) => m.pinned).map((m) => (
                <div key={m.msgId} className="truncate">{m.kind === "file" ? m.fileName : m.body}</div>
              ))}
            </div>
          )}
          {shown.length === 0 && (
            <div className="flex flex-col items-center justify-center gap-2 py-24 text-center text-gray-400">
              {results ? (
                <span className="text-sm">Nothing matches.</span>
              ) : (
                <>
                  <Avatar name={name} seed={seed} size={56} />
                  <div className="text-base text-gray-600">Send a message to {name}</div>
                  <div className="text-xs">Messages stay on your two machines. Paste a screenshot or drop files to share them.</div>
                </>
              )}
            </div>
          )}
          {shown.map((m) => {
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
                <Bubble
                  m={m}
                  grouped={grouped}
                  progress={transfers[m.msgId]}
                  quoted={m.replyTo ? (byId.get(m.replyTo) ?? null) : null}
                  onReply={() => {
                    setReplyTo(m);
                    area.current?.focus();
                  }}
                  onEdit={() => {
                    setEditing(m);
                    setCodeMode(false);
                    setText(m.body);
                    area.current?.focus();
                  }}
                />
              </div>
            );
          })}
          <div ref={bottom} />
        </div>
      </div>

      <div className="border-t border-gray-200">
        <div className="mx-auto w-full max-w-[880px] px-3 pt-2 pb-3">
          {editing && (
            <div className="mb-2 flex items-center gap-2 rounded-md border-l-4 border-amber-500 bg-gray-50 px-3 py-1.5 text-xs">
              <Pencil size={12} className="text-amber-600" />
              <div className="min-w-0 flex-1 text-gray-700">Editing message · Enter to save, Esc to cancel</div>
              <button className="text-gray-500 hover:text-gray-900" onClick={() => { setEditing(null); setText(""); }} aria-label="Cancel edit">✕</button>
            </div>
          )}
          {replyTo && (
            <div className="mb-2 flex items-center gap-2 rounded-md border-l-4 border-blue-500 bg-gray-50 px-3 py-1.5 text-xs">
              <div className="min-w-0 flex-1">
                <div className="font-medium text-blue-700">{replyTo.direction === "out" ? "You" : name}</div>
                <div className="truncate text-gray-600">{replyTo.kind === "file" ? `📄 ${replyTo.fileName ?? ""}` : replyTo.body}</div>
              </div>
              <button className="text-gray-500 hover:text-gray-900" onClick={() => setReplyTo(null)} aria-label="Cancel reply">
                ✕
              </button>
            </div>
          )}
          {pending.length > 0 && (
            <div className="mb-2 flex flex-wrap gap-2">
              {pending.map((f) => (
                <div key={f.path} className="relative rounded-md border border-gray-200 bg-gray-50 p-1">
                  {f.preview ? (
                    <img src={f.preview} alt={f.name} className="h-20 w-20 rounded object-cover" />
                  ) : (
                    <div className="flex h-20 w-32 flex-col items-center justify-center gap-1 text-xs text-gray-600">
                      <span className="text-2xl">📄</span>
                      <span className="max-w-full truncate px-1">{f.name}</span>
                    </div>
                  )}
                  <button
                    className="absolute -top-2 -right-2 flex h-5 w-5 items-center justify-center rounded-full bg-gray-800 text-[11px] text-on-accent shadow hover:bg-red-600"
                    onClick={() => setPending((cur) => cur.filter((x) => x.path !== f.path))}
                    aria-label={`Remove ${f.name}`}
                  >
                    ✕
                  </button>
                </div>
              ))}
            </div>
          )}
          {toolbar && (
            <div className="mb-1 flex items-center gap-1 text-sm text-gray-600">
              <FmtButton title="Bold (Ctrl+B)" onClick={() => wrap("**")}><b>B</b></FmtButton>
              <FmtButton title="Italic (Ctrl+I)" onClick={() => wrap("_")}><i>I</i></FmtButton>
              <FmtButton title="Strikethrough" onClick={() => wrap("~~")}><s>S</s></FmtButton>
              <FmtButton title="Inline code (Ctrl+E)" onClick={() => wrap("`", "`", "code")}><code>{"<>"}</code></FmtButton>
              <FmtButton title="Code block (Ctrl+Shift+E)" onClick={() => setCodeMode(true)}>{"{ }"}</FmtButton>
              <FmtButton title="Bulleted list" onClick={() => wrap("- ", "", "item")}>• list</FmtButton>
              <FmtButton title="Quote" onClick={() => wrap("> ", "", "quote")}>❝ quote</FmtButton>
            </div>
          )}
          {codeMode && (
            <div className="mb-1 flex items-center gap-2 rounded-t-md border border-b-0 border-gray-700 bg-gray-800 px-3 py-1 text-xs text-gray-300">
              <span className="font-medium text-gray-100">Code block</span>
              <select className="rounded border border-gray-600 bg-gray-900 px-1 py-0.5 text-xs text-gray-100" value={codeLang} onChange={(e) => setCodeLang(e.target.value)}>
                {CODE_LANGS.map((l) => (
                  <option key={l} value={l}>
                    {l || "plain text"}
                  </option>
                ))}
              </select>
              <span className="ml-auto">Enter for a new line · Tab indents · Ctrl+Enter sends</span>
              <button className="rounded px-1.5 py-0.5 hover:bg-white/10" onClick={() => setCodeMode(false)} title="Back to normal message">
                ✕
              </button>
            </div>
          )}
          <div className="flex items-end gap-2">
            <button className="btn" onClick={() => void pick()} title="Attach files">
              <Paperclip size={16} />
            </button>
            <button className={`btn ${toolbar ? "bg-gray-100" : ""}`} onClick={() => setToolbar((v) => !v)} title="Formatting (Markdown also works: **bold**, _italic_, `code`)">
              Aa
            </button>
            <button className={`btn ${codeMode ? "bg-gray-800 text-on-accent" : ""}`} onClick={() => setCodeMode((v) => !v)} title="Code block (Ctrl+Shift+E)">
              {"{ }"}
            </button>
            <textarea
              ref={area}
              className={`input min-h-[40px] flex-1 resize-none ${codeMode ? "rounded-t-none border-gray-700 bg-gray-900 font-mono text-[13px] text-gray-100 placeholder:text-gray-500 focus:border-gray-500 focus:ring-0" : "font-sans"}`}
              rows={1}
              spellCheck={!codeMode}
              placeholder={codeMode ? "Paste or type code…" : online ? "Message… (Enter to send, Shift+Enter for a new line)" : "Peer is offline — messages are queued and sent when it returns"}
              value={text}
              onChange={(e) => onChange(e.target.value)}
              onKeyDown={onKey}
              onPaste={(e) => void onPaste(e)}
            />
            <button className="btn btn-primary" onClick={() => void submit()} disabled={!text.trim() && pending.length === 0} title="Send (Enter)">
              <Send size={15} />
              {pending.length > 0 ? ` ${pending.length}` : ""}
            </button>
          </div>
        </div>
      </div>

      {dragging && (
        <div className="pointer-events-none absolute inset-0 z-10 flex items-center justify-center bg-blue-600/10 backdrop-blur-[1px]">
          <div className="rounded-xl border-2 border-dashed border-blue-500 bg-white/90 px-8 py-6 text-lg font-medium text-blue-800">
            Drop to attach for {name}
          </div>
        </div>
      )}
    </div>
  );
}

function MenuItem({ children, onClick, danger }: { children: React.ReactNode; onClick: () => void; danger?: boolean }) {
  return (
    <button className={`block w-full px-3 py-1.5 text-left text-sm hover:bg-gray-50 ${danger ? "text-red-700" : ""}`} onClick={onClick}>
      {children}
    </button>
  );
}

function FmtButton({ children, title, onClick }: { children: React.ReactNode; title: string; onClick: () => void }) {
  return (
    <button className="rounded border border-gray-200 bg-white px-2 py-0.5 text-gray-700 hover:bg-gray-100" title={title} onMouseDown={(e) => e.preventDefault()} onClick={onClick}>
      {children}
    </button>
  );
}

function Ticks({ status, mine }: { status: string; mine: boolean }) {
  if (!mine) return null;
  if (status === "queued") return <Clock size={11} aria-label="Queued until the peer is online" />;
  if (status === "sending") return <Check size={12} aria-label="Sending" />;
  if (status === "failed") return <X size={12} aria-label="Failed" />;
  if (status === "read") return <CheckCheck size={13} className="text-cyan-300" aria-label="Read" />;
  return <CheckCheck size={13} className="opacity-70" aria-label="Delivered" />;
}

function Bubble({
  m,
  grouped,
  progress,
  quoted,
  onReply,
  onEdit,
}: {
  m: ChatMessage;
  grouped: boolean;
  progress?: TransferProgress;
  quoted: ChatMessage | null;
  onReply: () => void;
  onEdit: () => void;
}) {
  const mine = m.direction === "out";
  const { deleteMessage, react, peers } = useChat();
  const openCompose = useMail((s) => s.openCompose);
  const updateComposer = useMail((s) => s.updateComposer);
  const [menu, setMenu] = useState(false);
  const [emojiRow, setEmojiRow] = useState(false);
  // Images and code blocks stand on their own; only plain text gets the fill.
  const frameless = m.kind === "file" || (m.kind === "text" && isOnlyCodeBlock(m.body));
  const peerName = peers.find((p) => p.id === m.peerId)?.displayName ?? "peer";
  const sendAsEmail = async () => {
    setMenu(false);
    await openCompose();
    const body = m.kind === "file" ? "" : m.body;
    updateComposer({ subject: `Chat with ${peerName}`, body, files: m.kind === "file" && m.filePath ? [m.filePath] : [] });
    navigateTo("mail");
  };
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!menu) return;
    const onClick = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setMenu(false);
    };
    document.addEventListener("mousedown", onClick);
    return () => document.removeEventListener("mousedown", onClick);
  }, [menu]);

  const menuEl = (
    <div className="relative self-center" ref={ref}>
      <button
        className={`rounded px-1 text-gray-400 hover:bg-gray-100 hover:text-gray-700 ${menu ? "opacity-100" : "opacity-0 group-hover:opacity-100"}`}
        onClick={() => setMenu(!menu)}
        aria-label="Message options"
      >
        ▾
      </button>
      {menu && (
        <div className={`absolute top-full z-10 mt-1 w-48 rounded-md border border-gray-200 bg-white py-1 shadow-lg ${mine ? "right-0" : "left-0"}`}>
          <div className="flex justify-around px-2 py-1">
            {QUICK_EMOJI.slice(0, 6).map((e) => (
              <button key={e} className="rounded px-1 text-base hover:bg-gray-100" onClick={() => { setMenu(false); void react(m.msgId, e); }}>
                {e}
              </button>
            ))}
          </div>
          <div className="my-1 border-t border-gray-100" />
          <MenuItem
            onClick={() => {
              setMenu(false);
              onReply();
            }}
          >
            Reply
          </MenuItem>
          {mine && m.kind === "text" && (
            <MenuItem
              onClick={() => {
                setMenu(false);
                onEdit();
              }}
            >
              Edit
            </MenuItem>
          )}
          <MenuItem onClick={() => void sendAsEmail()}>Send as email…</MenuItem>
          <MenuItem
            onClick={() => {
              setMenu(false);
              void chatIpc.pin(m.msgId, !m.pinned).then((next) => {
                if (!next) return;
                const list = useChat.getState().messages;
                const i = list.findIndex((x) => x.msgId === next.msgId);
                useChat.setState({
                  messages: i === -1 ? [...list, next] : list.map((x, n) => (n === i ? next : x)),
                });
              });
            }}
          >
            {m.pinned ? "Unpin" : "Pin"}
          </MenuItem>
          <MenuItem
            onClick={() => {
              setMenu(false);
              void chatIpc.pin(m.msgId, !m.pinned).then((next) => next && useChat.setState((s) => ({ messages: s.messages.map((x) => (x.msgId === next.msgId ? next : x)) })));
            }}
          >
            {m.pinned ? "Unpin" : "Pin"}
          </MenuItem>
          <MenuItem
            onClick={() => {
              setMenu(false);
              void navigator.clipboard.writeText(m.kind === "file" ? (m.filePath ?? m.fileName ?? "") : m.body);
            }}
          >
            Copy
          </MenuItem>
          <div className="my-1 border-t border-gray-100" />
          <MenuItem
            onClick={() => {
              setMenu(false);
              void deleteMessage(m.msgId, false);
            }}
          >
            Delete for me
          </MenuItem>
          {mine && (
            <MenuItem
              danger
              onClick={() => {
                setMenu(false);
                void deleteMessage(m.msgId, true);
              }}
            >
              Delete for everyone
            </MenuItem>
          )}
        </div>
      )}
    </div>
  );

  return (
    <div
      className={`group flex items-end gap-1 ${mine ? "justify-end" : "justify-start"} ${grouped ? "mt-0.5" : "mt-2"}`}
      onContextMenu={(e) => {
        e.preventDefault();
        setMenu(true);
      }}
    >
      {mine && menuEl}
      {mine && (
        <button
          className={`rounded px-1 text-gray-400 hover:bg-gray-100 hover:text-gray-700 ${emojiRow ? "opacity-100" : "opacity-0 group-hover:opacity-100"}`}
          onClick={() => setEmojiRow((v) => !v)}
          aria-label="React"
        >
          <SmilePlus size={14} />
        </button>
      )}
      <div className={`flex flex-col ${mine ? "items-end" : "items-start"} ${m.body.includes("```") || m.kind === "file" ? "max-w-[85%]" : "max-w-[60%]"}`}>
      <div
        className={`${frameless ? "p-0" : "px-3 py-1.5"} text-sm ${
          frameless
            ? "text-gray-900"
            : mine
              ? "rounded-2xl rounded-br-md bg-bubble-own text-bubble-own-fg"
              : "rounded-2xl rounded-bl-md bg-bubble-peer text-bubble-peer-fg"
        } ${m.status === "failed" ? "opacity-60 ring-1 ring-red-400" : ""}`}
      >
        {m.replyTo && (
          <div className={`mb-1 rounded border-l-2 px-2 py-0.5 text-xs ${mine ? "border-blue-200 bg-blue-500/60 text-blue-50" : "border-blue-500 bg-white/70 text-gray-600"}`}>
            {quoted ? (quoted.kind === "file" ? `📄 ${quoted.fileName ?? ""}` : quoted.body.slice(0, 140)) : "Original message unavailable"}
          </div>
        )}
        {m.kind === "text" ? <MarkdownBody body={m.body} mine={mine && !frameless} /> : <FileCard m={m} mine={mine} frameless progress={progress} />}
        {m.preview?.title && (
          <a href={m.preview.url} className="mt-1 block overflow-hidden rounded-lg border border-gray-200 bg-white text-left text-xs text-gray-800" onClick={(e) => { e.preventDefault(); void openUrl(m.preview!.url); }}>
            {m.preview.image && <img src={m.preview.image} alt="" className="max-h-32 w-full object-cover" />}
            <div className="px-2 py-1 font-medium">{m.preview.title}</div>
            {m.preview.description && <div className="px-2 pb-1 text-gray-500 line-clamp-2">{m.preview.description}</div>}
          </a>
        )}
        {m.preview?.title && (
          <a href={m.preview.url} className="mt-1 block overflow-hidden rounded-md border border-gray-200 bg-white text-left text-xs text-gray-800" onClick={(e) => { e.preventDefault(); void openUrl(m.preview!.url); }}>
            {m.preview.image && <img src={m.preview.image} alt="" className="max-h-32 w-full object-cover" />}
            <div className="px-2 py-1 font-medium">{m.preview.title}</div>
            {m.preview.description && <div className="px-2 pb-1 text-gray-500">{m.preview.description}</div>}
          </a>
        )}
        {!frameless && (
          <div className={`mt-0.5 flex items-center justify-end gap-1 text-[10px] ${mine ? "text-bubble-own-fg/70" : "text-gray-500"}`}>
            {m.editedAt && <span title={new Date(m.editedAt).toLocaleString()}>edited ·</span>}
            {timeOnly(m.createdAt)}
            <Ticks status={m.status} mine={mine} />
          </div>
        )}
      </div>
      {frameless && (
        <div className={`mt-0.5 flex items-center gap-1 px-1 text-[10px] text-gray-500 ${mine ? "flex-row-reverse" : ""}`}>
          {timeOnly(m.createdAt)}
          {m.editedAt && <span>· edited</span>}
          <Ticks status={m.status} mine={mine} />
        </div>
      )}
      {Object.keys(m.reactions ?? {}).length > 0 && (
        <div className={`mt-0.5 flex flex-wrap gap-1 ${mine ? "justify-end" : ""}`}>
          {Object.entries(m.reactions).map(([emoji, who]) => (
            <button
              key={emoji}
              className={`rounded-full border px-1.5 py-0.5 text-xs ${who.includes("me") ? "border-blue-400 bg-blue-50 text-blue-900" : "border-gray-200 bg-white text-gray-700 hover:bg-gray-50"}`}
              onClick={() => void react(m.msgId, emoji)}
              title={who.map((w) => (w === "me" ? "You" : peerName)).join(", ")}
            >
              {emoji} {who.length > 1 ? who.length : ""}
            </button>
          ))}
        </div>
      )}
      {emojiRow && (
        <div className="mt-1 flex gap-1 rounded-full border border-gray-200 bg-white px-2 py-1 shadow">
          {QUICK_EMOJI.map((e) => (
            <button key={e} className="rounded px-0.5 text-base hover:bg-gray-100" onClick={() => { setEmojiRow(false); void react(m.msgId, e); }}>
              {e}
            </button>
          ))}
        </div>
      )}
      </div>
      {!mine && (
        <button
          className={`rounded px-1 text-gray-400 hover:bg-gray-100 hover:text-gray-700 ${emojiRow ? "opacity-100" : "opacity-0 group-hover:opacity-100"}`}
          onClick={() => setEmojiRow((v) => !v)}
          aria-label="React"
        >
          <SmilePlus size={14} />
        </button>
      )}
      {!mine && menuEl}
    </div>
  );
}

function MarkdownBody({ body, mine }: { body: string; mine: boolean }) {
  const html = renderMarkdown(body).replace(/@([\w.-]+)/g, '<span class="font-semibold text-blue-800">@$1</span>');
  return (
    <div
      className={`md break-words ${mine ? "md-mine" : ""}`}
      onClick={(e) => {
        const t = e.target as HTMLElement;
        const copy = t.closest(".md-copy") as HTMLElement | null;
        if (copy) {
          const code = codeFromCopyButton(copy);
          if (code !== null) {
            void navigator.clipboard.writeText(code);
            copy.textContent = "Copied";
            window.setTimeout(() => (copy.textContent = "Copy"), 1500);
          }
          return;
        }
        const a = t.closest("a");
        if (a?.href) {
          e.preventDefault();
          void openUrl(a.href);
        }
      }}
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}

function FileCard({ m, mine, progress, frameless }: { m: ChatMessage; mine: boolean; progress?: TransferProgress; frameless?: boolean }) {
  void mine;
  const [preview, setPreview] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const done = m.status === "unread" || m.status === "received" || m.status === "delivered" || m.status === "read" || (mine && m.status !== "failed");
  useEffect(() => {
    if (!m.filePath || !done) return;
    let alive = true;
    chatIpc.filePreview(m.filePath).then((p) => alive && setPreview(p)).catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [m.filePath, done]);
  const pct = progress && progress.bytesTotal > 0 ? Math.round((progress.bytesDone / progress.bytesTotal) * 100) : null;
  const openIt = (reveal: boolean) => m.filePath && chatIpc.openFile(m.filePath, reveal).catch((e) => setErr(errorMessage(e)));
  return (
    <div className={`min-w-[200px] ${frameless && !preview ? "rounded-xl border border-gray-200 bg-white px-3 py-2" : ""}`}>
      {preview ? (
        <img src={preview} alt={m.fileName ?? ""} className="mb-1 max-h-80 cursor-pointer rounded-xl border border-gray-200" onClick={() => void openIt(false)} />
      ) : (
        <div className="flex items-center gap-2">
          <Paperclip size={16} className="text-gray-500" />
          <span className="truncate font-medium" title={m.fileName ?? ""}>
            {m.fileName}
          </span>
        </div>
      )}
      <div className="flex items-center gap-2 text-xs text-gray-500">
        {preview && <span className="truncate">{m.fileName}</span>}
        <span>{m.fileSize !== null ? bytes(m.fileSize) : ""}</span>
        {pct !== null && <span>· {pct}%</span>}
        {progress && (progress.state === "active" || progress.state === "paused") && (
          <button
            className="underline"
            onClick={() => void chatIpc.pauseTransfer(progress.transferId, progress.state !== "paused")}
          >
            {progress.state === "paused" ? "Resume" : "Pause"}
          </button>
        )}
        {done && m.filePath && (
          <>
            <button className="underline" onClick={() => void openIt(false)}>
              Open
            </button>
            <button className="underline" onClick={() => void openIt(true)}>
              Show in folder
            </button>
          </>
        )}
        {done && !m.filePath && <span>(file removed)</span>}
        {!mine && m.status === "interrupted" && <span>· waiting for the sender to reconnect</span>}
        {!mine && m.status === "receiving" && !progress && <span>· receiving…</span>}
        {mine && m.status === "queued" && <span>· queued</span>}
      </div>
      {err && <div className="text-xs text-red-700">{err}</div>}
      {pct !== null && (
        <div className="mt-1 h-1 overflow-hidden rounded bg-gray-300">
          <div className="h-full bg-blue-600" style={{ width: `${pct}%` }} />
        </div>
      )}
    </div>
  );
}
