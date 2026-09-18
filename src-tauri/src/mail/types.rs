use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    Gmail,
}

impl ProviderKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderKind::Gmail => "gmail",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: i64,
    pub provider: String,
    pub email: String,
    pub display_name: Option<String>,
    pub sync_cursor: Option<String>,
}

/// Logical folders shown in the UI. Providers map these onto whatever they
/// use natively (labels for Gmail, folders for IMAP).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Folder {
    Inbox,
    Starred,
    Sent,
    Drafts,
    Archive,
    Trash,
    Spam,
    /// Local-only view of snoozed mail.
    Snoozed,
    All,
}

/// Gmail inbox tabs. `Primary` is everything in the inbox that carries no
/// other category label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Primary,
    Social,
    Promotions,
    Updates,
    Forums,
}

impl Category {
    pub fn label_id(self) -> &'static str {
        match self {
            Category::Primary => "CATEGORY_PERSONAL",
            Category::Social => "CATEGORY_SOCIAL",
            Category::Promotions => "CATEGORY_PROMOTIONS",
            Category::Updates => "CATEGORY_UPDATES",
            Category::Forums => "CATEGORY_FORUMS",
        }
    }
}

/// What the message list is showing. `label` (a provider label id) wins
/// over `folder`; `category` only applies to the inbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListQuery {
    pub folder: Folder,
    #[serde(default)]
    pub category: Option<Category>,
    #[serde(default)]
    pub label: Option<String>,
}

