mod api;
mod auth;

pub use api::split_mailbox;

use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use futures::{stream, StreamExt};

use crate::db::Db;
use crate::error::{AppError, Result};

use super::store::MailStore;
use super::types::*;
use super::{MailProvider, SyncObserver};
use api::{ApiError, GmailApi};
use auth::GmailAuth;

/// How many messages the first sync pulls. Everything older is fetched on
/// demand later (roadmap: "load more" paging against the server).
const INITIAL_SYNC_LIMIT: usize = 300;
const FETCH_CONCURRENCY: usize = 8;

pub struct GmailProvider {
    auth: GmailAuth,
    api: GmailApi,
}

impl GmailProvider {
    pub fn new(db: Arc<Db>) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(concat!("trakzen-conecta/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("reqwest client");
        Self {
            auth: GmailAuth::new(db, http.clone()),
            api: GmailApi::new(http),
        }
    }

    async fn token(&self, account: &Account) -> Result<String> {
        self.auth.access_token(&account.email).await
    }

    /// Returns the messages that were not in the store before this call.
    async fn fetch_and_store(
        &self,
        token: &str,
        account: &Account,
        store: &MailStore,
        ids: Vec<String>,
        observe: SyncObserver<'_>,
    ) -> Result<Vec<RemoteMessage>> {
        let total = ids.len();
        let mut done = 0usize;
        let mut batch: Vec<RemoteMessage> = Vec::with_capacity(32);
        let mut gone: Vec<String> = Vec::new();
        let known: HashSet<String> = store.known_remote_ids(account.id)?.into_iter().collect();
        let mut fresh: Vec<RemoteMessage> = Vec::new();

        let mut results = stream::iter(ids)
            .map(|id| async move { (id.clone(), self.api.get_metadata(token, &id).await) })
            .buffer_unordered(FETCH_CONCURRENCY);

        while let Some((id, res)) = results.next().await {
            match res? {
                Some(m) => {
                    if !known.contains(&m.remote_id) {
                        fresh.push(m.clone());
                    }
                    batch.push(m)
                }
                None => gone.push(id),
            }
            done += 1;
            if batch.len() >= 32 {
                store.upsert_messages(account.id, &batch)?;
                batch.clear();
                observe(SyncEvent::Progress {
                    account_id: account.id,
                    done,
                    total,
                });
            }
        }
        if !batch.is_empty() {
            store.upsert_messages(account.id, &batch)?;
        }
        if !gone.is_empty() {
            store.delete_messages(account.id, &gone)?;
        }
        observe(SyncEvent::Progress {
            account_id: account.id,
            done,
            total,
        });
        Ok(fresh)
    }

    async fn full_sync(
        &self,
        token: &str,
        account: &Account,
        store: &MailStore,
        observe: SyncObserver<'_>,
    ) -> Result<()> {
        // Grab the cursor *before* listing so nothing that lands mid-sync
        // falls between the initial pull and the first history pass.
        let profile = self.api.profile(token).await?;

        let mut ids = Vec::new();
        let mut page = None;
        while ids.len() < INITIAL_SYNC_LIMIT {
            let want = (INITIAL_SYNC_LIMIT - ids.len()).min(500) as u32;
            let list = self.api.list_ids(token, None, want, page.as_deref()).await?;
            ids.extend(list.messages.into_iter().map(|m| m.id));
            match list.next_page_token {
                Some(t) => page = Some(t),
                None => break,
            }
        }

        let known: HashSet<String> = store.known_remote_ids(account.id)?.into_iter().collect();
        let fresh: Vec<String> = ids.iter().filter(|id| !known.contains(*id)).cloned().collect();
        let refresh: Vec<String> = ids.iter().filter(|id| known.contains(*id)).cloned().collect();

        // New messages first so the inbox fills in quickly; label refreshes
        // for ones we already have can trail behind.
        self.fetch_and_store(token, account, store, fresh, observe).await?;
        self.fetch_and_store(token, account, store, refresh, observe).await?;

        // One cheap ids-only query gives us the paperclip icon without
        // pulling full payloads for every message.
        let with_att = self
            .api
            .list_ids(token, Some("has:attachment"), 500, None)
            .await?;
        let ids: Vec<String> = with_att.messages.into_iter().map(|m| m.id).collect();
        store.mark_has_attachments(account.id, &ids)?;

        store.set_cursor(account.id, Some(&profile.history_id))?;
        Ok(())
    }

    async fn refresh_labels(&self, token: &str, account: &Account, store: &MailStore) -> Result<()> {
        let labels = self.api.list_labels(token).await?;
        store.replace_labels(account.id, &labels)
    }

    /// Returns `Ok(false)` when the cursor is too old and a full sync is needed.
    async fn incremental_sync(
        &self,
        token: &str,
        account: &Account,
        store: &MailStore,
        cursor: &str,
        observe: SyncObserver<'_>,
    ) -> Result<bool> {
        let mut touched: HashSet<String> = HashSet::new();
        let mut deleted: HashSet<String> = HashSet::new();
        let mut latest = cursor.to_string();
        let mut page = None;

        loop {
            let hp = match self.api.history(token, cursor, page.as_deref()).await {
                Ok(hp) => hp,
                Err(ApiError::NotFound) => return Ok(false),
                Err(ApiError::Other(e)) => return Err(e),
            };
            if let Some(h) = hp.history_id {
                latest = h;
            }
            for rec in &hp.history {
                for key in ["messagesAdded", "labelsAdded", "labelsRemoved"] {
                    if let Some(items) = rec[key].as_array() {
                        for it in items {
                            if let Some(id) = it["message"]["id"].as_str() {
                                touched.insert(id.to_string());
                            }
                        }
                    }
                }
                if let Some(items) = rec["messagesDeleted"].as_array() {
                    for it in items {
                        if let Some(id) = it["message"]["id"].as_str() {
                            deleted.insert(id.to_string());
                            touched.remove(id);
                        }
                    }
                }
            }
            match hp.next_page_token {
                Some(t) => page = Some(t),
                None => break,
            }
        }

        if !deleted.is_empty() {
            store.delete_messages(account.id, &deleted.into_iter().collect::<Vec<_>>())?;
        }
        let ids: Vec<String> = touched.into_iter().collect();
        let fresh = self.fetch_and_store(token, account, store, ids, observe).await?;
        store.set_cursor(account.id, Some(&latest))?;

        let arrivals: Vec<&RemoteMessage> = fresh
            .iter()
            .filter(|m| !m.is_read && m.labels.iter().any(|l| l == "INBOX"))
            .collect();
        if !arrivals.is_empty() {
            let remote_ids: Vec<String> = arrivals.iter().map(|m| m.remote_id.clone()).collect();
            let rows = store.summaries_by_remote_ids(account.id, &remote_ids)?;
            observe(SyncEvent::NewMail {
                account_id: account.id,
                messages: rows
                    .into_iter()
                    .map(|m| NewMailInfo {
                        id: m.id,
                        from_name: if m.from_name.is_empty() { m.from_addr } else { m.from_name },
                        subject: m.subject,
                    })
                    .collect(),
            });
        }
        Ok(true)
    }
}

/// Maps a UI view onto Gmail list parameters: label ids (ANDed) plus an
/// optional search string for the parts labels cannot express.
fn view_params(query: &ListQuery) -> (Vec<&'static str>, Option<String>, Vec<String>) {
    // Returns (static labels, q, owned labels) – owned for user label ids.
    if let Some(l) = &query.label {
        return (vec![], None, vec![l.clone()]);
    }
    match query.folder {
        Folder::Inbox => match query.category {
            Some(Category::Primary) => (vec!["INBOX"], Some("category:primary".into()), vec![]),
            Some(c) => (vec!["INBOX", c.label_id()], None, vec![]),
            None => (vec!["INBOX"], None, vec![]),
        },
        Folder::Starred => (vec!["STARRED"], None, vec![]),
        Folder::Sent => (vec!["SENT"], None, vec![]),
        Folder::Drafts => (vec!["DRAFT"], None, vec![]),
        Folder::Trash => (vec!["TRASH"], None, vec![]),
        Folder::Spam => (vec!["SPAM"], None, vec![]),
        Folder::Archive => (vec![], Some("-in:inbox -in:sent -in:drafts -in:spam -in:trash".into()), vec![]),
        Folder::All | Folder::Snoozed => (vec![], None, vec![]),
    }
}

#[async_trait]
impl MailProvider for GmailProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Gmail
    }

    async fn login(&self, app: &tauri::AppHandle, store: &MailStore) -> Result<Account> {
        let tokens = self.auth.login(app).await?;
        let profile = self.api.profile(&tokens.access_token).await?;
        self.auth.store_tokens(&profile.email_address, tokens)?;
        store.upsert_account(ProviderKind::Gmail.as_str(), &profile.email_address, None)
    }

    async fn logout(&self, account: &Account) -> Result<()> {
        self.auth.logout(&account.email).await
    }

    async fn sync(
        &self,
        account: &Account,
        store: &MailStore,
        observe: SyncObserver<'_>,
    ) -> Result<()> {
        let token = self.token(account).await?;
        // Labels are cheap and change rarely; refresh them on every pass so
        // a label created on the web shows up on the next sync.
        self.refresh_labels(&token, account, store).await?;
        let full = match &account.sync_cursor {
            Some(cursor) => {
                observe(SyncEvent::Started {
                    account_id: account.id,
                    full: false,
                });
                !self
                    .incremental_sync(&token, account, store, cursor, observe)
                    .await?
            }
            None => true,
        };
        if full {
            observe(SyncEvent::Started {
                account_id: account.id,
                full: true,
            });
            self.full_sync(&token, account, store, observe).await?;
        }
        Ok(())
    }

    async fn fetch_body(&self, account: &Account, remote_id: &str) -> Result<RemoteBody> {
        let token = self.token(account).await?;
        self.api.get_full(&token, remote_id).await
    }

    async fn fetch_attachment(
        &self,
        account: &Account,
        remote_id: &str,
        attachment_remote_id: &str,
    ) -> Result<Vec<u8>> {
        let token = self.token(account).await?;
        self.api
            .get_attachment(&token, remote_id, attachment_remote_id)
            .await
    }

    async fn send(&self, account: &Account, raw: Vec<u8>, thread_id: Option<&str>) -> Result<()> {
        let token = self.token(account).await?;
        self.api.send(&token, &raw, thread_id).await
    }

    async fn set_flags(&self, account: &Account, remote_id: &str, flags: FlagChange) -> Result<()> {
        let token = self.token(account).await?;
        let mut add = Vec::new();
        let mut remove = Vec::new();
        match flags.read {
            Some(true) => remove.push("UNREAD"),
            Some(false) => add.push("UNREAD"),
            None => {}
        }
        match flags.starred {
            Some(true) => add.push("STARRED"),
            Some(false) => remove.push("STARRED"),
            None => {}
        }
        if add.is_empty() && remove.is_empty() {
            return Ok(());
        }
        self.api.modify_labels(&token, remote_id, &add, &remove).await
    }

    async fn trash(&self, account: &Account, remote_id: &str) -> Result<()> {
        let token = self.token(account).await?;
        self.api.trash(&token, remote_id).await
    }

    async fn archive(&self, account: &Account, remote_id: &str) -> Result<()> {
        let token = self.token(account).await?;
        self.api
            .modify_labels(&token, remote_id, &[], &["INBOX"])
            .await
    }

    async fn modify_labels(
        &self,
        account: &Account,
        remote_id: &str,
        add: &[String],
        remove: &[String],
    ) -> Result<()> {
        let token = self.token(account).await?;
        let add: Vec<&str> = add.iter().map(String::as_str).collect();
        let remove: Vec<&str> = remove.iter().map(String::as_str).collect();
        self.api.modify_labels(&token, remote_id, &add, &remove).await
    }

    async fn list_labels(&self, account: &Account) -> Result<Vec<RemoteLabel>> {
        let token = self.token(account).await?;
        self.api.list_labels(&token).await
    }

    async fn create_label(&self, account: &Account, name: &str) -> Result<RemoteLabel> {
        let token = self.token(account).await?;
        self.api.create_label(&token, name).await
    }

    async fn list_filters(&self, account: &Account) -> Result<Vec<MailFilter>> {
        let token = self.token(account).await?;
        self.api.list_filters(&token).await
    }

    async fn create_filter(&self, account: &Account, filter: &NewFilter) -> Result<MailFilter> {
        let token = self.token(account).await?;
        let created = self.api.create_filter(&token, filter).await?;
        if filter.apply_to_existing {
            let (add, remove) = filter.label_changes();
            let q = filter.as_query();
            let mut page = None;
            let mut rounds = 0;
            loop {
                let list = self.api.list_ids_in(&token, None, Some(&q), 500, page.as_deref()).await?;
                let ids: Vec<String> = list.messages.into_iter().map(|m| m.id).collect();
                if !ids.is_empty() {
                    self.api.batch_modify(&token, &ids, &add, &remove).await?;
                }
                rounds += 1;
                match list.next_page_token {
                    // Cap the sweep so a very broad filter cannot run for minutes.
                    Some(t) if rounds < 10 => page = Some(t),
                    _ => break,
                }
            }
        }
        Ok(created)
    }

    async fn delete_filter(&self, account: &Account, filter_id: &str) -> Result<()> {
        let token = self.token(account).await?;
        self.api.delete_filter(&token, filter_id).await
    }

    async fn batch_modify(
        &self,
        account: &Account,
        remote_ids: &[String],
        add: &[String],
        remove: &[String],
    ) -> Result<()> {
        let token = self.token(account).await?;
        self.api.batch_modify(&token, remote_ids, add, remove).await
    }

    async fn modify_thread(
        &self,
        account: &Account,
        thread_id: &str,
        add: &[String],
        remove: &[String],
    ) -> Result<()> {
        let token = self.token(account).await?;
        self.api.modify_thread(&token, thread_id, add, remove).await
    }

    async fn save_draft(
        &self,
        account: &Account,
        raw: Vec<u8>,
        thread_id: Option<&str>,
        draft_id: Option<&str>,
    ) -> Result<String> {
        let token = self.token(account).await?;
        self.api.save_draft(&token, &raw, thread_id, draft_id).await
    }

    async fn delete_draft(&self, account: &Account, draft_id: &str) -> Result<()> {
        let token = self.token(account).await?;
        self.api.delete_draft(&token, draft_id).await
    }

    async fn open_draft(&self, account: &Account, message_remote_id: &str) -> Result<DraftContent> {
        let token = self.token(account).await?;
        let id = self
            .api
            .find_draft(&token, message_remote_id)
            .await?
            .ok_or_else(|| AppError::NotFound("draft no longer exists on the server".into()))?;
        self.api.get_draft(&token, &id).await
    }

    async fn modify_threads(
        &self,
        account: &Account,
        thread_ids: &[String],
        add: &[String],
        remove: &[String],
    ) -> Result<()> {
        let token = self.token(account).await?;
        // Chunked rather than buffer_unordered: keeps the closure lifetimes
        // simple under async-trait and stays under Gmail's per-second quota.
        for chunk in thread_ids.chunks(FETCH_CONCURRENCY) {
            let mut set = futures::stream::FuturesUnordered::new();
            for t in chunk {
                set.push(self.api.modify_thread(&token, t, add, remove));
            }
            while let Some(r) = set.next().await {
                r?;
            }
        }
        Ok(())
    }

    async fn count_view(&self, account: &Account, query: &ListQuery) -> Result<u64> {
        if query.folder == Folder::Snoozed {
            return Ok(0);
        }
        let token = self.token(account).await?;
        let (labels, q, owned) = view_params(query);
        let mut all: Vec<&str> = labels;
        all.extend(owned.iter().map(String::as_str));
        self.api.estimate(&token, &all, q.as_deref()).await
    }

    async fn modify_view(
        &self,
        account: &Account,
        query: &ListQuery,
        add: &[String],
        remove: &[String],
    ) -> Result<usize> {
        if query.folder == Folder::Snoozed {
            return Ok(0);
        }
        let token = self.token(account).await?;
        let (labels, q, owned) = view_params(query);
        let mut all: Vec<&str> = labels;
        all.extend(owned.iter().map(String::as_str));
        let mut page: Option<String> = None;
        let mut touched = 0usize;
        loop {
            let list = self
                .api
                .list_ids_labels(&token, &all, q.as_deref(), 500, page.as_deref())
                .await?;
            let ids: Vec<String> = list.messages.into_iter().map(|m| m.id).collect();
            if ids.is_empty() {
                break;
            }
            touched += ids.len();
            self.api.batch_modify(&token, &ids, add, remove).await?;
            match list.next_page_token {
                // 20 pages = 10k messages; beyond that the user should narrow the view.
                Some(t) if touched < 10_000 => page = Some(t),
                _ => break,
            }
        }
        Ok(touched)
    }

    async fn search(
        &self,
        account: &Account,
        store: &MailStore,
        query: &str,
        max: u32,
    ) -> Result<Vec<String>> {
        let token = self.token(account).await?;
        let list = self.api.list_ids_in(&token, None, Some(query), max, None).await?;
        let ids: Vec<String> = list.messages.into_iter().map(|m| m.id).collect();
        let noop = |_: SyncEvent| {};
        self.fetch_and_store(&token, account, store, ids.clone(), &noop).await?;
        Ok(ids)
    }

    async fn fetch_label_page(
        &self,
        account: &Account,
        store: &MailStore,
        label_id: &str,
        page_token: Option<&str>,
    ) -> Result<(usize, Option<String>)> {
        let token = self.token(account).await?;
        let list = self
            .api
            .list_ids_in(&token, Some(label_id), None, 100, page_token)
            .await?;
        let known: HashSet<String> = store.known_remote_ids(account.id)?.into_iter().collect();
        let fresh: Vec<String> = list
            .messages
            .into_iter()
            .map(|m| m.id)
            .filter(|id| !known.contains(id))
            .collect();
        let added = fresh.len();
        let noop = |_: SyncEvent| {};
        self.fetch_and_store(&token, account, store, fresh, &noop).await?;
        Ok((added, list.next_page_token))
    }
}

impl From<ApiError> for AppError {
    fn from(e: ApiError) -> Self {
        match e {
            ApiError::NotFound => AppError::NotFound("gmail resource".into()),
            ApiError::Other(e) => e,
        }
    }
}
