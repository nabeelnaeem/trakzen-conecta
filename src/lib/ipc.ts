import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  Account,
  ChatMessage,
  ChatStatus,
  ComposeDraft,
  FetchResult,
  FlagChange,
  Identity,
  Label,
  ListQuery,
  MailFilter,
  MessageDetail,
  MessageSummary,
  OutgoingMessage,
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
  listMessages: (accountId: number, query: ListQuery, limit = 100, offset = 0) =>
    invoke<MessageSummary[]>("mail_list_messages", { accountId, query, limit, offset }),
  listLabels: (accountId: number) => invoke<Label[]>("mail_list_labels", { accountId }),
  modifyLabels: (messageId: number, add: string[], remove: string[]) =>
    invoke<MessageDetail>("mail_modify_labels", { messageId, add, remove }),
  fetchMore: (accountId: number, query: ListQuery, reset: boolean) =>
    invoke<FetchResult>("mail_fetch_more", { accountId, query, reset }),
  listFilters: (accountId: number) => invoke<MailFilter[]>("mail_list_filters", { accountId }),
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
  sendText: (peerId: number, body: string) =>
    invoke<ChatMessage>("chat_send_text", { peerId, body }),
  sendFile: (peerId: number, path: string) =>
    invoke<ChatMessage>("chat_send_file", { peerId, path }),
  openFile: (path: string, reveal: boolean) => invoke<void>("chat_open_file", { path, reveal }),
  onMessage: (cb: (m: ChatMessage) => void) =>
    listen<ChatMessage>("chat://message", (e) => cb(e.payload)),
  onPeer: (cb: (p: Peer) => void) => listen<Peer>("chat://peer", (e) => cb(e.payload)),
  onTransfer: (cb: (t: TransferProgress) => void) =>
    listen<TransferProgress>("chat://transfer", (e) => cb(e.payload)),
  onStatus: (cb: (s: ChatStatus) => void) =>
    listen<ChatStatus>("chat://status", (e) => cb(e.payload)),
};

export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}
