import { create } from "zustand";
import { chat, errorMessage } from "../../lib/ipc";
import { notify } from "../../lib/notify";
import type { ChatMessage, ChatStatus, Identity, Nearby, Peer, TransferProgress } from "../../lib/types";

/** A file waiting in the composer to be sent. */
export interface PendingFile {
  path: string;
  name: string;
  preview: string | null;
}

/** An unsent message, kept while other conversations are open. */
export interface Draft {
  text: string;
  files: PendingFile[];
  replyTo: ChatMessage | null;
  codeMode: boolean;
  codeLang: string;
}

interface ChatState {
  identity: Identity | null;
  status: ChatStatus | null;
  peers: Peer[];
  activePeerId: number | null;
  /** The newest messages of the open conversation; older pages are added on demand. */
  messages: ChatMessage[];
  /** There is history before the first loaded message. */
  hasOlder: boolean;
  loadingOlder: boolean;
  /** Pinned messages of the open conversation, loaded or not. */
  pinned: ChatMessage[];
  /** Live transfers by transfer id; a group file has one per member. */
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
  /** peer row id → unsent message */
  drafts: Record<number, Draft>;

  init: () => Promise<void>;
  refreshIdentity: () => Promise<void>;
  loadPeers: () => Promise<void>;
  selectPeer: (id: number | null) => Promise<void>;
  /** Puts the page before the first loaded message in front; resolves to how many were added. */
  loadOlder: (limit?: number) => Promise<number>;
  /** Loads older pages until `m` is in `messages`; false if it cannot be reached. */
  jumpTo: (m: ChatMessage) => Promise<boolean>;
  /** Puts a changed message of the open conversation in place. */
  applyMessage: (m: ChatMessage) => void;
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
  setDraft: (peerId: number, draft: Draft) => void;
  /** Re-reads peers and the open conversation, e.g. after the machine wakes. */
  resync: () => Promise<void>;
}

// Only the text survives a restart: attachments may be temporary files and a
// reply target may be gone by then.
const DRAFT_KEY = "tc.chatDraft.";

export function saveDraftText(peerId: number, text: string) {
  try {
    if (text) localStorage.setItem(DRAFT_KEY + peerId, text);
    else localStorage.removeItem(DRAFT_KEY + peerId);
  } catch {
    return;
  }
}