impl ListQuery {
    /// The provider label that backs this view, for server-side paging.
    pub fn label_id(&self) -> Option<String> {
        if let Some(l) = &self.label {
            return Some(l.clone());
        }
        let id = match self.folder {
            Folder::Inbox => match self.category {
                Some(c) if c != Category::Primary => return Some(c.label_id().into()),
                _ => "INBOX",
            },
            Folder::Starred => "STARRED",
            Folder::Sent => "SENT",
            Folder::Drafts => "DRAFT",
            Folder::Trash => "TRASH",
            Folder::Spam => "SPAM",
            Folder::Archive | Folder::All | Folder::Snoozed => return None,
        };
        Some(id.into())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Label {
    pub id: i64,
    pub remote_id: String,
    pub name: String,
    /// `system` or `user`
    pub kind: String,
    pub bg_color: Option<String>,
    pub fg_color: Option<String>,
    pub unread: i64,
    pub total: i64,
}

#[derive(Debug, Clone, Default)]
pub struct RemoteLabel {
    pub remote_id: String,
    pub name: String,
    pub kind: String,
    pub bg_color: Option<String>,
    pub fg_color: Option<String>,
    pub visible: bool,
}

/// A server-side filter rule (Gmail "Filters and blocked addresses").
/// Read-only for now; shown in Settings so users can see what the account
/// does to incoming mail.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailFilter {
    pub id: String,
    /// Human-readable criteria, e.g. "from: alerts@x.com" — ordered.
    pub criteria: Vec<(String, String)>,
    pub add_labels: Vec<String>,
    pub remove_labels: Vec<String>,
    pub forward: Option<String>,
    /// Raw label ids, kept so a filter can be edited (names above are for display).
    #[serde(default)]
    pub add_label_ids: Vec<String>,
    #[serde(default)]
    pub remove_label_ids: Vec<String>,
}

/// A server-side draft opened for editing.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftContent {
    pub draft_id: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub body_text: String,
    pub thread_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Contact {
    pub email: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchResult {
    pub added: usize,
    pub has_more: bool,
}

/// Outcome of one server page for a view.
#[derive(Debug, Clone, Default)]
pub struct PageFetched {
    pub added: usize,
    pub next_page: Option<String>,
    /// Date of the oldest message on the page; the view is complete in the
    /// cache down to this point.
    pub oldest: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSummary {
    pub id: i64,
    pub account_id: i64,
    pub remote_id: String,
    pub thread_id: Option<String>,
    pub subject: String,
    pub from_name: String,
    pub from_addr: String,
    pub to_addrs: String,
    pub cc_addrs: String,
    pub snippet: String,
    pub date: i64,
    pub labels: Vec<String>,
    pub is_read: bool,
    pub is_starred: bool,
    pub has_attachments: bool,
    /// Populated by conversation-grouped listings; 1 / 0 otherwise.
    #[serde(default = "one")]
    pub thread_count: i64,
    #[serde(default)]
    pub thread_unread: i64,
}

fn one() -> i64 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentInfo {
    pub id: i64,
    pub remote_id: String,
    pub filename: String,
    pub mime_type: String,
    pub size: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageDetail {
    #[serde(flatten)]
    pub summary: MessageSummary,
    pub body_html: Option<String>,
    pub body_text: Option<String>,
    pub message_id_hdr: Option<String>,
    pub references_hdr: Option<String>,
    pub attachments: Vec<AttachmentInfo>,
    /// Present when the sender offers a mailing-list unsubscribe.
    pub can_unsubscribe: bool,
}

/// Metadata for one message as reported by a provider.
#[derive(Debug, Clone, Default)]
pub struct RemoteMessage {
    pub remote_id: String,
    pub thread_id: Option<String>,
    pub subject: String,
    pub from_name: String,
    pub from_addr: String,
    pub to_addrs: String,
    pub cc_addrs: String,
    pub snippet: String,
    pub date: i64,
    pub labels: Vec<String>,
    pub is_read: bool,
    pub is_starred: bool,
    pub has_attachments: bool,
    pub message_id_hdr: Option<String>,
    pub references_hdr: Option<String>,
    pub list_unsubscribe: Option<String>,
    pub list_unsubscribe_post: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct RemoteAttachment {
    pub remote_id: String,
    pub filename: String,
    pub mime_type: String,
    pub size: i64,
}

#[derive(Debug, Clone, Default)]
pub struct RemoteBody {
    pub html: Option<String>,
    pub text: Option<String>,
    pub attachments: Vec<RemoteAttachment>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlagChange {
    pub read: Option<bool>,
    pub starred: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewMailInfo {
    pub id: i64,
    pub from_name: String,
    pub subject: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum SyncEvent {
    Started { account_id: i64, full: bool },
    Progress { account_id: i64, done: usize, total: usize },
    Finished { account_id: i64 },
    Failed { account_id: i64, error: String },
    /// Unread inbox messages that arrived since the previous pass.
    NewMail { account_id: i64, messages: Vec<NewMailInfo> },
}

/// Filter definition as entered in the UI. Mirrors Gmail's dialog; the
/// provider maps it onto its own criteria/action shape.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewFilter {
    #[serde(default)]
    pub from: String,
    #[serde(default)]
    pub to: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub has_words: String,
    #[serde(default)]
    pub not_words: String,
    #[serde(default)]
    pub has_attachment: bool,
    #[serde(default)]
    pub skip_inbox: bool,
    #[serde(default)]
    pub mark_read: bool,
    #[serde(default)]
    pub star: bool,
    #[serde(default)]
    pub add_label: Option<String>,
    #[serde(default)]
    pub delete: bool,
    #[serde(default)]
    pub never_spam: bool,
    #[serde(default)]
    pub mark_important: bool,
    /// Also run the action over matching mail already in the mailbox.
    #[serde(default)]
    pub apply_to_existing: bool,
}

impl NewFilter {
    /// Gmail search string equivalent to the criteria.
    pub fn as_query(&self) -> String {
        let mut q = Vec::new();
        if !self.from.trim().is_empty() {
            q.push(format!("from:({})", self.from.trim()));
        }
        if !self.to.trim().is_empty() {
            q.push(format!("to:({})", self.to.trim()));
        }
        if !self.subject.trim().is_empty() {
            q.push(format!("subject:({})", self.subject.trim()));
        }
        if !self.has_words.trim().is_empty() {
            q.push(self.has_words.trim().to_string());
        }
        if !self.not_words.trim().is_empty() {
            q.push(format!("-({})", self.not_words.trim()));
        }
        if self.has_attachment {
            q.push("has:attachment".into());
        }
        q.join(" ")
    }

    pub fn label_changes(&self) -> (Vec<String>, Vec<String>) {
        let mut add = Vec::new();
        let mut remove = Vec::new();
        if self.skip_inbox {
            remove.push("INBOX".into());
        }
        if self.mark_read {
            remove.push("UNREAD".into());
        }
        if self.star {
            add.push("STARRED".into());
        }
        if let Some(l) = &self.add_label {
            if !l.is_empty() {
                add.push(l.clone());
            }
        }
        if self.delete {
            add.push("TRASH".into());
        }
        if self.never_spam {
            remove.push("SPAM".into());
        }
        if self.mark_important {
            add.push("IMPORTANT".into());
        }
        (add, remove)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum OutgoingAttachment {
    /// A file on disk chosen by the user.
    Path { path: String },
    /// An attachment of a stored message (used when forwarding).
    Stored { message_id: i64, attachment_id: i64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutgoingMessage {
    pub account_id: i64,
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    #[serde(default)]
    pub bcc: Vec<String>,
    pub subject: String,
    pub body_text: String,
    /// Extra HTML appended below the user's text (quoted original when
    /// replying/forwarding). Already sanitised.
    #[serde(default)]
    pub quoted_html: Option<String>,
    #[serde(default)]
    pub in_reply_to: Option<String>,
    #[serde(default)]
    pub references: Option<String>,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub attachments: Vec<OutgoingAttachment>,
    /// Server draft this message was edited from; deleted after sending.
    #[serde(default)]
    pub draft_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReplyMode {
    Reply,
    ReplyAll,
    Forward,
}

/// Pre-filled composer state for reply / reply-all / forward.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposeDraft {
    pub account_id: i64,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: String,
    pub quoted_html: Option<String>,
    pub quoted_text: String,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
    pub thread_id: Option<String>,
    pub attachments: Vec<OutgoingAttachment>,
    pub attachment_names: Vec<String>,
}
