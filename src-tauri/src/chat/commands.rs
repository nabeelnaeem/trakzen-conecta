use tauri::State;
use tauri_plugin_opener::OpenerExt;

use crate::error::{AppError, Result};
use crate::AppState;

use super::types::*;

#[tauri::command]
pub async fn chat_identity(state: State<'_, AppState>) -> Result<Identity> {
    Ok(state.chat.identity())
}

#[tauri::command]
pub async fn chat_set_display_name(state: State<'_, AppState>, name: String) -> Result<Identity> {
    state.chat.set_display_name(&name)
}

#[tauri::command]
pub async fn chat_list_peers(state: State<'_, AppState>) -> Result<Vec<Peer>> {
    state.chat.list_peers()
}

#[tauri::command]
pub async fn chat_add_peer(
    state: State<'_, AppState>,
    display_name: String,
    host: String,
    port: Option<u16>,
) -> Result<Peer> {
    let host = host.trim();
    if host.is_empty() {
        return Err(AppError::Other("address is required".into()));
    }
    let name = if display_name.trim().is_empty() {
        host.to_string()
    } else {
        display_name.trim().to_string()
    };
    let port = port.unwrap_or(crate::settings::DEFAULT_CHAT_PORT);
    let peer = state.chat.store.add_peer(&name, host, port)?;
    // Try to reach it straight away so the online dot is accurate.
    let engine = state.chat.clone();
    let id = peer.id;
    tauri::async_runtime::spawn(async move {
        let _ = engine.connect_peer(id).await;
    });
    Ok(peer)
}

#[tauri::command]
pub async fn chat_update_peer(
    state: State<'_, AppState>,
    peer_id: i64,
    display_name: String,
    host: String,
    port: u16,
) -> Result<Peer> {
    state
        .chat
        .store
        .update_peer(peer_id, display_name.trim(), host.trim(), port)?;
    let mut p = state.chat.store.get_peer(peer_id)?;
    p.online = state.chat.is_online(peer_id);
    Ok(p)
}

#[tauri::command]
pub async fn chat_remove_peer(state: State<'_, AppState>, peer_id: i64) -> Result<()> {
    state.chat.store.remove_peer(peer_id)
}

#[tauri::command]
pub async fn chat_connect_peer(state: State<'_, AppState>, peer_id: i64) -> Result<bool> {
    Ok(state.chat.connect_peer(peer_id).await.is_ok())
}

#[tauri::command]
pub async fn chat_list_messages(
    state: State<'_, AppState>,
    peer_id: i64,
    limit: Option<i64>,
    before_id: Option<i64>,
) -> Result<Vec<ChatMessage>> {
    state
        .chat
        .store
        .list_messages(peer_id, limit.unwrap_or(100).clamp(1, 500), before_id)
}

#[tauri::command]
pub async fn chat_mark_read(state: State<'_, AppState>, peer_id: i64) -> Result<()> {
    state.chat.store.mark_read(peer_id)
}

#[tauri::command]
pub async fn chat_send_text(
    state: State<'_, AppState>,
    peer_id: i64,
    body: String,
) -> Result<ChatMessage> {
    state.chat.send_text(peer_id, &body).await
}

#[tauri::command]
pub async fn chat_send_file(
    state: State<'_, AppState>,
    peer_id: i64,
    path: String,
) -> Result<ChatMessage> {
    state.chat.send_file(peer_id, path).await
}

#[tauri::command]
pub async fn chat_open_file(app: tauri::AppHandle, path: String, reveal: bool) -> Result<()> {
    let res = if reveal {
        app.opener().reveal_item_in_dir(&path)
    } else {
        app.opener().open_path(&path, None::<&str>)
    };
    res.map_err(|e| AppError::Other(format!("could not open: {e}")))
}

/// Receives raw bytes from the webview (pasted screenshots, dropped blobs)
/// and stashes them as a file so they can go through the normal transfer.
#[tauri::command]
pub async fn chat_stash_blob(app: tauri::AppHandle, request: tauri::ipc::Request<'_>) -> Result<String> {
    use tauri::Manager;
    let name = request
        .headers()
        .get("x-file-name")
        .and_then(|v| v.to_str().ok())
        .map(|s| urlencoding_decode(s))
        .unwrap_or_else(|| "pasted.png".into());
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err(AppError::Other("expected a binary body".into()));
    };
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| AppError::Other(format!("no cache dir: {e}")))?
        .join("outgoing");
    tokio::fs::create_dir_all(&dir).await?;
    let safe = sanitize_filename::sanitize(&name);
    let path = crate::util::unique_path(&dir, if safe.is_empty() { "file" } else { &safe });
    tokio::fs::write(&path, bytes).await?;
    Ok(path.to_string_lossy().into_owned())
}

/// Small images come back as a data URL for inline previews; anything else
/// (or anything large) returns None and the UI shows a file card.
#[tauri::command]
pub async fn chat_file_preview(path: String) -> Result<Option<String>> {
    use base64::Engine;
    const MAX: u64 = 8 * 1024 * 1024;
    let mime = mime_guess::from_path(&path).first_or_octet_stream();
    if mime.type_() != mime_guess::mime::IMAGE {
        return Ok(None);
    }
    let meta = match tokio::fs::metadata(&path).await {
        Ok(m) => m,
        Err(_) => return Ok(None),
    };
    if meta.len() > MAX {
        return Ok(None);
    }
    let bytes = tokio::fs::read(&path).await?;
    Ok(Some(format!(
        "data:{};base64,{}",
        mime,
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )))
}

fn urlencoding_decode(s: &str) -> String {
    url::form_urlencoded::parse(format!("v={s}").as_bytes())
        .find(|(k, _)| k == "v")
        .map(|(_, v)| v.into_owned())
        .unwrap_or_else(|| s.to_string())
}
