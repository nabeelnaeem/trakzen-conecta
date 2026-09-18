import { create } from "zustand";
import { chat, errorMessage } from "../../lib/ipc";
import { notify } from "../../lib/notify";
import type { ChatMessage, ChatStatus, Identity, Nearby, Peer, TransferProgress } from "../../lib/types";

interface ChatState {
  identity: Identity | null;
  status: ChatStatus | null;
  peers: Peer[];
  activePeerId: number | null;
  messages: ChatMessage[];
  transfers: Record<string, TransferProgress>;
  nearby: Nearby[];
  /** peer row id → time the last "typing" arrived */
  typing: Record<number, number>;
  /** conecta://pair link handed in by a deep link, consumed by the Add form. */
  pendingPair: string | null;
  error: string | null;
  initialised: boolean;

  init: () => Promise<void>;
  refreshIdentity: () => Promise<void>;
  loadPeers: () => Promise<void>;
  selectPeer: (id: number | null) => Promise<void>;
  addPeer: (name: string, host: string, port?: number) => Promise<void>;
  removePeer: (id: number) => Promise<void>;
  sendText: (body: string, replyTo?: string | null) => Promise<void>;
  sendFile: (path: string) => Promise<void>;
  addNearby: (peerId: string) => Promise<void>;
  /** Select a peer by its uuid or row id (deep links). */
  selectByRoute: (peer: string) => Promise<void>;
  setPendingPair: (link: string | null) => void;
  deleteMessage: (msgId: string, forEveryone: boolean) => Promise<void>;
  react: (msgId: string, emoji: string) => Promise<void>;
  edit: (msgId: string, body: string) => Promise<boolean>;
  /** Send a message to a specific peer (used by "share to chat" from mail). */
  sendTo: (peerId: number, body: string) => Promise<void>;
  clearChat: (forEveryone: boolean) => Promise<void>;
  clearError: () => void;
}

function upsertMessage(list: ChatMessage[], m: ChatMessage): ChatMessage[] {
  const i = list.findIndex((x) => x.msgId === m.msgId);
  if (i === -1) return [...list, m];
  const next = list.slice();
  next[i] = m;
  return next;
}

