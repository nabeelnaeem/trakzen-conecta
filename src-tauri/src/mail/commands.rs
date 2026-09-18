use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::error::{AppError, Result};
use crate::AppState;

use super::compose::{self, ResolvedAttachment};
use super::sanitize;
use super::types::*;

pub const EVENT_SYNC: &str = "mail://sync";

#[derive(Default)]
pub struct SyncGuard(Mutex<HashSet<i64>>);

#[derive(Clone, Default)]
pub struct PageState {
    /// `None` once the server has no more pages.
    pub next: Option<String>,
    /// The view is complete in the cache down to this date.
    pub oldest: i64,
    pub exhausted: bool,
}

/// Server paging position per (account, label). Absent = not started.
#[derive(Default)]
pub struct PageTokens(Mutex<HashMap<(i64, String), PageState>>);

/// Oldest date the list may show for a view without risking a gap.
fn view_floor(state: &AppState, account_id: i64, query: &ListQuery) -> i64 {
    let Some(label) = query.label_id() else { return 0 };
    let tokens = state.page_tokens.0.lock().unwrap();
    match tokens.get(&(account_id, label)) {
        Some(p) if p.exhausted => 0,
        Some(p) => p.oldest,
        None => 0,
    }
}

#[tauri::command]
pub async fn mail_list_accounts(state: State<'_, AppState>) -> Result<Vec<Account>> {
    state.mail.list_accounts()
}

#[tauri::command]
pub async fn mail_add_account(
    app: AppHandle,
    state: State<'_, AppState>,
    provider: String,
) -> Result<Account> {
    let p = state.providers.provider_for(&provider)?;
    let account = p.login(&app, &state.mail).await?;
    spawn_sync(app.clone(), account.id);
    Ok(account)
}

#[tauri::command]
pub async fn mail_remove_account(state: State<'_, AppState>, account_id: i64) -> Result<()> {
    let account = state.mail.get_account(account_id)?;
    let p = state.providers.provider_for(&account.provider)?;
    // Best effort: a revoke failure should not leave the account stuck.
    if let Err(e) = p.logout(&account).await {
        tracing::warn!(%e, "logout failed");
    }
    state.mail.delete_account(account_id)
}

#[tauri::command]
pub async fn mail_sync(app: AppHandle, account_id: i64) -> Result<()> {
    if account_id == super::UNIFIED_ACCOUNT {
        for a in app.state::<AppState>().mail.list_accounts()? {
            spawn_sync(app.clone(), a.id);
        }
    } else {
        spawn_sync(app, account_id);
    }
    Ok(())
}

pub fn spawn_sync(app: AppHandle, account_id: i64) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        {
            let mut running = state.sync_guard.0.lock().unwrap();
            if !running.insert(account_id) {
                return;
            }
        }
        let result = run_sync(&app, &state, account_id).await;
        state.sync_guard.0.lock().unwrap().remove(&account_id);
        let ev = match result {
            Ok(()) => SyncEvent::Finished { account_id },
            Err(e) => {
                tracing::error!(account_id, error = %e, "sync failed");
                SyncEvent::Failed {
                    account_id,
                    error: e.to_string(),
                }
            }
        };
        let _ = app.emit(EVENT_SYNC, ev);
    });
}

async fn run_sync(app: &AppHandle, state: &AppState, account_id: i64) -> Result<()> {
    let account = state.mail.get_account(account_id)?;
    let provider = state.providers.provider_for(&account.provider)?;
    let emitter = app.clone();
    let observe = move |ev: SyncEvent| {
        let _ = emitter.emit(EVENT_SYNC, ev);
    };
    provider.sync(&account, &state.mail, &observe).await
}

#[tauri::command]
pub async fn mail_list_messages(
    state: State<'_, AppState>,
    account_id: i64,
    query: ListQuery,
    limit: Option<i64>,
    offset: Option<i64>,
    conversations: Option<bool>,
) -> Result<Vec<MessageSummary>> {
    let floor = view_floor(&state, account_id, &query);
    state.mail.list_messages_since(
        account_id,
        &query,
        limit.unwrap_or(100).clamp(1, 5000),
        offset.unwrap_or(0).max(0),
        conversations.unwrap_or(false),
        floor,
    )
}

