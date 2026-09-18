import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  Account,
  ChatMessage,
  ChatStatus,
  ComposeDraft,
  DeletedEvent,
  Nearby,
  StorageStats,
  Contact,
  DraftContent,
  FetchResult,
  FlagChange,
  Identity,
  Label,
  ListQuery,
  MailFilter,
  MessageDetail,
  MessageSummary,
  NewFilter,
  OutgoingMessage,
  SnoozedMessage,
  Peer,
  ReplyMode,
  SettingsPatch,
  SettingsView,
  SyncEvent,
  TransferProgress,
} from "./types";

// Typed wrappers so components never touch raw command names.

export const settings = {
  get: () => invoke<SettingsView>("settings_get"),
  update: (patch: SettingsPatch) => invoke<SettingsView>("settings_update", { patch }),
};

export const mail = {
  listAccounts: () => invoke<Account[]>("mail_list_accounts"),
  addAccount: (provider: string) => invoke<Account>("mail_add_account", { provider }),
  removeAccount: (accountId: number) => invoke<void>("mail_remove_account", { accountId }),
  sync: (accountId: number) => invoke<void>("mail_sync", { accountId }),
  listMessages: (accountId: number, query: ListQuery, limit = 100, offset = 0, conversations = false) =>
    invoke<MessageSummary[]>("mail_list_messages", { accountId, query, limit, offset, conversations }),
  listThread: (accountId: number, threadId: string) =>
    invoke<MessageSummary[]>("mail_list_thread", { accountId, threadId }),
  bulkModify: (messageIds: number[], add: string[], remove: string[]) =>
    invoke<void>("mail_bulk_modify", { messageIds, add, remove }),
  markView: (accountId: number, query: ListQuery, read: boolean) =>
    invoke<number>("mail_mark_view", { accountId, query, read }),
  viewCount: (accountId: number, query: ListQuery) => invoke<number>("mail_view_count", { accountId, query }),
  modifyView: (accountId: number, query: ListQuery, add: string[], remove: string[]) =>
    invoke<number>("mail_modify_view", { accountId, query, add, remove }),
  threadsModify: (accountId: number, threadIds: string[], add: string[], remove: string[]) =>
    invoke<void>("mail_threads_modify", { accountId, threadIds, add, remove }),
  threadModify: (accountId: number, threadId: string, add: string[], remove: string[]) =>
    invoke<void>("mail_thread_modify", { accountId, threadId, add, remove }),
  createLabel: (accountId: number, name: string) => invoke<Label>("mail_create_label", { accountId, name }),
  createFilter: (accountId: number, filter: NewFilter) =>
    invoke<MailFilter>("mail_create_filter", { accountId, filter }),
  deleteFilter: (accountId: number, filterId: string) =>
    invoke<void>("mail_delete_filter", { accountId, filterId }),
  updateFilter: (accountId: number, filterId: string, filter: NewFilter) =>
    invoke<MailFilter>("mail_update_filter", { accountId, filterId, filter }),
  unsubscribe: (messageId: number) => invoke<{ method: string }>("mail_unsubscribe", { messageId }),
  reauth: (accountId: number) => invoke<Account>("mail_reauth", { accountId }),
  searchServer: (accountId: number, query: string) =>
    invoke<MessageSummary[]>("mail_search_server", { accountId, query }),
  snooze: (messageId: number, until: number) => invoke<void>("mail_snooze", { messageId, until }),
  unsnooze: (messageId: number) => invoke<void>("mail_unsnooze", { messageId }),
  listSnoozed: (accountId: number) => invoke<SnoozedMessage[]>("mail_list_snoozed", { accountId }),
  onUnsnoozed: (cb: (m: MessageSummary[]) => void) =>
    listen<MessageSummary[]>("mail://unsnoozed", (e) => cb(e.payload)),
  listLabels: (accountId: number) => invoke<Label[]>("mail_list_labels", { accountId }),
  modifyLabels: (messageId: number, add: string[], remove: string[]) =>
    invoke<MessageDetail>("mail_modify_labels", { messageId, add, remove }),
  fetchMore: (accountId: number, query: ListQuery, reset: boolean) =>
    invoke<FetchResult>("mail_fetch_more", { accountId, query, reset }),
  listFilters: (accountId: number) => invoke<MailFilter[]>("mail_list_filters", { accountId }),
  suggestContacts: (accountId: number, query: string) =>
    invoke<Contact[]>("mail_suggest_contacts", { accountId, query }),
  search: (accountId: number, query: string) =>
    invoke<MessageSummary[]>("mail_search", { accountId, query }),
  unreadCount: (accountId: number) => invoke<number>("mail_unread_count", { accountId }),
  getMessage: (messageId: number) => invoke<MessageDetail>("mail_get_message", { messageId }),
  setFlags: (messageId: number, flags: FlagChange) =>
    invoke<void>("mail_set_flags", { messageId, flags }),
  trash: (messageId: number) => invoke<void>("mail_trash", { messageId }),
  archive: (messageId: number) => invoke<void>("mail_archive", { messageId }),
  composeDraft: (messageId: number, mode: ReplyMode) =>
    invoke<ComposeDraft>("mail_compose_draft", { messageId, mode }),
  send: (message: OutgoingMessage) => invoke<void>("mail_send", { message }),
  saveDraft: (message: OutgoingMessage) => invoke<string>("mail_save_draft", { message }),
  discardDraft: (accountId: number, draftId: string) =>
    invoke<void>("mail_discard_draft", { accountId, draftId }),
  openDraft: (messageId: number) => invoke<DraftContent>("mail_open_draft", { messageId }),
  saveAttachment: (attachmentId: number, open: boolean) =>
    invoke<string>("mail_save_attachment", { attachmentId, open }),
  onSync: (cb: (ev: SyncEvent) => void): Promise<UnlistenFn> =>
    listen<SyncEvent>("mail://sync", (e) => cb(e.payload)),
};

