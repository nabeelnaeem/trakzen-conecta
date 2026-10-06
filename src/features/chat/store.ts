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
  /** peer row id → when the last "typing" arrived and, in a group, from whom */
  typing: Record<number, { at: number; who: string | null }>;
  /** conecta://pair link handed in by a deep link, consumed by the Add form. */
  pendingPair: string | null;
  error: string | null;
  initialised: boolean;
  /** The chat tab is on screen. Hidden tabs stay mounted, so App reports this. */
  visible: boolean;

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
  createGroup: (name: string, memberIds: number[]) => Promise<void>;
  clearChat: () => Promise<void>;
  clearError: () => void;
  setVisible: (visible: boolean) => void;
  /** Re-reads peers and the open conversation, e.g. after the machine wakes. */
  resync: () => Promise<void>;
}

/** The user can see the open conversation right now. */
function isViewing() {
  return useChat.getState().visible && document.hasFocus();
}

async function markActiveRead() {
  const id = useChat.getState().activePeerId;
  if (id === null) return;
  try {
    await chat.markRead(id);
  } catch {
    return;
  }
  const { peers } = useChat.getState();
  useChat.setState({ peers: peers.map((p) => (p.id === id ? { ...p, unread: 0 } : p)) });
  reloadPeersSoon();
}

// Focus and visibilitychange usually fire together.
let resyncTimer: number | undefined;
function resyncSoon() {
  if (resyncTimer !== undefined) return;
  resyncTimer = window.setTimeout(() => {
    resyncTimer = undefined;
    void useChat.getState().resync();
  }, 100);
}

function upsertMessage(list: ChatMessage[], m: ChatMessage): ChatMessage[] {
  const i = list.findIndex((x) => x.msgId === m.msgId);
  if (i === -1) return [...list, m];
  const next = list.slice();
  next[i] = m;
  return next;
}

// A command's return value is a snapshot from before the peer answered. If
// the event stream already put a newer copy in the list, keep that one.
function addMessage(list: ChatMessage[], m: ChatMessage): ChatMessage[] {
  return list.some((x) => x.msgId === m.msgId) ? list : [...list, m];
}

// Events for a conversation that arrive while it is being read from disk,
// so the snapshot cannot wipe them out when it lands.
interface Load {
  peerId: number;
  seen: Map<string, ChatMessage>;
  deleted: Set<string>;
  cleared: boolean;
}
const loads = new Set<Load>();

// Statuses only a later event can produce. An event copy is normally the
// fresher one, but one emitted just before a receipt must not undo it.
const SETTLED: Record<string, number> = { unread: 1, delivered: 1, received: 2, read: 2 };

function fresher(event: ChatMessage, snapshot: ChatMessage): ChatMessage {
  return (SETTLED[snapshot.status] ?? 0) > (SETTLED[event.status] ?? 0) ? snapshot : event;
}

/** Reads the conversation and merges in what arrived meanwhile; false if the user moved on. */
async function loadConversation(id: number): Promise<boolean> {
  const load: Load = { peerId: id, seen: new Map(), deleted: new Set(), cleared: false };
  loads.add(load);
  try {
    const snapshot = await chat.listMessages(id, 200);
    if (useChat.getState().activePeerId !== id) return false;
    const merged = new Map<string, ChatMessage>();
    if (!load.cleared) {
      for (const m of snapshot) if (!load.deleted.has(m.msgId)) merged.set(m.msgId, m);
    }
    for (const m of load.seen.values()) {
      const s = merged.get(m.msgId);
      merged.set(m.msgId, s ? fresher(m, s) : m);
    }
    useChat.setState({ messages: [...merged.values()].sort((a, b) => a.id - b.id) });
    return true;
  } finally {
    loads.delete(load);
  }
}

// Rows merged away, and where their history went. Adding a peer by IP can
// merge its row before the add command's own reply arrives.
const retired = new Map<number, number | null>();

// Peer list replies can land out of order (a burst of messages fires many
// reloads); only the newest request may write, and bursts share one request.
let peerSeq = 0;
let peerLoads = 0;
let peerTimer: number | undefined;