#[tauri::command]
pub async fn mail_list_thread(
    state: State<'_, AppState>,
    account_id: i64,
    thread_id: String,
) -> Result<Vec<MessageSummary>> {
    state.mail.list_thread(account_id, &thread_id)
}

/// Label change applied to a set of local message ids (one account) with a
/// single batched provider call. Used by multi-select and by thread-level
/// actions, which pass every message of the thread.
#[tauri::command]
pub async fn mail_bulk_modify(
    state: State<'_, AppState>,
    message_ids: Vec<i64>,
    add: Vec<String>,
    remove: Vec<String>,
) -> Result<()> {
    if message_ids.is_empty() {
        return Ok(());
    }
    let rows = state.mail.remote_ids(&message_ids)?;
    let Some(&(_, account_id, _)) = rows.first() else {
        return Ok(());
    };
    if rows.iter().any(|(_, a, _)| *a != account_id) {
        return Err(AppError::Other("selection spans multiple accounts".into()));
    }
    let account = state.mail.get_account(account_id)?;
    let provider = state.providers.provider_for(&account.provider)?;
    let remote: Vec<String> = rows.iter().map(|(_, _, r)| r.clone()).collect();
    provider.batch_modify(&account, &remote, &add, &remove).await?;
    let ids: Vec<i64> = rows.iter().map(|(id, _, _)| *id).collect();
    let add: Vec<&str> = add.iter().map(String::as_str).collect();
    let remove: Vec<&str> = remove.iter().map(String::as_str).collect();
    state.mail.set_labels_bulk(&ids, &add, &remove)
}

/// Gmail's estimate of how many messages a view holds on the server.
#[tauri::command]
pub async fn mail_view_count(
    state: State<'_, AppState>,
    account_id: i64,
    query: ListQuery,
) -> Result<u64> {
    let account = state.mail.get_account(account_id)?;
    let provider = state.providers.provider_for(&account.provider)?;
    provider.count_view(&account, &query).await
}

/// Label change over an entire view on the server ("select all N
/// conversations in Inbox"), mirrored onto whatever is cached locally.
#[tauri::command]
pub async fn mail_modify_view(
    state: State<'_, AppState>,
    account_id: i64,
    query: ListQuery,
    add: Vec<String>,
    remove: Vec<String>,
) -> Result<usize> {
    let account = state.mail.get_account(account_id)?;
    let provider = state.providers.provider_for(&account.provider)?;
    let n = provider.modify_view(&account, &query, &add, &remove).await?;
    let rows = state.mail.list_messages(account_id, &query, 100_000, 0)?;
    let ids: Vec<i64> = rows.iter().map(|m| m.id).collect();
    let add: Vec<&str> = add.iter().map(String::as_str).collect();
    let remove: Vec<&str> = remove.iter().map(String::as_str).collect();
    state.mail.set_labels_bulk(&ids, &add, &remove)?;
    Ok(n)
}

#[tauri::command]
pub async fn mail_mark_view(
    state: State<'_, AppState>,
    account_id: i64,
    query: ListQuery,
    read: bool,
) -> Result<usize> {
    let (add, remove) = if read {
        (vec![], vec!["UNREAD".to_string()])
    } else {
        (vec!["UNREAD".to_string()], vec![])
    };
    mail_modify_view(state, account_id, query, add, remove).await
}

/// One change across many threads; resolves to a concurrent batch on the
/// provider side and one local update.
#[tauri::command]
pub async fn mail_threads_modify(
    state: State<'_, AppState>,
    account_id: i64,
    thread_ids: Vec<String>,
    add: Vec<String>,
    remove: Vec<String>,
) -> Result<()> {
    if thread_ids.is_empty() {
        return Ok(());
    }
    let account = state.mail.get_account(account_id)?;
    let provider = state.providers.provider_for(&account.provider)?;
    provider
        .modify_threads(&account, &thread_ids, &add, &remove)
        .await?;
    let mut ids = Vec::new();
    for t in &thread_ids {
        ids.extend(state.mail.thread_local_ids(account_id, t)?);
    }
    let add: Vec<&str> = add.iter().map(String::as_str).collect();
    let remove: Vec<&str> = remove.iter().map(String::as_str).collect();
    state.mail.set_labels_bulk(&ids, &add, &remove)
}

