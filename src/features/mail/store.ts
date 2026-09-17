import { create } from "zustand";
import { errorMessage, mail } from "../../lib/ipc";
import type {
  Account,
  ComposeDraft,
  Folder,
  MessageDetail,
  MessageSummary,
  ReplyMode,
  SyncEvent,
} from "../../lib/types";

export interface ComposerState {
  accountId: number;
  to: string;
  cc: string;
  bcc: string;
  subject: string;
  body: string;
  draft: ComposeDraft | null;
  files: string[];
}

interface SyncState {
  done: number;
  total: number;
}

interface MailState {
  accounts: Account[];
  activeAccountId: number | null;
  folder: Folder;
  messages: MessageSummary[];
  selectedId: number | null;
  detail: MessageDetail | null;
  loadingDetail: boolean;
  syncing: Record<number, SyncState | null>;
  unread: number;
  search: string;
  error: string | null;
  composer: ComposerState | null;
  busy: boolean;
  initialised: boolean;

  init: () => Promise<void>;
  loadAccounts: () => Promise<void>;
  setAccount: (id: number) => void;
  setFolder: (f: Folder) => void;
  setSearch: (q: string) => void;
  refresh: () => Promise<void>;
  select: (id: number | null) => Promise<void>;
  sync: () => Promise<void>;
  addAccount: () => Promise<void>;
  removeAccount: (id: number) => Promise<void>;
  toggleStar: (m: MessageSummary) => Promise<void>;
  markUnread: (id: number) => Promise<void>;
  trash: (id: number) => Promise<void>;
  archive: (id: number) => Promise<void>;
  openCompose: (mode?: ReplyMode, messageId?: number) => Promise<void>;
  updateComposer: (patch: Partial<ComposerState>) => void;
  closeCompose: () => void;
  send: () => Promise<void>;
  clearError: () => void;
}

const splitList = (s: string) =>
  s
    .split(/[,;]/)
    .map((x) => x.trim())
    .filter(Boolean);

