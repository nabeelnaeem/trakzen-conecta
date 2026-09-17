import { create } from "zustand";
import { errorMessage, mail, settings } from "../../lib/ipc";
import { notify, notifyPrefs } from "../../lib/notify";
import type {
  Account,
  Category,
  ComposeDraft,
  Folder,
  Label,
  ListQuery,
  MessageDetail,
  MessageSummary,
  NewFilter,
  OutgoingMessage,
  ReplyMode,
  SnoozedMessage,
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
  draftId: string | null;
  dirty: boolean;
  saving: boolean;
  savedAt: number | null;
}

interface SyncState {
  done: number;
  total: number;
}

export interface PendingSend {
  message: OutgoingMessage;
  composer: ComposerState;
  sendAt: number;
  timer: number;
}

const threadKey = (m: MessageSummary) => m.threadId ?? m.remoteId;

interface MailState {
  accounts: Account[];
  activeAccountId: number | null;
  folder: Folder;
  category: Category;
  label: string | null;
  labels: Label[];
  messages: MessageSummary[];
  snoozed: SnoozedMessage[];
  hasMore: boolean;
  fetching: boolean;
  conversations: boolean;
  // Reading pane: the open thread (one message in flat mode).
  openId: number | null;
  thread: MessageSummary[];
  expanded: number[];
  details: Record<number, MessageDetail>;
  loadingDetail: boolean;
  // Multi-select (ids of list rows).
  selected: number[];
  syncing: Record<number, SyncState | null>;
  unread: number;
  search: string;
  serverSearch: boolean;
  showImages: boolean;
  undoSeconds: number;
  pendingSend: PendingSend | null;
  filterEditor: NewFilter | null;
  error: string | null;
  notice: string | null;
  /// Label of the long-running action in flight, for the progress toast.
  working: string | null;
  /// "Select all N conversations in <view>" state.
  viewCount: number | null;
  allInView: boolean;
  composer: ComposerState | null;
  busy: boolean;
  initialised: boolean;

  init: () => Promise<void>;
  loadAccounts: () => Promise<void>;
  loadLabels: () => Promise<void>;
  setAccount: (id: number) => void;
  setFolder: (f: Folder) => void;
  setCategory: (c: Category) => void;
  setLabel: (remoteId: string) => void;
  setSearch: (q: string) => void;
  searchOnServer: () => Promise<void>;
  applySettings: (s: { showImages: boolean; conversations: boolean; undoSeconds: number }) => void;
  query: () => ListQuery;
  refresh: () => Promise<void>;
  fetchFromServer: (reset: boolean) => Promise<void>;
  loadMore: () => Promise<void>;
  open: (id: number | null) => Promise<void>;
  openNext: (delta: number) => Promise<void>;
  expand: (id: number, on?: boolean) => Promise<void>;
  sync: () => Promise<void>;
  addAccount: () => Promise<void>;
  removeAccount: (id: number) => Promise<void>;
  toggleStar: (m: MessageSummary) => Promise<void>;
  // Thread-level actions on the open thread (or a specific row).
  act: (action: "archive" | "trash" | "spam" | "notSpam" | "unread" | "read" | "inbox", ids?: number[]) => Promise<void>;
  modifyLabels: (ids: number[], add: string[], remove: string[]) => Promise<boolean>;
  snooze: (ids: number[], until: number | null) => Promise<void>;
  toggleSelect: (id: number, range?: boolean) => void;
  selectAll: (on: boolean) => void;
  selectWhere: (which: "read" | "unread") => void;
  selectEntireView: () => Promise<void>;
  markView: (read: boolean) => Promise<void>;
  run: <T>(label: string, fn: () => Promise<T>) => Promise<T | undefined>;
  openCompose: (mode?: ReplyMode, messageId?: number) => Promise<void>;
  updateComposer: (patch: Partial<ComposerState>) => void;
  closeCompose: () => void;
  discardDraft: () => Promise<void>;
  saveDraftNow: () => Promise<void>;
  send: () => Promise<void>;
  undoSend: () => void;
  openFilterEditor: (prefill?: Partial<NewFilter>) => void;
  closeFilterEditor: () => void;
  createFilter: (f: NewFilter) => Promise<boolean>;
  clearError: () => void;
}