#[tauri::command]
pub async fn mail_thread_modify(
    state: State<'_, AppState>,
    account_id: i64,
    thread_id: String,
    add: Vec<String>,
    remove: Vec<String>,
) -> Result<()> {
    let account = state.mail.get_account(account_id)?;
    let provider = state.providers.provider_for(&account.provider)?;
    provider
        .modify_thread(&account, &thread_id, &add, &remove)
        .await?;
    let ids = state.mail.thread_local_ids(account_id, &thread_id)?;
    let add: Vec<&str> = add.iter().map(String::as_str).collect();
    let remove: Vec<&str> = remove.iter().map(String::as_str).collect();
    state.mail.set_labels_bulk(&ids, &add, &remove)
}

#[tauri::command]
pub async fn mail_create_label(
    state: State<'_, AppState>,
    account_id: i64,
    name: String,
) -> Result<Label> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::Other("label name is required".into()));
    }
    let account = state.mail.get_account(account_id)?;
    let provider = state.providers.provider_for(&account.provider)?;
    provider.create_label(&account, name).await?;
    let labels = provider.list_labels(&account).await?;
    state.mail.replace_labels(account_id, &labels)?;
    state
        .mail
        .list_labels(account_id)?
        .into_iter()
        .find(|l| l.name == name)
        .ok_or_else(|| AppError::Other("label created but not found".into()))
}

#[tauri::command]
pub async fn mail_create_filter(
    state: State<'_, AppState>,
    account_id: i64,
    filter: NewFilter,
) -> Result<MailFilter> {
    let account = state.mail.get_account(account_id)?;
    let provider = state.providers.provider_for(&account.provider)?;
    let created = provider.create_filter(&account, &filter).await.map_err(|e| match e {
        // Accounts connected before the settings scope was added must
        // re-consent; make that actionable instead of a bare 403.
        AppError::Provider(msg) if msg.contains("insufficient") || msg.contains("403") => {
            AppError::Auth("This account needs to be signed in again to manage filters (Settings → Sign in again).".into())
        }
        other => other,
    })?;
    if filter.apply_to_existing {
        spawn_sync(state.app.clone(), account_id);
    }
    Ok(created)
}

/// Gmail has no filter update; replace = create the new one, then delete
/// the old one (in that order so a failure never loses the rule).
#[tauri::command]
pub async fn mail_update_filter(
    state: State<'_, AppState>,
    account_id: i64,
    filter_id: String,
    filter: NewFilter,
) -> Result<MailFilter> {
    let account = state.mail.get_account(account_id)?;
    let provider = state.providers.provider_for(&account.provider)?;
    let created = provider.create_filter(&account, &filter).await?;
    provider.delete_filter(&account, &filter_id).await?;
    Ok(created)
}

#[tauri::command]
pub async fn mail_delete_filter(
    state: State<'_, AppState>,
    account_id: i64,
    filter_id: String,
) -> Result<()> {
    let account = state.mail.get_account(account_id)?;
    let provider = state.providers.provider_for(&account.provider)?;
    provider.delete_filter(&account, &filter_id).await
}