export const chat = {
  identity: () => invoke<Identity>("chat_identity"),
  setDisplayName: (name: string) => invoke<Identity>("chat_set_display_name", { name }),
  listPeers: () => invoke<Peer[]>("chat_list_peers"),
  addPeer: (displayName: string, host: string, port?: number) =>
    invoke<Peer>("chat_add_peer", { displayName, host, port }),
  updatePeer: (peerId: number, displayName: string, host: string, port: number) =>
    invoke<Peer>("chat_update_peer", { peerId, displayName, host, port }),
  removePeer: (peerId: number) => invoke<void>("chat_remove_peer", { peerId }),
  connectPeer: (peerId: number) => invoke<boolean>("chat_connect_peer", { peerId }),
  listMessages: (peerId: number, limit = 100, beforeId?: number) =>
    invoke<ChatMessage[]>("chat_list_messages", { peerId, limit, beforeId }),
  markRead: (peerId: number) => invoke<void>("chat_mark_read", { peerId }),
  sendText: (peerId: number, body: string, replyTo?: string | null) =>
    invoke<ChatMessage>("chat_send_text", { peerId, body, replyTo: replyTo ?? null }),
  typing: (peerId: number) => invoke<void>("chat_typing", { peerId }),
  search: (peerId: number, query: string) => invoke<ChatMessage[]>("chat_search", { peerId, query }),
  nearby: () => invoke<Nearby[]>("chat_nearby"),
  addNearby: (peerId: string) => invoke<Peer>("chat_add_nearby", { peerId }),
  pairingQr: () => invoke<[string, string]>("chat_pairing_qr"),
  onNearby: (cb: (n: Nearby[]) => void) => listen<Nearby[]>("chat://nearby", (e) => cb(e.payload)),
  onTyping: (cb: (peerId: number) => void) => listen<number>("chat://typing", (e) => cb(e.payload)),
  sendFile: (peerId: number, path: string) =>
    invoke<ChatMessage>("chat_send_file", { peerId, path }),
  openFile: (path: string, reveal: boolean) => invoke<void>("chat_open_file", { path, reveal }),
  stashBlob: (name: string, bytes: Uint8Array) =>
    invoke<string>("chat_stash_blob", bytes, { headers: { "x-file-name": encodeURIComponent(name) } }),
  filePreview: (path: string) => invoke<string | null>("chat_file_preview", { path }),
  deleteMessage: (msgId: string, forEveryone: boolean) =>
    invoke<void>("chat_delete_message", { msgId, forEveryone }),
  clearChat: (peerId: number, forEveryone: boolean) => invoke<void>("chat_clear_chat", { peerId, forEveryone }),
  storageStats: () => invoke<StorageStats>("chat_storage_stats"),
  clearStorage: (which: "received" | "outgoing") => invoke<number>("chat_clear_storage", { which }),
  onDeleted: (cb: (d: DeletedEvent) => void) =>
    listen<DeletedEvent>("chat://deleted", (e) => cb(e.payload)),
  onMessage: (cb: (m: ChatMessage) => void) =>
    listen<ChatMessage>("chat://message", (e) => cb(e.payload)),
  onPeer: (cb: (p: Peer) => void) => listen<Peer>("chat://peer", (e) => cb(e.payload)),
  onTransfer: (cb: (t: TransferProgress) => void) =>
    listen<TransferProgress>("chat://transfer", (e) => cb(e.payload)),
  onStatus: (cb: (s: ChatStatus) => void) =>
    listen<ChatStatus>("chat://status", (e) => cb(e.payload)),
};

export const app = {
  quit: () => invoke<void>("app_quit"),
  setBadge: (count: number) => invoke<void>("app_set_badge", { count }),
};

export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}