export const useMail = create<MailState>((set, get) => ({
  accounts: [],
  activeAccountId: null,
  folder: "inbox",
  messages: [],
  selectedId: null,
  detail: null,
  loadingDetail: false,
  syncing: {},
  unread: 0,
  search: "",
  error: null,
  composer: null,
  busy: false,
  initialised: false,

  init: async () => {
    if (get().initialised) return;
    set({ initialised: true });
    await get().loadAccounts();
    await mail.onSync((ev: SyncEvent) => {
      const { syncing, activeAccountId } = get();
      switch (ev.type) {
        case "started":
          set({ syncing: { ...syncing, [ev.accountId]: { done: 0, total: 0 } } });
          break;
        case "progress":
          set({ syncing: { ...syncing, [ev.accountId]: { done: ev.done, total: ev.total } } });
          if (ev.accountId === activeAccountId) void get().refresh();
          break;
        case "finished": {
          const next = { ...syncing };
          delete next[ev.accountId];
          set({ syncing: next });
          if (ev.accountId === activeAccountId) void get().refresh();
          break;
        }
        case "failed": {
          const next = { ...syncing };
          delete next[ev.accountId];
          set({ syncing: next, error: `Sync failed: ${ev.error}` });
          break;
        }
      }
    });
  },

  loadAccounts: async () => {
    const accounts = await mail.listAccounts();
    const active = get().activeAccountId;
    const activeAccountId =
      active !== null && accounts.some((a) => a.id === active) ? active : (accounts[0]?.id ?? null);
    set({ accounts, activeAccountId });
    await get().refresh();
  },

  setAccount: (id) => {
    set({ activeAccountId: id, selectedId: null, detail: null, search: "" });
    void get().refresh();
  },

  setFolder: (folder) => {
    set({ folder, selectedId: null, detail: null, search: "" });
    void get().refresh();
  },

  setSearch: (search) => {
    set({ search });
    void get().refresh();
  },

  refresh: async () => {
    const { activeAccountId, folder, search } = get();
    if (activeAccountId === null) {
      set({ messages: [], unread: 0 });
      return;
    }
    try {
      const [messages, unread] = await Promise.all([
        search.trim()
          ? mail.search(activeAccountId, search)
          : mail.listMessages(activeAccountId, folder, 200),
        mail.unreadCount(activeAccountId),
      ]);
      set({ messages, unread });
    } catch (e) {
      set({ error: errorMessage(e) });
    }
  },

  select: async (id) => {
    if (id === null) {
      set({ selectedId: null, detail: null });
      return;
    }
    set({ selectedId: id, loadingDetail: true });
    try {
      const detail = await mail.getMessage(id);
      // The user may have moved on while the body was loading.
      if (get().selectedId !== id) return;
      const wasUnread = get().messages.some((m) => m.id === id && !m.isRead);
      set({
        detail,
        messages: get().messages.map((m) => (m.id === id ? { ...m, isRead: true } : m)),
        unread: wasUnread ? Math.max(0, get().unread - 1) : get().unread,
      });
    } catch (e) {
      set({ error: errorMessage(e) });
    } finally {
      if (get().selectedId === id) set({ loadingDetail: false });
    }
  },

  sync: async () => {
    const id = get().activeAccountId;
    if (id === null) return;
    try {
      await mail.sync(id);
    } catch (e) {
      set({ error: errorMessage(e) });
    }
  },

  addAccount: async () => {
    set({ busy: true, error: null });
    try {
      const account = await mail.addAccount("gmail");
      set({ activeAccountId: account.id });
      await get().loadAccounts();
    } catch (e) {
      set({ error: errorMessage(e) });
    } finally {
      set({ busy: false });
    }
  },

  removeAccount: async (id) => {
    try {
      await mail.removeAccount(id);
      set({ activeAccountId: null, selectedId: null, detail: null });
      await get().loadAccounts();
    } catch (e) {
      set({ error: errorMessage(e) });
    }
  },

  toggleStar: async (m) => {
    const starred = !m.isStarred;
    set({
      messages: get().messages.map((x) => (x.id === m.id ? { ...x, isStarred: starred } : x)),
      detail: get().detail?.id === m.id ? { ...get().detail!, isStarred: starred } : get().detail,
    });
    try {
      await mail.setFlags(m.id, { starred });
    } catch (e) {
      set({ error: errorMessage(e) });
      await get().refresh();
    }
  },

  markUnread: async (id) => {
    try {
      await mail.setFlags(id, { read: false });
      set({ selectedId: null, detail: null });
      await get().refresh();
    } catch (e) {
      set({ error: errorMessage(e) });
    }
  },

  trash: async (id) => {
    try {
      await mail.trash(id);
      set({ selectedId: null, detail: null, messages: get().messages.filter((m) => m.id !== id) });
    } catch (e) {
      set({ error: errorMessage(e) });
    }
  },

  archive: async (id) => {
    try {
      await mail.archive(id);
      if (get().folder === "inbox") {
        set({ selectedId: null, detail: null, messages: get().messages.filter((m) => m.id !== id) });
      }
    } catch (e) {
      set({ error: errorMessage(e) });
    }
  },

  openCompose: async (mode, messageId) => {
    const accountId = get().activeAccountId;
    if (accountId === null) return;
    if (!mode || messageId === undefined) {
      set({ composer: { accountId, to: "", cc: "", bcc: "", subject: "", body: "", draft: null, files: [] } });
      return;
    }
    try {
      const draft = await mail.composeDraft(messageId, mode);
      set({
        composer: {
          accountId: draft.accountId,
          to: draft.to.join(", "),
          cc: draft.cc.join(", "),
          bcc: "",
          subject: draft.subject,
          body: "",
          draft,
          files: [],
        },
      });
    } catch (e) {
      set({ error: errorMessage(e) });
    }
  },

  updateComposer: (patch) => {
    const c = get().composer;
    if (c) set({ composer: { ...c, ...patch } });
  },

  closeCompose: () => set({ composer: null }),

  send: async () => {
    const c = get().composer;
    if (!c) return;
    set({ busy: true, error: null });
    try {
      await mail.send({
        accountId: c.accountId,
        to: splitList(c.to),
        cc: splitList(c.cc),
        bcc: splitList(c.bcc),
        subject: c.subject,
        bodyText: c.body,
        quotedHtml: c.draft?.quotedHtml ?? null,
        inReplyTo: c.draft?.inReplyTo ?? null,
        references: c.draft?.references ?? null,
        threadId: c.draft?.threadId ?? null,
        attachments: [
          ...c.files.map((path) => ({ kind: "path" as const, path })),
          ...(c.draft?.attachments ?? []),
        ],
      });
      set({ composer: null });
    } catch (e) {
      set({ error: errorMessage(e) });
    } finally {
      set({ busy: false });
    }
  },

  clearError: () => set({ error: null }),
}));