function reloadPeersSoon() {
  if (peerTimer !== undefined) return;
  peerTimer = window.setTimeout(() => {
    peerTimer = undefined;
    void useChat.getState().loadPeers();
  }, 100);
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
  visible: false,

  init: async () => {
    if (get().initialised) return;
    set({ initialised: true });
    // Listen before loading anything, so nothing emitted while the first
    // loads are in flight is missed.
    await Promise.all([
      chat.onStatus((status) => {
        set({ status });
        void chat.identity().then((identity) => set({ identity }));
      }),
      chat.onPeer((p) => {
        const peers = get().peers;
        const i = peers.findIndex((x) => x.id === p.id);
        if (i === -1) {
          set({ peers: [p, ...peers] });
        } else {
          const next = peers.slice();
          next[i] = p;
          set({ peers: next });
        }
        // A list read before this event would undo it; read again instead.
        if (peerLoads > 0) {
          peerSeq++;
          reloadPeersSoon();
        }
        // A merge may have retired a duplicate row; refresh to drop it.
        if (p.peerId && peers.some((x) => x.id !== p.id && x.peerId === p.peerId)) {
          reloadPeersSoon();
        }
      }),
      chat.onPeerRemoved(({ id, mergedInto }) => {
        retired.set(id, mergedInto);
        peerSeq++;
        set({ peers: get().peers.filter((p) => p.id !== id) });
        if (get().activePeerId === id) void get().selectPeer(mergedInto);
        reloadPeersSoon();
      }),
      chat.onMessage((m) => {
        const { activePeerId, messages, peers } = get();
        const viewing = m.peerId === activePeerId && isViewing();
        for (const l of loads) if (l.peerId === m.peerId) l.seen.set(m.msgId, m);
        if (m.peerId === activePeerId) {
          set({ messages: upsertMessage(messages, m) });
          if (viewing && m.direction === "in" && m.status === "unread") void chat.markRead(m.peerId);
        }
        if (m.direction === "in" && (m.status === "unread" || m.status === "offered") && !viewing) {
          const peer = peers.find((p) => p.id === m.peerId);
          const who = peer?.displayName ?? "New message";
          const what = m.status === "offered" ? `Wants to send you ${m.fileName ?? "a file"}` : m.kind === "file" ? `Sent a file: ${m.fileName ?? ""}` : m.body;
          const body = peer?.isGroup && m.senderName ? `${m.senderName}: ${what}` : what;
          void notify(who, body, "chat", `conecta://chat/${peer?.peerId ?? m.peerId}`);
        }
        reloadPeersSoon();
      }),
      chat.onNearby((nearby) => set({ nearby })),
      chat.onTyping(({ peerId, who }) => {
        set({ typing: { ...get().typing, [peerId]: { at: Date.now(), who } } });
        window.setTimeout(() => {
          const t = get().typing;
          if (Date.now() - (t[peerId]?.at ?? 0) >= 3900) {
            const next = { ...t };
            delete next[peerId];
            set({ typing: next });
          }
        }, 4000);
      }),
      chat.onDeleted((d) => {
        for (const l of loads) {
          if (l.peerId !== d.peerId) continue;
          if (d.msgIds.length === 0) {
            l.cleared = true;
            l.seen.clear();
          }
          for (const id of d.msgIds) {
            l.deleted.add(id);
            l.seen.delete(id);
          }
        }
        if (d.peerId === get().activePeerId) {
          set({
            messages: d.msgIds.length === 0 ? [] : get().messages.filter((m) => !d.msgIds.includes(m.msgId)),
          });
        }
        reloadPeersSoon();
      }),
      chat.onTransfer((t) => {
        const transfers = { ...get().transfers };
        if (t.state === "active" || t.state === "paused") transfers[t.msgId] = t;
        else delete transfers[t.msgId];
        set({ transfers });
      }),
    ]);
    // Coming back (focus, restore, wake from sleep) re-reads what may have
    // been missed, and marks read what arrived while nobody was looking.
    window.addEventListener("focus", resyncSoon);
    document.addEventListener("visibilitychange", () => {
      if (document.visibilityState === "visible") resyncSoon();
    });
    chat.nearby().then((nearby) => set({ nearby })).catch(() => undefined);
    await Promise.all([get().refreshIdentity(), get().loadPeers()]);
  },

  refreshIdentity: async () => {
    try {
      set({ identity: await chat.identity() });
    } catch (e) {
      set({ error: errorMessage(e) });
    }
  },

  loadPeers: async () => {
    const seq = ++peerSeq;
    peerLoads++;
    try {
      const peers = await chat.listPeers();
      if (seq === peerSeq) set({ peers });
    } catch (e) {
      if (seq === peerSeq) set({ error: errorMessage(e) });
    } finally {
      peerLoads--;
    }
  },

  selectPeer: async (id) => {
    set({ activePeerId: id, messages: [] });
    if (id === null) return;
    try {
      if (!(await loadConversation(id))) return;
      // Selected from elsewhere (a notification, share-to-chat) while the
      // tab is hidden: it is marked read when the tab is shown.
      if (get().visible) await markActiveRead();
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
      if (retired.has(p.id)) {
        reloadPeersSoon();
        await get().selectPeer(retired.get(p.id) ?? null);
        return;
      }
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
      set({ messages: addMessage(get().messages, m) });
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
      set({ messages: addMessage(get().messages, m) });
    } catch (e) {
      set({ error: errorMessage(e) });
    }
  },

  selectByRoute: async (peer) => {
    const match = (x: Peer) => x.peerId === peer || String(x.id) === peer;
    let p = get().peers.find(match);
    if (!p) {
      // Asked directly: a newer reload may have superseded the store's.
      try {
        p = (await chat.listPeers()).find(match);
      } catch (e) {
        set({ error: errorMessage(e) });
        return;
      }
      reloadPeersSoon();
    }
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
      if (get().activePeerId === peerId) set({ messages: addMessage(get().messages, m) });
      void get().loadPeers();
    } catch (e) {
      set({ error: errorMessage(e) });
    }
  },

  createGroup: async (name, memberIds) => {
    try {
      const p = await chat.createGroup(name, memberIds);
      set({ peers: [p, ...get().peers.filter((x) => x.id !== p.id)] });
      await get().selectPeer(p.id);
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

  clearChat: async () => {
    const id = get().activePeerId;
    if (id === null) return;
    try {
      await chat.clearChat(id);
      set({ messages: [] });
      void get().loadPeers();
    } catch (e) {
      set({ error: errorMessage(e) });
    }
  },

  clearError: () => set({ error: null }),

  setVisible: (visible) => {
    if (get().visible === visible) return;
    set({ visible });
    if (isViewing()) void markActiveRead();
  },

  resync: async () => {
    void get().loadPeers();
    const id = get().activePeerId;
    if (id === null) return;
    try {
      if (!(await loadConversation(id))) return;
    } catch {
      return;
    }
    if (isViewing()) await markActiveRead();
  },
}));
