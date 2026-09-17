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

/// Server paging position per (account, label). Absent = not started,
/// `None` = exhausted.
#[derive(Default)]
pub struct PageTokens(Mutex<HashMap<(i64, String), Option<String>>>);

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
    spawn_sync(app, account_id);
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
) -> Result<Vec<MessageSummary>> {
    state.mail.list_messages(
        account_id,
        &query,
        limit.unwrap_or(100).clamp(1, 1000),
        offset.unwrap_or(0).max(0),
    )
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
            Some(None) => {
                return Ok(FetchResult {
                    added: 0,
                    has_more: false,
                })
            }
            Some(Some(t)) => Some(t.clone()),
            None => None,
        }
    };

    let account = state.mail.get_account(account_id)?;
    let provider = state.providers.provider_for(&account.provider)?;
    let (added, next) = provider
        .fetch_label_page(&account, &state.mail, &label_id, token.as_deref())
        .await?;
    let has_more = next.is_some();
    state.page_tokens.0.lock().unwrap().insert(key, next);
    Ok(FetchResult { added, has_more })
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
        tauri::async_runtime::spawn(async move {
            if let Err(e) = provider.set_flags(&account, &remote_id, flags).await {
                tracing::warn!(%e, "mark read failed");
            }
        });
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

#[tauri::command]
pub async fn mail_send(state: State<'_, AppState>, mut message: OutgoingMessage) -> Result<()> {
    if message.to.iter().all(|t| t.trim().is_empty()) {
        return Err(AppError::Other("add at least one recipient".into()));
    }
    if let Some(sig) = crate::settings::get(&state.db, crate::settings::MAIL_SIGNATURE)? {
        if !sig.trim().is_empty() {
            message.body_text = format!("{}\n\n-- \n{}", message.body_text.trim_end(), sig.trim());
        }
    }
    let account = state.mail.get_account(message.account_id)?;
    let provider = state.providers.provider_for(&account.provider)?;

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

    let raw = compose::build_raw(&account, &message, resolved)?;
    provider
        .send(&account, raw, message.thread_id.as_deref())
        .await?;
    // Pull the sent copy into the local store so it shows up under Sent.
    spawn_sync(state.app.clone(), account.id);
    Ok(())
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
