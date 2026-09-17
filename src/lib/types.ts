// Mirrors the serde shapes in src-tauri/src/{mail,chat}/types.rs.

// ---- mail -----------------------------------------------------------------

export type ProviderKind = "gmail";

export interface Account {
  id: number;
  provider: ProviderKind;
  email: string;
  displayName: string | null;
  syncCursor: string | null;
}

export type Folder =
  | "inbox"
  | "starred"
  | "sent"
  | "drafts"
  | "archive"
  | "trash"
  | "spam"
  | "snoozed"
  | "all";

export type Category = "primary" | "social" | "promotions" | "updates" | "forums";

export interface ListQuery {
  folder: Folder;
  category?: Category | null;
  label?: string | null;
}

export interface Label {
  id: number;
  remoteId: string;
  name: string;
  kind: "system" | "user";
  bgColor: string | null;
  fgColor: string | null;
  unread: number;
  total: number;
}

export interface MailFilter {
  id: string;
  criteria: [string, string][];
  addLabels: string[];
  removeLabels: string[];
  forward: string | null;
}

export interface FetchResult {
  added: number;
  hasMore: boolean;
}

export interface MessageSummary {
  id: number;
  accountId: number;
  remoteId: string;
  threadId: string | null;
  subject: string;
  fromName: string;
  fromAddr: string;
  toAddrs: string;
  ccAddrs: string;
  snippet: string;
  date: number;
  labels: string[];
  isRead: boolean;
  isStarred: boolean;
  hasAttachments: boolean;
  threadCount: number;
  threadUnread: number;
}

export interface SnoozedMessage extends MessageSummary {
  until: number;
}

export interface NewFilter {
  from: string;
  to: string;
  subject: string;
  hasWords: string;
  notWords: string;
  hasAttachment: boolean;
  skipInbox: boolean;
  markRead: boolean;
  star: boolean;
  addLabel: string | null;
  delete: boolean;
  neverSpam: boolean;
  markImportant: boolean;
  applyToExisting: boolean;
}

export const emptyFilter = (): NewFilter => ({
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
});

export interface NewMailInfo {
  id: number;
  fromName: string;
  subject: string;
}

export interface AttachmentInfo {
  id: number;
  remoteId: string;
  filename: string;
  mimeType: string;
  size: number;
}

export interface MessageDetail extends MessageSummary {
  bodyHtml: string | null;
  bodyText: string | null;
  messageIdHdr: string | null;
  referencesHdr: string | null;
  attachments: AttachmentInfo[];
}

export interface FlagChange {
  read?: boolean | null;
  starred?: boolean | null;
}

export type SyncEvent =
  | { type: "started"; accountId: number; full: boolean }
  | { type: "progress"; accountId: number; done: number; total: number }
  | { type: "finished"; accountId: number }
  | { type: "failed"; accountId: number; error: string }
  | { type: "newMail"; accountId: number; messages: NewMailInfo[] };

export type OutgoingAttachment =
  | { kind: "path"; path: string }
  | { kind: "stored"; messageId: number; attachmentId: number };

export interface OutgoingMessage {
  accountId: number;
  to: string[];
  cc: string[];
  bcc: string[];
  subject: string;
  bodyText: string;
  quotedHtml?: string | null;
  inReplyTo?: string | null;
  references?: string | null;
  threadId?: string | null;
  attachments: OutgoingAttachment[];
  draftId?: string | null;
}

export interface DraftContent {
  draftId: string;
  to: string[];
  cc: string[];
  bcc: string[];
  subject: string;
  bodyText: string;
  threadId: string | null;
  inReplyTo: string | null;
  references: string | null;
}

export type ReplyMode = "reply" | "reply-all" | "forward";

export interface ComposeDraft {
  accountId: number;
  to: string[];
  cc: string[];
  subject: string;
  quotedHtml: string | null;
  quotedText: string;
  inReplyTo: string | null;
  references: string | null;
  threadId: string | null;
  attachments: OutgoingAttachment[];
  attachmentNames: string[];
}

// ---- chat -----------------------------------------------------------------

export interface Identity {
  peerId: string;
  displayName: string;
  port: number;
  addresses: string[];
  listening: boolean;
}

export interface Peer {
  id: number;
  peerId: string | null;
  displayName: string;
  host: string;
  port: number;
  lastSeen: number | null;
  online: boolean;
  unread: number;
  lastMessage: string | null;
  lastMessageAt: number | null;
}

export interface ChatMessage {
  id: number;
  msgId: string;
  peerId: number;
  direction: "in" | "out";
  kind: "text" | "file";
  body: string;
  fileName: string | null;
  filePath: string | null;
  fileSize: number | null;
  status: string;
  createdAt: number;
}

export interface TransferProgress {
  transferId: string;
  msgId: string;
  peerId: number;
  direction: "in" | "out";
  fileName: string;
  bytesDone: number;
  bytesTotal: number;
  state: "active" | "done" | "failed";
}

export interface ChatStatus {
  listening: boolean;
  error?: string;
}

// ---- settings -------------------------------------------------------------

export interface SettingsView {
  googleClientId: string;
  googleClientSecretSet: boolean;
  chatDisplayName: string;
  chatPort: number;
  chatDownloadDir: string;
  mailShowImages: boolean;
  mailSignature: string;
  mailPollSeconds: number;
  closeToTray: boolean;
  notifications: boolean;
  notificationSound: boolean;
  conversationView: boolean;
  undoSendSeconds: number;
}

export interface Contact {
  email: string;
  name: string;
}

export interface SettingsPatch {
  googleClientId?: string;
  googleClientSecret?: string;
  chatDisplayName?: string;
  chatPort?: number;
  chatDownloadDir?: string;
  mailShowImages?: boolean;
  mailSignature?: string;
  mailPollSeconds?: number;
  closeToTray?: boolean;
  notifications?: boolean;
  notificationSound?: boolean;
  conversationView?: boolean;
  undoSendSeconds?: number;
}