/// Re-runs the consent flow for an existing account, e.g. after the app
/// started asking for an additional scope.
#[tauri::command]
pub async fn mail_reauth(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: i64,
) -> Result<Account> {
    let account = state.mail.get_account(account_id)?;
    let provider = state.providers.provider_for(&account.provider)?;
    let fresh = provider.login(&app, &state.mail).await?;
    if fresh.email != account.email {
        return Err(AppError::Auth(format!(
            "you signed in as {} but this account is {}",
            fresh.email, account.email
        )));
    }
    Ok(fresh)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnsubscribeResult {
    /// "one-click" (done silently), "mailto" (an unsubscribe email was
    /// sent) or "browser" (a page was opened for the user to finish).
    pub method: String,
}

/// Unsubscribes via the message's List-Unsubscribe header: the RFC 8058
/// one-click POST when offered, otherwise a mailto (sent from the account)
/// or an https link opened in the browser.
#[tauri::command]
pub async fn mail_unsubscribe(
    app: AppHandle,
    state: State<'_, AppState>,
    message_id: i64,
) -> Result<UnsubscribeResult> {
    let (header, post) = state.mail.unsubscribe_headers(message_id)?;
    let header = header.ok_or_else(|| AppError::Other("this message has no unsubscribe link".into()))?;
    let targets: Vec<String> = header
        .split(',')
        .map(|s| s.trim().trim_start_matches('<').trim_end_matches('>').to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let https = targets.iter().find(|t| t.starts_with("https://") || t.starts_with("http://"));
    let mailto = targets.iter().find(|t| t.starts_with("mailto:"));

    if let (Some(url), Some(p)) = (https, &post) {
        if p.contains("List-Unsubscribe=One-Click") {
            let res = reqwest::Client::new()
                .post(url)
                .header("Content-Type", "application/x-www-form-urlencoded")
                .body("List-Unsubscribe=One-Click")
                .send()
                .await?;
            if res.status().is_success() {
                return Ok(UnsubscribeResult { method: "one-click".into() });
            }
        }
    }
    if let Some(m) = mailto {
        let detail = state.mail.get_message(message_id)?;
        let account = state.mail.get_account(detail.summary.account_id)?;
        let provider = state.providers.provider_for(&account.provider)?;
        let rest = &m["mailto:".len()..];
        let (addr, query) = rest.split_once('?').unwrap_or((rest, ""));
        let subject = url::form_urlencoded::parse(query.as_bytes())
            .find(|(k, _)| k == "subject")
            .map(|(_, v)| v.into_owned())
            .unwrap_or_else(|| "unsubscribe".into());
        let msg = OutgoingMessage {
            account_id: account.id,
            to: vec![addr.to_string()],
            cc: vec![],
            bcc: vec![],
            subject,
            body_text: "unsubscribe".into(),
            quoted_html: None,
            in_reply_to: None,
            references: None,
            thread_id: None,
            attachments: vec![],
            draft_id: None,
        };
        let raw = compose::build_raw(&account, &msg, vec![])?;
        provider.send(&account, raw, None).await?;
        return Ok(UnsubscribeResult { method: "mailto".into() });
    }
    if let Some(url) = https {
        app.opener()
            .open_url(url, None::<&str>)
            .map_err(|e| AppError::Other(format!("could not open browser: {e}")))?;
        return Ok(UnsubscribeResult { method: "browser".into() });
    }
    Err(AppError::Other("the unsubscribe header had no usable link".into()))
}

#[tauri::command]
pub async fn mail_search_server(
    state: State<'_, AppState>,
    account_id: i64,
    query: String,
) -> Result<Vec<MessageSummary>> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    let account = state.mail.get_account(account_id)?;
    let provider = state.providers.provider_for(&account.provider)?;
    let ids = provider
        .search(&account, &state.mail, query.trim(), 100)
        .await?;
    state.mail.summaries_by_remote_ids(account_id, &ids)
}

#[tauri::command]
pub async fn mail_snooze(state: State<'_, AppState>, message_id: i64, until: i64) -> Result<()> {
    state.mail.snooze(message_id, until)
}

#[tauri::command]
pub async fn mail_unsnooze(state: State<'_, AppState>, message_id: i64) -> Result<()> {
    state.mail.unsnooze(message_id)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snoozed {
    #[serde(flatten)]
    pub message: MessageSummary,
    pub until: i64,
}

#[tauri::command]
pub async fn mail_list_snoozed(state: State<'_, AppState>, account_id: i64) -> Result<Vec<Snoozed>> {
    Ok(state
        .mail
        .list_snoozed(account_id)?
        .into_iter()
        .map(|(message, until)| Snoozed { message, until })
        .collect())
}

#[tauri::command]
pub async fn mail_list_labels(state: State<'_, AppState>, account_id: i64) -> Result<Vec<Label>> {
    state.mail.list_labels(account_id)
}

#[tauri::command]
pub async fn mail_modify_labels(
    state: State<'_, AppState>,
    message_id: i64,
    add: Vec<String>,
    remove: Vec<String>,
) -> Result<MessageDetail> {
    let detail = state.mail.get_message(message_id)?;
    let account = state.mail.get_account(detail.summary.account_id)?;
    let provider = state.providers.provider_for(&account.provider)?;
    provider
        .modify_labels(&account, &detail.summary.remote_id, &add, &remove)
        .await?;
    let add: Vec<&str> = add.iter().map(String::as_str).collect();
    let remove: Vec<&str> = remove.iter().map(String::as_str).collect();
    state.mail.set_labels(message_id, &add, &remove)?;
    state.mail.get_message(message_id)
}

/// Pulls the next page of a folder/label from the server. The UI calls this
/// when a view is opened and again for "load more"; `reset` restarts from
/// the newest message.
#[tauri::command]
pub async fn mail_fetch_more(
    state: State<'_, AppState>,
    account_id: i64,
    query: ListQuery,
    reset: bool,
) -> Result<FetchResult> {
    if account_id == super::UNIFIED_ACCOUNT {
        let mut added = 0usize;
        let mut has_more = false;
        for a in state.mail.list_accounts()? {
            let r = fetch_more_one(&state, a.id, &query, reset).await?;
            added += r.added;
            has_more |= r.has_more;
        }
        return Ok(FetchResult { added, has_more });
    }
    fetch_more_one(&state, account_id, &query, reset).await
}

async fn fetch_more_one(
    state: &AppState,
    account_id: i64,
    query: &ListQuery,
    reset: bool,
) -> Result<FetchResult> {
    let Some(label_id) = query.label_id() else {
        return Ok(FetchResult {
            added: 0,
            has_more: false,
        });
    };
    let key = (account_id, label_id.clone());
    let token = {
        let mut tokens = state.page_tokens.0.lock().unwrap();
        if reset {
            tokens.remove(&key);
        }
        match tokens.get(&key) {
            Some(p) if p.exhausted => {
                return Ok(FetchResult {
                    added: 0,
                    has_more: false,
                })
            }
            Some(p) => p.next.clone(),
            None => None,
        }
    };

    let account = state.mail.get_account(account_id)?;
    let provider = state.providers.provider_for(&account.provider)?;
    let page = provider
        .fetch_label_page(&account, &state.mail, &label_id, token.as_deref())
        .await?;
    let has_more = page.next_page.is_some();
    let mut tokens = state.page_tokens.0.lock().unwrap();
    let entry = tokens.entry(key).or_default();
    entry.next = page.next_page;
    entry.exhausted = !has_more;
    // Only ever move the floor down; an empty page keeps the previous one.
    if let Some(o) = page.oldest {
        entry.oldest = if entry.oldest == 0 { o } else { entry.oldest.min(o) };
    }
    Ok(FetchResult {
        added: page.added,
        has_more,
    })
}

#[tauri::command]
pub async fn mail_suggest_contacts(
    state: State<'_, AppState>,
    account_id: i64,
    query: String,
) -> Result<Vec<Contact>> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    state.mail.suggest_contacts(account_id, &query, 8)
}

#[tauri::command]
pub async fn mail_list_filters(
    state: State<'_, AppState>,
    account_id: i64,
) -> Result<Vec<MailFilter>> {
    let account = state.mail.get_account(account_id)?;
    let provider = state.providers.provider_for(&account.provider)?;
    let mut filters = provider.list_filters(&account).await?;
    // Swap label ids for the names the user knows.
    let names = state.mail.label_names(account_id)?;
    let pretty = |id: &String| -> String {
        names.get(id).cloned().unwrap_or_else(|| match id.as_str() {
            "INBOX" => "Inbox".into(),
            "UNREAD" => "Unread".into(),
            "STARRED" => "Starred".into(),
            "IMPORTANT" => "Important".into(),
            "TRASH" => "Trash".into(),
            "SPAM" => "Spam".into(),
            other => other.to_string(),
        })
    };
    for f in &mut filters {
        f.add_labels = f.add_labels.iter().map(pretty).collect();
        f.remove_labels = f.remove_labels.iter().map(pretty).collect();
    }
    Ok(filters)
}

#[tauri::command]
pub async fn mail_search(
    state: State<'_, AppState>,
    account_id: i64,
    query: String,
) -> Result<Vec<MessageSummary>> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    state.mail.search_messages(account_id, query.trim(), 200)
}