export const useChat = create<ChatState>((set, get) => ({
  identity: null,
  status: null,
  peers: [],
  activePeerId: null,
  messages: [],
  transfers: {},
  nearby: [],
  typing: {},
  pendingPair: null,
  error: null,
  initialised: false,

  init: async () => {
    if (get().initialised) return;
    set({ initialised: true });
    try {
      set({ identity: await chat.identity() });
    } catch (e) {
      set({ error: errorMessage(e) });
    }
    await get().loadPeers();
    // Messages that arrived while the window was in the background are
    // only marked read once the user is actually looking at them.
    window.addEventListener("focus", () => {
      const id = get().activePeerId;
      if (id === null) return;
      void chat.markRead(id).then(() => get().loadPeers());
    });

    await chat.onStatus((status) => {
      set({ status });
      void chat.identity().then((identity) => set({ identity }));
    });
    await chat.onPeer((p) => {
      const peers = get().peers;
      const i = peers.findIndex((x) => x.id === p.id);
      if (i === -1) {
        set({ peers: [p, ...peers] });
      } else {
        const next = peers.slice();
        next[i] = p;
        set({ peers: next });
      }
      // A merge may have retired a duplicate row; refresh to drop it.
      if (p.peerId && peers.some((x) => x.id !== p.id && x.peerId === p.peerId)) {
        void get().loadPeers();
      }
    });
    await chat.onMessage((m) => {
      const { activePeerId, messages, peers } = get();
      const viewing = m.peerId === activePeerId && document.hasFocus();
      if (m.peerId === activePeerId) {
        set({ messages: upsertMessage(messages, m) });
        if (viewing && m.direction === "in" && m.status === "unread") void chat.markRead(m.peerId);
      }
      if (m.direction === "in" && (m.status === "unread") && !viewing) {
        const peer = peers.find((p) => p.id === m.peerId);
        const who = peer?.displayName ?? "New message";
        void notify(who, m.kind === "file" ? `Sent a file: ${m.fileName ?? ""}` : m.body, "chat", `conecta://chat/${peer?.peerId ?? m.peerId}`);
      }
      void get().loadPeers();
    });
    chat.nearby().then((nearby) => set({ nearby })).catch(() => undefined);
    await chat.onNearby((nearby) => set({ nearby }));
    await chat.onTyping((peerId) => {
      set({ typing: { ...get().typing, [peerId]: Date.now() } });
      window.setTimeout(() => {
        const t = get().typing;
        if (Date.now() - (t[peerId] ?? 0) >= 3900) {
          const next = { ...t };
          delete next[peerId];
          set({ typing: next });
        }
      }, 4000);
    });
    await chat.onDeleted((d) => {
      if (d.peerId === get().activePeerId) {
        set({
          messages: d.msgIds.length === 0 ? [] : get().messages.filter((m) => !d.msgIds.includes(m.msgId)),
        });
      }
      void get().loadPeers();
    });
    await chat.onTransfer((t) => {
      const transfers = { ...get().transfers };
      if (t.state === "active") transfers[t.msgId] = t;
      else delete transfers[t.msgId];
      set({ transfers });
    });
  },

  refreshIdentity: async () => {
    try {
      set({ identity: await chat.identity() });
    } catch (e) {
      set({ error: errorMessage(e) });
    }
  },

  loadPeers: async () => {
    try {
      set({ peers: await chat.listPeers() });
    } catch (e) {
      set({ error: errorMessage(e) });
    }
  },

  selectPeer: async (id) => {
    set({ activePeerId: id, messages: [] });
    if (id === null) return;
    try {
      const messages = await chat.listMessages(id, 200);
      if (get().activePeerId !== id) return;
      set({ messages });
      await chat.markRead(id);
      set({ peers: get().peers.map((p) => (p.id === id ? { ...p, unread: 0 } : p)) });
      void chat.connectPeer(id);
    } catch (e) {
      // A peer that vanished underneath us (removed elsewhere) means the
      // list is stale; reload it rather than showing a dead conversation.
      set({ error: errorMessage(e), activePeerId: null, messages: [] });
      void get().loadPeers();
    }
  },

  addPeer: async (name, host, port) => {
    try {
      const p = await chat.addPeer(name, host, port);
      set({ peers: [p, ...get().peers.filter((x) => x.id !== p.id)] });
      await get().selectPeer(p.id);
    } catch (e) {
      set({ error: errorMessage(e) });
    }
  },

  removePeer: async (id) => {
    try {
      await chat.removePeer(id);
      set({
        peers: get().peers.filter((p) => p.id !== id),
        activePeerId: get().activePeerId === id ? null : get().activePeerId,
        messages: get().activePeerId === id ? [] : get().messages,
      });
    } catch (e) {
      set({ error: errorMessage(e) });
    }
  },

  sendText: async (body, replyTo) => {
    const id = get().activePeerId;
    if (id === null || !body.trim()) return;
    try {
      const m = await chat.sendText(id, body, replyTo);
      set({ messages: upsertMessage(get().messages, m) });
    } catch (e) {
      // The failed message is already in the DB with status=failed and an
      // event has updated the list; just surface the reason.
      set({ error: errorMessage(e) });
      void get().selectPeer(id);
    }
  },

  sendFile: async (path) => {
    const id = get().activePeerId;
    if (id === null) return;
    try {
      const m = await chat.sendFile(id, path);
      set({ messages: upsertMessage(get().messages, m) });
    } catch (e) {
      set({ error: errorMessage(e) });
    }
  },

  selectByRoute: async (peer) => {
    if (get().peers.length === 0) await get().loadPeers();
    const p = get().peers.find((x) => x.peerId === peer || String(x.id) === peer);
    if (p) await get().selectPeer(p.id);
    else set({ error: "That peer is not in your list." });
  },

  setPendingPair: (link) => set({ pendingPair: link }),

  addNearby: async (peerId) => {
    try {
      const p = await chat.addNearby(peerId);
      set({ peers: [p, ...get().peers.filter((x) => x.id !== p.id)] });
      await get().selectPeer(p.id);
    } catch (e) {
      set({ error: errorMessage(e) });
    }
  },

  react: async (msgId, emoji) => {
    try {
      const m = await chat.react(msgId, emoji);
      if (m) set({ messages: upsertMessage(get().messages, m) });
    } catch (e) {
      set({ error: errorMessage(e) });
    }
  },

  edit: async (msgId, body) => {
    try {
      const m = await chat.edit(msgId, body);
      if (m) set({ messages: upsertMessage(get().messages, m) });
      return true;
    } catch (e) {
      set({ error: errorMessage(e) });
      return false;
    }
  },

  sendTo: async (peerId, body) => {
    try {
      const m = await chat.sendText(peerId, body, null);
      if (get().activePeerId === peerId) set({ messages: upsertMessage(get().messages, m) });
      void get().loadPeers();
    } catch (e) {
      set({ error: errorMessage(e) });
    }
  },

  deleteMessage: async (msgId, forEveryone) => {
    try {
      await chat.deleteMessage(msgId, forEveryone);
      set({ messages: get().messages.filter((m) => m.msgId !== msgId) });
    } catch (e) {
      set({ error: errorMessage(e) });
    }
  },

  clearChat: async (forEveryone) => {
    const id = get().activePeerId;
    if (id === null) return;
    try {
      await chat.clearChat(id, forEveryone);
      set({ messages: [] });
      void get().loadPeers();
    } catch (e) {
      set({ error: errorMessage(e) });
    }
  },

  clearError: () => set({ error: null }),
}));