const splitList = (s: string) =>
  s
    .split(/[,;]/)
    .map((x) => x.trim())
    .filter(Boolean);

let lastSelectedId: number | null = null;
let fetchSeq = 0;
let draftTimer: number | null = null;

const emptyComposer = (accountId: number): ComposerState => ({
  accountId,
  to: "",
  cc: "",
  bcc: "",
  subject: "",
  body: "",
  draft: null,
  files: [],
  draftId: null,
  dirty: false,
  saving: false,
  savedAt: null,
});

function toOutgoing(c: ComposerState): OutgoingMessage {
  return {
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
    draftId: c.draftId,
  };
}

export const useMail = create<MailState>((set, get) => ({
  accounts: [],
  activeAccountId: null,
  folder: "inbox",
  category: "primary",
  label: null,
  labels: [],
  messages: [],
  snoozed: [],
  hasMore: true,
  fetching: false,
  conversations: true,
  openId: null,
  thread: [],
  expanded: [],
  details: {},
  loadingDetail: false,
  selected: [],
  syncing: {},
  unread: 0,
  search: "",
  serverSearch: false,
  showImages: true,
  undoSeconds: 10,
  pendingSend: null,
  filterEditor: null,
  error: null,
  notice: null,
  working: null,
  viewCount: null,
  allInView: false,
  composer: null,
  busy: false,
  initialised: false,

  // Wraps a slow action: shows the progress toast, surfaces errors.
  run: async (label, fn) => {
    set({ working: label, error: null });
    try {
      return await fn();
    } catch (e) {
      set({ error: errorMessage(e) });
      return undefined;
    } finally {
      set({ working: null });
    }
  },

  init: async () => {
    if (get().initialised) return;
    set({ initialised: true });
    try {
      const s = await settings.get();
      get().applySettings({
        showImages: s.mailShowImages,
        conversations: s.conversationView,
        undoSeconds: s.undoSendSeconds,
      });
      notifyPrefs.notifications = s.notifications;
      notifyPrefs.sound = s.notificationSound;
    } catch {
      /* defaults are fine */
    }
    await get().loadAccounts();
    // Coming back to the window is the moment people expect fresh mail.
    let lastFocusSync = 0;
    window.addEventListener("focus", () => {
      const now = Date.now();
      if (now - lastFocusSync < 15_000) return;
      lastFocusSync = now;
      void get().sync();
    });
    await mail.onUnsnoozed((due) => {
      void get().refresh();
      for (const m of due) void notify(m.fromName || m.fromAddr, m.subject || "(no subject)", "mail");
    });
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
          if (ev.accountId === activeAccountId) {
            void get().refresh();
            void get().loadLabels();
          }
          break;
        }
        case "failed": {
          const next = { ...syncing };
          delete next[ev.accountId];
          set({ syncing: next, error: `Sync failed: ${ev.error}` });
          break;
        }
        case "newMail": {
          const account = get().accounts.find((a) => a.id === ev.accountId);
          const focused = document.hasFocus();
          if (ev.messages.length === 1) {
            const m = ev.messages[0];
            void notify(m.fromName, m.subject || "(no subject)", "mail").then(() => undefined);
          } else if (ev.messages.length > 1) {
            void notify(
              `${ev.messages.length} new messages${account ? ` · ${account.email}` : ""}`,
              ev.messages
                .slice(0, 4)
                .map((m) => `${m.fromName}: ${m.subject}`)
                .join("\n"),
              "mail",
            );
          }
          void focused;
          break;
        }
      }
    });
  },

  applySettings: ({ showImages, conversations, undoSeconds }) => {
    const changed = conversations !== get().conversations;
    set({ showImages, conversations, undoSeconds });
    if (changed) {
      set({ openId: null, thread: [], expanded: [], selected: [] });
      void get().refresh();
    }
  },

  loadAccounts: async () => {
    const accounts = await mail.listAccounts();
    const active = get().activeAccountId;
    const activeAccountId =
      active !== null && accounts.some((a) => a.id === active) ? active : (accounts[0]?.id ?? null);
    set({ accounts, activeAccountId });
    await get().loadLabels();
    await get().fetchFromServer(true);
  },

  loadLabels: async () => {
    const id = get().activeAccountId;
    if (id === null) {
      set({ labels: [] });
      return;
    }
    try {
      set({ labels: await mail.listLabels(id) });
    } catch (e) {
      set({ error: errorMessage(e) });
    }
  },

  query: () => {
    const { folder, category, label } = get();
    return { folder, category: folder === "inbox" ? category : null, label };
  },

  setAccount: (id) => {
    set({ activeAccountId: id, openId: null, thread: [], expanded: [], selected: [], search: "", label: null, messages: [] });
    void get().loadLabels();
    void get().fetchFromServer(true);
  },

  setFolder: (folder) => {
    set({ folder, label: null, openId: null, thread: [], expanded: [], selected: [], allInView: false, viewCount: null, search: "", serverSearch: false, hasMore: true, messages: [] });
    void get().fetchFromServer(true);
  },

  setCategory: (category) => {
    set({ category, folder: "inbox", label: null, openId: null, thread: [], expanded: [], selected: [], allInView: false, viewCount: null, search: "", serverSearch: false, hasMore: true, messages: [] });
    void get().fetchFromServer(true);
  },

  setLabel: (remoteId) => {
    set({ label: remoteId, openId: null, thread: [], expanded: [], selected: [], allInView: false, viewCount: null, search: "", serverSearch: false, hasMore: true, messages: [] });
    void get().fetchFromServer(true);
  },

  setSearch: (search) => {
    set({ search, serverSearch: false, selected: [], allInView: false, viewCount: null });
    void get().refresh();
  },

  searchOnServer: async () => {
    const id = get().activeAccountId;
    const q = get().search.trim();
    if (id === null || !q) return;
    set({ fetching: true, serverSearch: true });
    try {
      const messages = await mail.searchServer(id, q);
      if (get().search.trim() === q) set({ messages, hasMore: false });
    } catch (e) {
      set({ error: errorMessage(e) });
    } finally {
      set({ fetching: false });
    }
  },

  refresh: async () => {
    const { activeAccountId, search, folder, conversations, serverSearch } = get();
    if (activeAccountId === null) {
      set({ messages: [], unread: 0, snoozed: [] });
      return;
    }
    if (serverSearch) return;
    try {
      if (folder === "snoozed" && !search.trim()) {
        const [snoozed, unread] = await Promise.all([mail.listSnoozed(activeAccountId), mail.unreadCount(activeAccountId)]);
        set({ snoozed, messages: snoozed, unread, hasMore: false });
        return;
      }
      const [messages, unread] = await Promise.all([
        search.trim()
          ? mail.search(activeAccountId, search)
          : mail.listMessages(activeAccountId, get().query(), 5000, 0, conversations),
        mail.unreadCount(activeAccountId),
      ]);
      set({ messages, unread });
    } catch (e) {
      set({ error: errorMessage(e) });
    }
  },

  // Asks the server for the next page of the current view and merges it
  // into the local cache; the list then re-reads from the cache.
  fetchFromServer: async (reset) => {
    const id = get().activeAccountId;
    if (id === null) return;
    if (get().folder === "snoozed") {
      await get().refresh();
      return;
    }
    // A newer request (view switch) supersedes this one; "load more" while
    // a page is already loading is simply ignored.
    if (!reset && get().fetching) return;
    const seq = ++fetchSeq;
    const query = get().query();
    set({ fetching: true });
    try {
      const r = await mail.fetchMore(id, query, reset);
      if (seq !== fetchSeq) return;
      set({ hasMore: r.hasMore });
      await get().refresh();
      if (r.added > 0) void get().loadLabels();
    } catch (e) {
      if (seq !== fetchSeq) return;
      // Offline or failed: fall back to whatever is cached.
      set({ error: errorMessage(e), hasMore: false });
      await get().refresh();
    } finally {
      if (seq === fetchSeq) set({ fetching: false });
    }
  },

  // Each page comes from the server so the list stays continuous by date.
  loadMore: async () => {
    await get().fetchFromServer(false);
  },

  open: async (id) => {
    if (id === null) {
      set({ openId: null, thread: [], expanded: [] });
      return;
    }
    const row = get().messages.find((m) => m.id === id);
    if (!row) return;
    lastSelectedId = id;
    if (row.labels.includes("DRAFT")) {
      try {
        const d = await mail.openDraft(id);
        set({
          composer: {
            ...emptyComposer(row.accountId),
            to: d.to.join(", "),
            cc: d.cc.join(", "),
            bcc: d.bcc.join(", "),
            subject: d.subject,
            body: d.bodyText,
            draftId: d.draftId,
            draft: d.threadId || d.inReplyTo
              ? {
                  accountId: row.accountId,
                  to: [],
                  cc: [],
                  subject: d.subject,
                  quotedHtml: null,
                  quotedText: "",
                  inReplyTo: d.inReplyTo,
                  references: d.references,
                  threadId: d.threadId,
                  attachments: [],
                  attachmentNames: [],
                }
              : null,
          },
        });
      } catch (e) {
        set({ error: errorMessage(e) });
      }
      return;
    }
    set({ openId: id, loadingDetail: true, thread: [row], expanded: [] });
    try {
      const accountId = row.accountId;
      const thread = get().conversations && row.threadCount > 1
        ? await mail.listThread(accountId, threadKey(row))
        : [row];
      if (get().openId !== id) return;
      // Gmail expands the newest message plus anything unread.
      const last = thread[thread.length - 1];
      const toExpand = thread.filter((m) => !m.isRead || m.id === last.id).map((m) => m.id);
      set({ thread, expanded: [] });
      for (const mid of toExpand) await get().expand(mid, true);
    } catch (e) {
      set({ error: errorMessage(e) });
    } finally {
      if (get().openId === id) set({ loadingDetail: false });
    }
  },

  openNext: async (delta) => {
    const { messages, openId } = get();
    if (messages.length === 0) return;
    const i = messages.findIndex((m) => m.id === openId);
    const next = i === -1 ? (delta > 0 ? 0 : messages.length - 1) : Math.min(messages.length - 1, Math.max(0, i + delta));
    await get().open(messages[next].id);
  },

  expand: async (mid, on) => {
    const isOpen = get().expanded.includes(mid);
    const want = on ?? !isOpen;
    if (!want) {
      set({ expanded: get().expanded.filter((x) => x !== mid) });
      return;
    }
    set({ expanded: [...get().expanded, mid] });
    if (get().details[mid]) return;
    try {
      const detail = await mail.getMessage(mid);
      const wasUnread = get().thread.some((m) => m.id === mid && !m.isRead) || get().messages.some((m) => m.id === mid && !m.isRead);
      set({
        details: { ...get().details, [mid]: detail },
        thread: get().thread.map((m) => (m.id === mid ? { ...m, isRead: true } : m)),
        messages: get().messages.map((m) =>
          m.id === mid
            ? { ...m, isRead: true, threadUnread: Math.max(0, m.threadUnread - 1) }
            : get().conversations && threadKey(m) === threadKey(detail) && wasUnread
              ? { ...m, threadUnread: Math.max(0, m.threadUnread - 1) }
              : m,
        ),
        unread: wasUnread ? Math.max(0, get().unread - 1) : get().unread,
      });
      if (wasUnread) void get().loadLabels();
    } catch (e) {
      set({ error: errorMessage(e) });
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
      set({ activeAccountId: null, openId: null, thread: [], expanded: [] });
      await get().loadAccounts();
    } catch (e) {
      set({ error: errorMessage(e) });
    }
  },

  toggleStar: async (m) => {
    const starred = !m.isStarred;
    const patchRow = (x: MessageSummary) => (x.id === m.id ? { ...x, isStarred: starred } : x);
    set({ messages: get().messages.map(patchRow), thread: get().thread.map(patchRow) });
    try {
      await mail.setFlags(m.id, { starred });
    } catch (e) {
      set({ error: errorMessage(e) });
      await get().refresh();
    }
  },

  // Resolves row ids to every message they stand for (whole threads in
  // conversation mode) and applies one batched label change.
  act: async (action, ids) => {
    const [add, remove] = {
      archive: [[], ["INBOX"]],
      trash: [["TRASH"], ["INBOX"]],
      spam: [["SPAM"], ["INBOX"]],
      notSpam: [[], ["SPAM"]],
      unread: [["UNREAD"], []],
      read: [[], ["UNREAD"]],
      inbox: [["INBOX"], ["TRASH", "SPAM"]],
    }[action] as [string[], string[]];
    const verb = { archive: "Archiving", trash: "Deleting", spam: "Reporting spam", notSpam: "Restoring", unread: "Marking unread", read: "Marking read", inbox: "Moving to inbox" }[action];

    // Whole-view selection: server-side over every message in the view.
    if (!ids && get().allInView && get().activeAccountId !== null) {
      const accountId = get().activeAccountId!;
      const n = await get().run(`${verb}…`, () => mail.modifyView(accountId, get().query(), add, remove));
      if (n === undefined) return;
      set({ selected: [], allInView: false, viewCount: null, openId: null, thread: [], expanded: [], notice: `${n} message${n === 1 ? "" : "s"} updated.` });
      window.setTimeout(() => set({ notice: null }), 3000);
      await Promise.all([get().refresh(), get().loadLabels()]);
      return;
    }

    const targets = ids ?? (get().selected.length ? get().selected : get().openId !== null ? [get().openId!] : []);
    if (targets.length === 0) return;
    const ok = await get().run(targets.length > 1 ? `${verb} ${targets.length}…` : `${verb}…`, () => get().modifyLabels(targets, add, remove));
    if (ok === undefined) return;
    const removesFromView =
      (action === "archive" && get().folder === "inbox" && !get().label) ||
      (action === "trash" && get().folder !== "trash") ||
      (action === "spam" && get().folder !== "spam") ||
      (action === "notSpam" && get().folder === "spam") ||
      (action === "inbox" && (get().folder === "trash" || get().folder === "spam"));
    if (removesFromView) {
      const gone = new Set(targets);
      set({
        messages: get().messages.filter((m) => !gone.has(m.id)),
        selected: [],
        openId: gone.has(get().openId ?? -1) ? null : get().openId,
        thread: gone.has(get().openId ?? -1) ? [] : get().thread,
      });
    } else if (action === "unread") {
      set({ openId: null, thread: [], expanded: [], selected: [] });
      await get().refresh();
    } else {
      set({ selected: [] });
      await get().refresh();
    }
    void get().loadLabels();
  },

  modifyLabels: async (ids, add, remove) => {
    const rows = get().messages.filter((m) => ids.includes(m.id));
    const rowsFromThread = get().thread.filter((m) => ids.includes(m.id) && !rows.some((r) => r.id === m.id));
    const all = [...rows, ...rowsFromThread];
    try {
      if (get().conversations && all.length > 0) {
        // Whole threads, like Gmail; one request that fans out server-side.
        const threadIds = Array.from(new Set(all.map(threadKey)));
        await mail.threadsModify(all[0].accountId, threadIds, add, remove);
      } else {
        await mail.bulkModify(ids, add, remove);
      }
      // Reflect locally without a full refresh so the pane doesn't flicker.
      const apply = (m: MessageSummary) => {
        if (!ids.includes(m.id) && !(get().conversations && all.some((r) => threadKey(r) === threadKey(m)))) return m;
        const labels = m.labels.filter((l) => !remove.includes(l)).concat(add.filter((l) => !m.labels.includes(l)));
        return { ...m, labels, isRead: !labels.includes("UNREAD"), isStarred: labels.includes("STARRED") };
      };
      const details = { ...get().details };
      for (const k of Object.keys(details)) details[Number(k)] = apply(details[Number(k)]) as MessageDetail;
      set({ messages: get().messages.map(apply), thread: get().thread.map(apply), details });
      // Unread badge and label counts change with almost every action.
      const accountId = get().activeAccountId;
      if (accountId !== null) mail.unreadCount(accountId).then((unread) => set({ unread })).catch(() => undefined);
      void get().loadLabels();
      return true;
    } catch (e) {
      set({ error: errorMessage(e) });
      await get().refresh();
      throw e;
    }
  },

  snooze: async (ids, until) => {
    await get().run(until === null ? "Unsnoozing…" : "Snoozing…", async () => {
      const rows = [...get().messages, ...get().thread].filter((m) => ids.includes(m.id));
      const targets = new Set<number>(ids);
      if (get().conversations) {
        for (const r of rows) {
          for (const t of await mail.listThread(r.accountId, threadKey(r))) targets.add(t.id);
        }
      }
      for (const id of targets) {
        if (until === null) await mail.unsnooze(id);
        else await mail.snooze(id, until);
      }
      const gone = new Set(ids);
      set({
        messages: get().messages.filter((m) => !gone.has(m.id)),
        selected: [],
        openId: gone.has(get().openId ?? -1) ? null : get().openId,
        thread: gone.has(get().openId ?? -1) ? [] : get().thread,
        notice: until === null ? "Moved back to the inbox." : `Snoozed until ${new Date(until).toLocaleString()}`,
      });
      window.setTimeout(() => set({ notice: null }), 4000);
    });
  },

  toggleSelect: (id, range) => {
    const { selected, messages } = get();
    if (range && lastSelectedId !== null) {
      const a = messages.findIndex((m) => m.id === lastSelectedId);
      const b = messages.findIndex((m) => m.id === id);
      if (a !== -1 && b !== -1) {
        const [lo, hi] = a < b ? [a, b] : [b, a];
        const ids = messages.slice(lo, hi + 1).map((m) => m.id);
        set({ selected: Array.from(new Set([...selected, ...ids])) });
        return;
      }
    }
    lastSelectedId = id;
    set({ selected: selected.includes(id) ? selected.filter((x) => x !== id) : [...selected, id] });
  },

  selectAll: (on) => {
    set({ selected: on ? get().messages.map((m) => m.id) : [], allInView: false });
    if (on && get().viewCount === null && get().activeAccountId !== null && !get().search) {
      const accountId = get().activeAccountId!;
      mail.viewCount(accountId, get().query()).then((n) => set({ viewCount: n })).catch(() => undefined);
    }
  },

  selectEntireView: async () => {
    set({ allInView: true, selected: get().messages.map((m) => m.id) });
  },

  selectWhere: (which) =>
    set({
      selected: get()
        .messages.filter((m) => (which === "unread" ? !m.isRead || m.threadUnread > 0 : m.isRead && m.threadUnread === 0))
        .map((m) => m.id),
    }),

  markView: async (read) => {
    const id = get().activeAccountId;
    if (id === null) return;
    const n = await get().run(read ? "Marking everything as read…" : "Marking everything as unread…", () => mail.markView(id, get().query(), read));
    if (n === undefined) return;
    set({ selected: [], allInView: false, notice: `${n} message${n === 1 ? "" : "s"} marked as ${read ? "read" : "unread"}.` });
    window.setTimeout(() => set({ notice: null }), 3000);
    await Promise.all([get().refresh(), get().loadLabels()]);
  },

  openCompose: async (mode, messageId) => {
    const accountId = get().activeAccountId;
    if (accountId === null) return;
    if (!mode || messageId === undefined) {
      set({ composer: emptyComposer(accountId) });
      return;
    }
    try {
      const draft = await mail.composeDraft(messageId, mode);
      set({
        composer: {
          ...emptyComposer(draft.accountId),
          to: draft.to.join(", "),
          cc: draft.cc.join(", "),
          subject: draft.subject,
          draft,
        },
      });
    } catch (e) {
      set({ error: errorMessage(e) });
    }
  },

  updateComposer: (patch) => {
    const c = get().composer;
    if (!c) return;
    set({ composer: { ...c, ...patch, dirty: true } });
    // Autosave to the server a few seconds after the last keystroke.
    if (draftTimer) window.clearTimeout(draftTimer);
    draftTimer = window.setTimeout(() => void get().saveDraftNow(), 3000);
  },

  saveDraftNow: async () => {
    const c = get().composer;
    if (!c || !c.dirty || c.saving) return;
    const hasContent = c.to.trim() || c.subject.trim() || c.body.trim();
    if (!hasContent) return;
    set({ composer: { ...c, saving: true } });
    try {
      const draftId = await mail.saveDraft(toOutgoing(c));
      const now = get().composer;
      if (now) set({ composer: { ...now, draftId, dirty: now !== c && now.dirty, saving: false, savedAt: Date.now() } });
    } catch (e) {
      const now = get().composer;
      if (now) set({ composer: { ...now, saving: false } });
      set({ error: `Draft not saved: ${errorMessage(e)}` });
    }
  },

  // Closing keeps the draft (saved to the server if anything changed).
  closeCompose: () => {
    const c = get().composer;
    if (draftTimer) window.clearTimeout(draftTimer);
    if (!c?.dirty) {
      set({ composer: null });
      return;
    }
    void get().saveDraftNow().then(() => {
      set({ composer: null, notice: "Draft saved" });
      window.setTimeout(() => set({ notice: null }), 2500);
      void get().sync();
    });
  },

  discardDraft: async () => {
    const c = get().composer;
    if (draftTimer) window.clearTimeout(draftTimer);
    set({ composer: null });
    if (c?.draftId) {
      try {
        await mail.discardDraft(c.accountId, c.draftId);
        if (get().folder === "drafts") await get().refresh();
      } catch (e) {
        set({ error: errorMessage(e) });
      }
    }
  },

  send: async () => {
    const c = get().composer;
    if (!c) return;
    if (draftTimer) window.clearTimeout(draftTimer);
    const message = toOutgoing(c);
    if (message.to.length === 0) {
      set({ error: "Add at least one recipient." });
      return;
    }
    const doSend = async () => {
      set({ pendingSend: null, busy: true, error: null });
      try {
        await mail.send(message);
        set({ notice: "Sent." });
        window.setTimeout(() => set({ notice: null }), 3000);
        if (get().folder === "drafts") void get().refresh();
      } catch (e) {
        // Give the user their draft back rather than losing it.
        set({ error: errorMessage(e), composer: c });
      } finally {
        set({ busy: false });
      }
    };
    const delay = get().undoSeconds;
    if (delay <= 0) {
      await doSend();
      return;
    }
    if (get().pendingSend) window.clearTimeout(get().pendingSend!.timer);
    const timer = window.setTimeout(() => void doSend(), delay * 1000);
    set({ composer: null, pendingSend: { message, composer: c, sendAt: Date.now() + delay * 1000, timer } });
  },

  undoSend: () => {
    const p = get().pendingSend;
    if (!p) return;
    window.clearTimeout(p.timer);
    set({ pendingSend: null, composer: p.composer });
  },

  openFilterEditor: (prefill) =>
    set({
      filterEditor: {
        from: "",
        to: "",
        subject: "",
        hasWords: "",
        notWords: "",
        hasAttachment: false,
        skipInbox: false,
        markRead: false,
        star: false,
        addLabel: null,
        delete: false,
        neverSpam: false,
        markImportant: false,
        applyToExisting: true,
        ...prefill,
      },
    }),

  closeFilterEditor: () => set({ filterEditor: null }),

  createFilter: async (f) => {
    const id = get().activeAccountId;
    if (id === null) return false;
    try {
      await mail.createFilter(id, f);
      set({ filterEditor: null, notice: "Filter created." });
      window.setTimeout(() => set({ notice: null }), 3000);
      return true;
    } catch (e) {
      set({ error: errorMessage(e) });
      return false;
    }
  },

  clearError: () => set({ error: null }),
}));