#[tauri::command]
pub async fn mail_unread_count(state: State<'_, AppState>, account_id: i64) -> Result<i64> {
    state.mail.unread_count(account_id)
}

/// Returns the message with its body, fetching and caching it on first open.
/// Opening a message also marks it read, mirroring every other client.
#[tauri::command]
pub async fn mail_get_message(state: State<'_, AppState>, message_id: i64) -> Result<MessageDetail> {
    load_message(&state, message_id).await
}

async fn load_message(state: &AppState, message_id: i64) -> Result<MessageDetail> {
    let mut detail = state.mail.get_message(message_id)?;
    let account = state.mail.get_account(detail.summary.account_id)?;
    let provider = state.providers.provider_for(&account.provider)?;

    if !state.mail.body_fetched(message_id)? {
        let mut body = provider
            .fetch_body(&account, &detail.summary.remote_id)
            .await?;
        body.html = body.html.as_deref().map(sanitize::html);
        state.mail.set_body(message_id, &body)?;
        detail = state.mail.get_message(message_id)?;
    }

    if !detail.summary.is_read {
        let flags = FlagChange {
            read: Some(true),
            starred: None,
        };
        state.mail.apply_flags(message_id, flags)?;
        detail.summary.is_read = true;
        let remote_id = detail.summary.remote_id.clone();
        let provider_bg = provider.clone();
        let account_bg = account.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = provider_bg.set_flags(&account_bg, &remote_id, flags).await {
                tracing::warn!(%e, "mark read failed");
            }
        });
    }

    if let Some(text) = detail.body_text.as_deref() {
        if let Some(inv) = super::ics::parse(text) {
            detail.invite = Some(inv);
        }
    }
    if detail.invite.is_none() {
        for att in &detail.attachments {
            if att.mime_type.to_ascii_lowercase().contains("calendar")
                || att.filename.to_ascii_lowercase().ends_with(".ics")
            {
                if let Ok(bytes) = provider
                    .fetch_attachment(&account, &detail.summary.remote_id, &att.remote_id)
                    .await
                {
                    if let Ok(text) = String::from_utf8(bytes) {
                        if let Some(inv) = super::ics::parse(&text) {
                            detail.invite = Some(inv);
                            break;
                        }
                    }
                }
            }
        }
    }
    Ok(detail)
}

