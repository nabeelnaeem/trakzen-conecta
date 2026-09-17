//! Provider-agnostic mail layer. Providers implement [`MailProvider`] and the
//! rest of the app only talks to the trait and the local store, so adding
//! Outlook/IMAP later means adding a module under `mail/` and registering it
//! in [`provider_for`].

pub mod commands;
pub mod compose;
pub mod gmail;
pub mod sanitize;
pub mod store;
pub mod types;

use std::sync::Arc;

use async_trait::async_trait;

use crate::error::{AppError, Result};
use store::MailStore;
use types::*;

pub type SyncObserver<'a> = &'a (dyn Fn(SyncEvent) + Send + Sync);

#[async_trait]
pub trait MailProvider: Send + Sync {
    fn kind(&self) -> ProviderKind;

    /// Interactive sign-in. Returns the account once the provider has stored
    /// whatever credentials it needs for later calls.
    async fn login(&self, app: &tauri::AppHandle, store: &MailStore) -> Result<Account>;

    /// Forget stored credentials for the account.
    async fn logout(&self, account: &Account) -> Result<()>;

    /// Bring the local store up to date with the server. Implementations
    /// decide between a full and an incremental pass using `account.sync_cursor`.
    async fn sync(&self, account: &Account, store: &MailStore, observe: SyncObserver<'_>)
        -> Result<()>;

    async fn fetch_body(&self, account: &Account, remote_id: &str) -> Result<RemoteBody>;

    async fn fetch_attachment(
        &self,
        account: &Account,
        remote_id: &str,
        attachment_remote_id: &str,
    ) -> Result<Vec<u8>>;

    /// `raw` is a complete RFC 5322 message.
    async fn send(&self, account: &Account, raw: Vec<u8>, thread_id: Option<&str>) -> Result<()>;

    async fn set_flags(&self, account: &Account, remote_id: &str, flags: FlagChange) -> Result<()>;

    async fn trash(&self, account: &Account, remote_id: &str) -> Result<()>;

    async fn archive(&self, account: &Account, remote_id: &str) -> Result<()>;

    async fn modify_labels(
        &self,
        account: &Account,
        remote_id: &str,
        add: &[String],
        remove: &[String],
    ) -> Result<()>;

    async fn list_labels(&self, account: &Account) -> Result<Vec<RemoteLabel>>;

    async fn list_filters(&self, account: &Account) -> Result<Vec<MailFilter>>;

    /// Pulls one page of message metadata for `label_id` (newest first),
    /// starting at `page_token`, into the store. Returns the number of
    /// messages that were new locally and the token for the next page.
    async fn fetch_label_page(
        &self,
        account: &Account,
        store: &MailStore,
        label_id: &str,
        page_token: Option<&str>,
    ) -> Result<(usize, Option<String>)>;
}

pub struct Providers {
    pub gmail: Arc<gmail::GmailProvider>,
}

impl Providers {
    pub fn provider_for(&self, kind: &str) -> Result<Arc<dyn MailProvider>> {
        match kind {
            "gmail" => Ok(self.gmail.clone()),
            other => Err(AppError::Provider(format!("unknown provider '{other}'"))),
        }
    }
}