function savedDrafts(): Record<number, Draft> {
  const out: Record<number, Draft> = {};
  try {
    for (let i = 0; i < localStorage.length; i++) {
      const key = localStorage.key(i);
      const text = key?.startsWith(DRAFT_KEY) ? localStorage.getItem(key) : null;
      if (key && text) out[Number(key.slice(DRAFT_KEY.length))] = { text, files: [], replyTo: null, codeMode: false, codeLang: "" };
    }
  } catch {
    return out;
  }
  return out;
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

/** A file message's transfers summed up for display. */
export interface MessageProgress {
  transferIds: string[];
  bytesDone: number;
  bytesTotal: number;
  state: "active" | "paused";
}

export function progressByMessage(transfers: Record<string, TransferProgress>): Record<string, MessageProgress> {
  const out: Record<string, MessageProgress> = {};
  for (const t of Object.values(transfers)) {
    const p = out[t.msgId] ?? (out[t.msgId] = { transferIds: [], bytesDone: 0, bytesTotal: 0, state: "paused" });
    p.transferIds.push(t.transferId);
    p.bytesDone += t.bytesDone;
    p.bytesTotal += t.bytesTotal;
    if (t.state === "active") p.state = "active";
  }
  return out;
}

/** Author of an incoming group message under their current name; the name
 * stored on the message is from when it was loaded. */
export function senderName(m: ChatMessage, peers: Peer[]): string | null {
  return (m.senderId && peers.find((p) => p.peerId === m.senderId)?.displayName) || m.senderName;
}

function upsertMessage(list: ChatMessage[], m: ChatMessage, hasOlder: boolean): ChatMessage[] {
  const i = list.findIndex((x) => x.msgId === m.msgId);
  if (i === -1) {
    // A change to a message before the loaded window; it shows when that page loads.
    if (hasOlder && list.length > 0 && m.id < list[0].id) return list;
    const at = list.findIndex((x) => x.id > m.id);
    return at === -1 ? [...list, m] : [...list.slice(0, at), m, ...list.slice(at)];
  }
  const next = list.slice();
  next[i] = m;
  return next;
}

function withPinned(list: ChatMessage[], m: ChatMessage): ChatMessage[] {
  const rest = list.filter((x) => x.msgId !== m.msgId);
  if (!m.pinned) return rest.length === list.length ? list : rest;
  return [...rest, m].sort((a, b) => a.id - b.id);
}

function applied(st: ChatState, m: ChatMessage): Partial<ChatState> {
  return { messages: upsertMessage(st.messages, m, st.hasOlder), pinned: withPinned(st.pinned, m) };
}

function prependMessages(list: ChatMessage[], page: ChatMessage[]): ChatMessage[] {
  const have = new Set(list.map((m) => m.msgId));
  return [...page.filter((m) => !have.has(m.msgId)), ...list].sort((a, b) => a.id - b.id);
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

const FIRST_PAGE = 200;
const OLDER_PAGE = 100;
/** The most `chat_list_messages` returns at once. */
const MAX_PAGE = 500;
const JUMP_PAGES = 10;

/** Every message from row `from` on, newest pages first. */
async function listSince(peerId: number, from: number): Promise<ChatMessage[]> {
  let out: ChatMessage[] = [];
  let before: number | undefined;
  for (;;) {
    const page = await chat.listMessages(peerId, MAX_PAGE, before);
    out = [...page, ...out];
    if (page.length < MAX_PAGE || page[0].id <= from) return out.filter((m) => m.id >= from);
    before = page[0].id;
  }
}

/** Reads the conversation and merges in what arrived meanwhile; false if the user moved on.
 * `from` re-reads from that row on, so a refresh keeps the older pages already shown. */
async function loadConversation(id: number, from: number | null = null): Promise<boolean> {
  const load: Load = { peerId: id, seen: new Map(), deleted: new Set(), cleared: false };
  loads.add(load);
  try {
    const [snapshot, pinnedNow] = await Promise.all([
      from === null ? chat.listMessages(id, FIRST_PAGE) : listSince(id, from),
      chat.listPinned(id),
    ]);
    const st = useChat.getState();
    if (st.activePeerId !== id) return false;
    const hasOlder = !load.cleared && (from === null ? snapshot.length === FIRST_PAGE : st.hasOlder);
    const floor = hasOlder ? (from ?? snapshot[0]?.id ?? 0) : 0;
    const merged = new Map<string, ChatMessage>();
    if (!load.cleared) {
      // Older pages that landed while this was in flight.
      if (from !== null) for (const m of st.messages) if (m.id < from && !load.deleted.has(m.msgId)) merged.set(m.msgId, m);
      for (const m of snapshot) if (!load.deleted.has(m.msgId)) merged.set(m.msgId, m);
    }
    for (const m of load.seen.values()) {
      const s = merged.get(m.msgId);
      if (s) merged.set(m.msgId, fresher(m, s));
      else if (m.id >= floor) merged.set(m.msgId, m);
    }
    let pinned = load.cleared ? [] : pinnedNow.filter((m) => !load.deleted.has(m.msgId));
    for (const m of load.seen.values()) pinned = withPinned(pinned, m);
    useChat.setState({ messages: [...merged.values()].sort((a, b) => a.id - b.id), hasOlder, pinned });
    return true;
  } finally {
    loads.delete(load);
  }
}

/** Re-reads the open conversation without dropping the older pages on screen. */
function reloadConversation(id: number) {
  const { messages } = useChat.getState();
  return loadConversation(id, messages.length > 0 ? messages[0].id : null);
}

let olderLoad: { peerId: number; done: Promise<number> } | null = null;

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
  hasOlder: false,
  loadingOlder: false,
  pinned: [],
  transfers: {},
  nearby: [],
  typing: {},
  pendingPair: null,
  error: null,
  initialised: false,
  visible: false,
  drafts: savedDrafts(),

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
      chat.onReload(({ peerIds }) => {
        const id = get().activePeerId;
        if (id !== null && peerIds.includes(id)) void reloadConversation(id).catch(() => undefined);
      }),
      chat.onMessage((m) => {
        const { activePeerId, peers } = get();
        const viewing = m.peerId === activePeerId && isViewing();
        for (const l of loads) if (l.peerId === m.peerId) l.seen.set(m.msgId, m);
        if (m.peerId === activePeerId) {
          set(applied(get(), m));
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
          set(
            d.msgIds.length === 0
              ? { messages: [], hasOlder: false, pinned: [] }
              : {
                  messages: get().messages.filter((m) => !d.msgIds.includes(m.msgId)),
                  pinned: get().pinned.filter((m) => !d.msgIds.includes(m.msgId)),
                },
          );
        }
        reloadPeersSoon();
      }),
      chat.onTransfer((t) => {
        const transfers = { ...get().transfers };
        if (t.state === "active" || t.state === "paused") transfers[t.transferId] = t;
        else delete transfers[t.transferId];
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
    // Transfers already running (the webview was reloaded mid-transfer).
    chat
      .listTransfers()
      .then((list) => set({ transfers: { ...Object.fromEntries(list.map((t) => [t.transferId, t])), ...get().transfers } }))
      .catch(() => undefined);
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
    set({ activePeerId: id, messages: [], hasOlder: false, loadingOlder: false, pinned: [] });
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

  loadOlder: (limit = OLDER_PAGE) => {
    const { activePeerId: id, messages, hasOlder } = get();
    if (id === null || !hasOlder || messages.length === 0) return Promise.resolve(0);
    if (olderLoad?.peerId === id) return olderLoad.done;
    const load: Load = { peerId: id, seen: new Map(), deleted: new Set(), cleared: false };
    loads.add(load);
    set({ loadingOlder: true });
    const done = (async () => {
      try {
        const page = await chat.listMessages(id, limit, messages[0].id);
        if (get().activePeerId !== id) return 0;
        const fresh = load.cleared ? [] : page.filter((m) => !load.deleted.has(m.msgId));
        const before = get().messages.length;
        const next = prependMessages(get().messages, fresh);
        set({ messages: next, hasOlder: !load.cleared && page.length === limit });
        return next.length - before;
      } catch (e) {
        if (get().activePeerId === id) set({ error: errorMessage(e) });
        return 0;
      } finally {
        loads.delete(load);
        if (olderLoad?.peerId === id) olderLoad = null;
        if (get().activePeerId === id) set({ loadingOlder: false });
      }
    })();
    olderLoad = { peerId: id, done };
    return done;
  },

  jumpTo: async (m) => {
    const id = get().activePeerId;
    if (id === null || m.peerId !== id) return false;
    for (let i = 0; ; i++) {
      if (get().messages.some((x) => x.msgId === m.msgId)) return true;
      if (get().activePeerId !== id || !get().hasOlder || i === JUMP_PAGES) break;
      await get().loadOlder(MAX_PAGE);
    }
    if (get().activePeerId === id) {
      set({ error: get().hasOlder ? "That message is too far back to show here." : "That message is no longer in this conversation." });
    }
    return false;
  },

  applyMessage: (m) => {
    if (m.peerId === get().activePeerId) set(applied(get(), m));
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
      const drafts = { ...get().drafts };
      delete drafts[id];
      saveDraftText(id, "");
      set({
        drafts,
        peers: get().peers.filter((p) => p.id !== id),
        activePeerId: get().activePeerId === id ? null : get().activePeerId,
        messages: get().activePeerId === id ? [] : get().messages,
        hasOlder: get().activePeerId === id ? false : get().hasOlder,
        pinned: get().activePeerId === id ? [] : get().pinned,
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
      if (m) get().applyMessage(m);
    } catch (e) {
      set({ error: errorMessage(e) });
    }
  },

  edit: async (msgId, body) => {
    try {
      const m = await chat.edit(msgId, body);
      if (m) get().applyMessage(m);
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
      set({ messages: get().messages.filter((m) => m.msgId !== msgId), pinned: get().pinned.filter((m) => m.msgId !== msgId) });
    } catch (e) {
      set({ error: errorMessage(e) });
    }
  },

  clearChat: async () => {
    const id = get().activePeerId;
    if (id === null) return;
    try {
      await chat.clearChat(id);
      set({ messages: [], hasOlder: false, pinned: [] });
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

  setDraft: (peerId, draft) => {
    // The conversation of a removed peer saves its draft as it unmounts.
    if (!get().peers.some((p) => p.id === peerId)) return;
    const drafts = { ...get().drafts };
    if (draft.text || draft.files.length || draft.replyTo || draft.codeMode) drafts[peerId] = draft;
    else delete drafts[peerId];
    set({ drafts });
    saveDraftText(peerId, draft.text);
  },

  resync: async () => {
    void get().loadPeers();
    const id = get().activePeerId;
    if (id === null) return;
    try {
      if (!(await reloadConversation(id))) return;
    } catch {
      return;
    }
    if (isViewing()) await markActiveRead();
  },
}));