#[tauri::command]
pub async fn mail_set_flags(
    state: State<'_, AppState>,
    message_id: i64,
    flags: FlagChange,
) -> Result<()> {
    let detail = state.mail.get_message(message_id)?;
    let account = state.mail.get_account(detail.summary.account_id)?;
    let provider = state.providers.provider_for(&account.provider)?;
    state.mail.apply_flags(message_id, flags)?;
    provider
        .set_flags(&account, &detail.summary.remote_id, flags)
        .await
}

#[tauri::command]
pub async fn mail_trash(state: State<'_, AppState>, message_id: i64) -> Result<()> {
    let detail = state.mail.get_message(message_id)?;
    let account = state.mail.get_account(detail.summary.account_id)?;
    let provider = state.providers.provider_for(&account.provider)?;
    provider.trash(&account, &detail.summary.remote_id).await?;
    state.mail.set_labels(message_id, &["TRASH"], &["INBOX"])
}

#[tauri::command]
pub async fn mail_archive(state: State<'_, AppState>, message_id: i64) -> Result<()> {
    let detail = state.mail.get_message(message_id)?;
    let account = state.mail.get_account(detail.summary.account_id)?;
    let provider = state.providers.provider_for(&account.provider)?;
    provider.archive(&account, &detail.summary.remote_id).await?;
    state.mail.set_labels(message_id, &[], &["INBOX"])
}

#[tauri::command]
pub async fn mail_compose_draft(
    state: State<'_, AppState>,
    message_id: i64,
    mode: ReplyMode,
) -> Result<ComposeDraft> {
    // Body must be present to quote it.
    let detail = load_message(&state, message_id).await?;
    let account = state.mail.get_account(detail.summary.account_id)?;
    Ok(compose::draft_for(&account, &detail, mode))
}

async fn resolve_attachments(
    state: &AppState,
    account: &Account,
    provider: &std::sync::Arc<dyn super::MailProvider>,
    message: &OutgoingMessage,
) -> Result<Vec<ResolvedAttachment>> {
    let mut resolved = Vec::with_capacity(message.attachments.len());
    for att in &message.attachments {
        resolved.push(match att {
            OutgoingAttachment::Path { path } => {
                let p = PathBuf::from(path);
                let filename = p
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "attachment".into());
                let mime_type = mime_guess::from_path(&p).first_or_octet_stream().to_string();
                ResolvedAttachment {
                    filename,
                    mime_type,
                    data: tokio::fs::read(&p).await?,
                }
            }
            OutgoingAttachment::Stored { attachment_id, .. } => {
                let (msg_id, info) = state.mail.get_attachment(*attachment_id)?;
                let src = state.mail.get_message(msg_id)?;
                let data = provider
                    .fetch_attachment(&account, &src.summary.remote_id, &info.remote_id)
                    .await?;
                ResolvedAttachment {
                    filename: info.filename,
                    mime_type: info.mime_type,
                    data,
                }
            }
        });
    }
    Ok(resolved)
}

#[tauri::command]
pub async fn mail_send(state: State<'_, AppState>, mut message: OutgoingMessage) -> Result<()> {
    if message.to.iter().all(|t| t.trim().is_empty()) {
        return Err(AppError::Other("add at least one recipient".into()));
    }
    let sig = crate::settings::signature_for(&state.db, message.account_id)?;
    if !sig.trim().is_empty() {
        message.body_text = format!("{}\n\n-- \n{}", message.body_text.trim_end(), sig.trim());
    }
    let account = state.mail.get_account(message.account_id)?;
    let provider = state.providers.provider_for(&account.provider)?;
    let resolved = resolve_attachments(&state, &account, &provider, &message).await?;
    let raw = compose::build_raw(&account, &message, resolved)?;
    provider
        .send(&account, raw, message.thread_id.as_deref())
        .await?;
    if let Some(d) = &message.draft_id {
        if let Err(e) = provider.delete_draft(&account, d).await {
            tracing::warn!(%e, "could not delete draft after send");
        }
    }
    // Pull the sent copy into the local store so it shows up under Sent.
    spawn_sync(state.app.clone(), account.id);
    Ok(())
}

#[tauri::command]
pub async fn mail_save_draft(state: State<'_, AppState>, message: OutgoingMessage) -> Result<String> {
    let account = state.mail.get_account(message.account_id)?;
    let provider = state.providers.provider_for(&account.provider)?;
    let resolved = resolve_attachments(&state, &account, &provider, &message).await?;
    let raw = compose::build_raw_lenient(&account, &message, resolved)?;
    provider
        .save_draft(&account, raw, message.thread_id.as_deref(), message.draft_id.as_deref())
        .await
}

#[tauri::command]
pub async fn mail_discard_draft(state: State<'_, AppState>, account_id: i64, draft_id: String) -> Result<()> {
    let account = state.mail.get_account(account_id)?;
    let provider = state.providers.provider_for(&account.provider)?;
    provider.delete_draft(&account, &draft_id).await?;
    spawn_sync(state.app.clone(), account_id);
    Ok(())
}

#[tauri::command]
pub async fn mail_open_draft(state: State<'_, AppState>, message_id: i64) -> Result<DraftContent> {
    let detail = state.mail.get_message(message_id)?;
    let account = state.mail.get_account(detail.summary.account_id)?;
    let provider = state.providers.provider_for(&account.provider)?;
    provider.open_draft(&account, &detail.summary.remote_id).await
}

/// Downloads an attachment into the user's Downloads folder and returns the
/// path. With `open`, also hands it to the OS default handler.
#[tauri::command]
pub async fn mail_save_attachment(
    app: AppHandle,
    state: State<'_, AppState>,
    attachment_id: i64,
    open: bool,
) -> Result<String> {
    let (msg_id, info) = state.mail.get_attachment(attachment_id)?;
    let detail = state.mail.get_message(msg_id)?;
    let account = state.mail.get_account(detail.summary.account_id)?;
    let provider = state.providers.provider_for(&account.provider)?;
    let data = provider
        .fetch_attachment(&account, &detail.summary.remote_id, &info.remote_id)
        .await?;

    let dir = app
        .path()
        .download_dir()
        .map_err(|e| AppError::Other(format!("no downloads dir: {e}")))?
        .join("Trakzen Conecta");
    tokio::fs::create_dir_all(&dir).await?;
    let path = crate::util::unique_path(&dir, &sanitize_filename::sanitize(&info.filename));
    tokio::fs::write(&path, &data).await?;

    if open {
        app.opener()
            .open_path(path.to_string_lossy(), None::<&str>)
            .map_err(|e| AppError::Other(format!("could not open file: {e}")))?;
    }
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
pub async fn mail_schedule(state: State<'_, AppState>, message: OutgoingMessage, send_at: i64) -> Result<()> {
    if message.to.iter().all(|t| t.trim().is_empty()) {
        return Err(AppError::Other("add at least one recipient".into()));
    }
    let payload = serde_json::to_string(&message)?;
    state.mail.enqueue_outbox(message.account_id, &payload, send_at)?;
    Ok(())
}

#[tauri::command]
pub async fn mail_rsvp(state: State<'_, AppState>, message_id: i64, accept: bool) -> Result<()> {
    let detail = load_message(&state, message_id).await?;
    let Some(inv) = detail.invite.clone() else {
        return Err(AppError::Other("no calendar invite on this message".into()));
    };
    let Some(organizer) = inv.organizer.clone() else {
        return Err(AppError::Other("invite has no organizer to reply to".into()));
    };
    let account = state.mail.get_account(detail.summary.account_id)?;
    let mut original = detail.body_text.clone().unwrap_or_default();
    if !original.to_ascii_uppercase().contains("BEGIN:VEVENT") {
        for att in &detail.attachments {
            if att.mime_type.to_ascii_lowercase().contains("calendar") || att.filename.to_ascii_lowercase().ends_with(".ics")
            {
                let provider = state.providers.provider_for(&account.provider)?;
                if let Ok(bytes) = provider
                    .fetch_attachment(&account, &detail.summary.remote_id, &att.remote_id)
                    .await
                {
                    original = String::from_utf8_lossy(&bytes).into_owned();
                    break;
                }
            }
        }
    }
    let ics = super::ics::reply(&original, accept, &account.email);
    let dir = std::env::temp_dir();
    let path = dir.join(format!("conecta-rsvp-{}.ics", uuid::Uuid::new_v4()));
    tokio::fs::write(&path, ics).await?;
    let verb = if accept { "Accepted" } else { "Declined" };
    let message = OutgoingMessage {
        account_id: account.id,
        to: vec![organizer],
        cc: vec![],
        bcc: vec![],
        subject: format!("{verb}: {}", inv.summary),
        body_text: format!("{verb} the invitation \"{}\".", inv.summary),
        quoted_html: None,
        in_reply_to: detail.message_id_hdr.clone(),
        references: detail.references_hdr.clone(),
        thread_id: detail.summary.thread_id.clone(),
        attachments: vec![OutgoingAttachment::Path {
            path: path.to_string_lossy().into_owned(),
        }],
        draft_id: None,
    };
    mail_send(state, message).await
}

pub async fn flush_outbox(app: &AppHandle) {
    let due = {
        let state = app.state::<AppState>();
        state.mail.pop_due_outbox(crate::db::now_ms()).unwrap_or_default()
    };
    for (_id, payload) in due {
        match serde_json::from_str::<OutgoingMessage>(&payload) {
            Ok(message) => {
                let state = app.state::<AppState>();
                if let Err(e) = mail_send(state, message).await {
                    tracing::warn!(%e, "scheduled send failed");
                }
            }
            Err(e) => tracing::warn!(%e, "bad scheduled payload"),
        }
    }
}
