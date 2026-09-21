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
    state.chat.mark_read(peer_id).await
}

#[tauri::command]
pub async fn chat_send_text(
    state: State<'_, AppState>,
    peer_id: i64,
    body: String,
    reply_to: Option<String>,
) -> Result<ChatMessage> {
    state.chat.send_text(peer_id, &body, reply_to.as_deref()).await
}

#[tauri::command]
pub async fn chat_react(state: State<'_, AppState>, msg_id: String, emoji: String) -> Result<Option<ChatMessage>> {
    state.chat.react(&msg_id, &emoji).await
}

#[tauri::command]
pub async fn chat_edit(state: State<'_, AppState>, msg_id: String, body: String) -> Result<Option<ChatMessage>> {
    state.chat.edit(&msg_id, &body).await
}

#[tauri::command]
pub async fn chat_typing(state: State<'_, AppState>, peer_id: i64) -> Result<()> {
    state.chat.send_typing(peer_id).await;
    Ok(())
}

#[tauri::command]
pub async fn chat_search(state: State<'_, AppState>, peer_id: i64, query: String) -> Result<Vec<ChatMessage>> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    state.chat.store.search_messages(peer_id, query.trim(), 200)
}

#[tauri::command]
pub async fn chat_nearby(state: State<'_, AppState>) -> Result<Vec<super::discovery::Nearby>> {
    Ok(state.chat.nearby())
}

/// Adds a discovered peer using its advertised address; if a row with that
/// peer id already exists it just reconnects.
#[tauri::command]
pub async fn chat_add_nearby(state: State<'_, AppState>, peer_id: String) -> Result<Peer> {
    let n = state
        .chat
        .nearby()
        .into_iter()
        .find(|n| n.peer_id == peer_id)
        .ok_or_else(|| AppError::NotFound("that peer is no longer visible".into()))?;
    let host = n.addresses.first().cloned().unwrap_or_default();
    let row = state
        .chat
        .store
        .bind_peer(None, &n.peer_id, &n.display_name, &host, n.port)?;
    let engine = state.chat.clone();
    tauri::async_runtime::spawn(async move {
        let _ = engine.connect_peer(row).await;
    });
    let mut p = state.chat.store.get_peer(row)?;
    p.online = state.chat.is_online(row);
    Ok(p)
}

/// SVG QR code of a pairing link for this machine.
#[tauri::command]
pub async fn chat_pairing_qr(state: State<'_, AppState>) -> Result<(String, String)> {
    let id = state.chat.identity();
    let host = id.addresses.first().cloned().unwrap_or_else(|| "127.0.0.1".into());
    let link = format!(
        "conecta://pair?host={}&port={}&id={}&name={}",
        host,
        id.port,
        id.peer_id,
        urlencoding_encode(&id.display_name)
    );
    let code = qrcode::QrCode::new(link.as_bytes()).map_err(|e| AppError::Other(e.to_string()))?;
    let svg = code
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(220, 220)
        .quiet_zone(true)
        .build();
    Ok((link, svg))
}

fn urlencoding_encode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
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

#[tauri::command]
pub async fn chat_delete_message(state: State<'_, AppState>, msg_id: String, for_everyone: bool) -> Result<()> {
    state.chat.delete_message(&msg_id, for_everyone).await
}

#[tauri::command]
pub async fn chat_clear_chat(state: State<'_, AppState>, peer_id: i64) -> Result<()> {
    state.chat.clear_chat(peer_id).await
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageStats {
    pub received_dir: String,
    pub received_files: u64,
    pub received_bytes: u64,
    pub outgoing_dir: String,
    pub outgoing_files: u64,
    pub outgoing_bytes: u64,
}

fn dir_stats(dir: &std::path::Path) -> (u64, u64) {
    let mut files = 0;
    let mut bytes = 0;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if let Ok(md) = e.metadata() {
                if md.is_file() {
                    files += 1;
                    bytes += md.len();
                }
            }
        }
    }
    (files, bytes)
}

#[tauri::command]
pub async fn chat_storage_stats(state: State<'_, AppState>) -> Result<StorageStats> {
    let received = state.chat.download_dir()?;
    let outgoing = state.chat.outgoing_dir()?;
    let (rf, rb) = dir_stats(&received);
    let (of, ob) = dir_stats(&outgoing);
    Ok(StorageStats {
        received_dir: received.to_string_lossy().into_owned(),
        received_files: rf,
        received_bytes: rb,
        outgoing_dir: outgoing.to_string_lossy().into_owned(),
        outgoing_files: of,
        outgoing_bytes: ob,
    })
}

/// Deletes every file in the received or pasted-media folder and forgets
/// the paths on the affected messages. `which` is "received" or "outgoing".
#[tauri::command]
pub async fn chat_clear_storage(state: State<'_, AppState>, which: String) -> Result<u64> {
    let dir = match which.as_str() {
        "received" => state.chat.download_dir()?,
        "outgoing" => state.chat.outgoing_dir()?,
        _ => return Err(AppError::Other("unknown storage area".into())),
    };
    let mut removed = 0;
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            if e.metadata().map(|m| m.is_file()).unwrap_or(false) && std::fs::remove_file(e.path()).is_ok() {
                removed += 1;
            }
        }
    }
    state.chat.store.forget_missing_files()?;
    Ok(removed)
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

#[tauri::command]
pub async fn chat_pause_transfer(state: State<'_, AppState>, transfer_id: String, pause: bool) -> Result<()> {
    state.chat.pause_transfer(&transfer_id, pause);
    Ok(())
}

/// Accept or decline a file a peer is offering (message status `offered`).
/// `always` also switches the sender to auto-accept from now on.
#[tauri::command]
pub async fn chat_answer_file(state: State<'_, AppState>, msg_id: String, accept: bool, always: Option<bool>) -> Result<()> {
    state.chat.answer_offer(&msg_id, accept, always.unwrap_or(false)).await
}

#[tauri::command]
pub async fn chat_set_auto_accept(state: State<'_, AppState>, peer_id: i64, on: bool) -> Result<Peer> {
    state.chat.store.set_auto_accept(peer_id, on)?;
    let mut p = state.chat.store.get_peer(peer_id)?;
    p.online = state.chat.is_online(peer_id);
    Ok(p)
}

#[tauri::command]
pub async fn chat_pin(state: State<'_, AppState>, msg_id: String, pinned: bool) -> Result<Option<ChatMessage>> {
    state.chat.pin_message(&msg_id, pinned).await
}

#[tauri::command]
pub async fn chat_create_group(state: State<'_, AppState>, name: String, member_ids: Vec<i64>) -> Result<Peer> {
    state.chat.create_group(&name, member_ids).await
}

#[tauri::command]
pub async fn chat_group_members(state: State<'_, AppState>, group_id: i64) -> Result<Vec<Peer>> {
    state.chat.group_members(group_id)
}

#[tauri::command]
pub async fn chat_group_add_members(state: State<'_, AppState>, group_id: i64, member_ids: Vec<i64>) -> Result<Peer> {
    state.chat.add_members(group_id, member_ids).await
}

#[tauri::command]
pub async fn chat_group_remove_member(state: State<'_, AppState>, group_id: i64, member_id: i64) -> Result<Peer> {
    state.chat.remove_member(group_id, member_id).await
}

#[tauri::command]
pub async fn chat_group_rename(state: State<'_, AppState>, group_id: i64, name: String) -> Result<Peer> {
    state.chat.rename_group(group_id, &name).await
}

#[tauri::command]
pub async fn chat_group_leave(state: State<'_, AppState>, group_id: i64) -> Result<Peer> {
    state.chat.leave_group(group_id).await
}

fn urlencoding_decode(s: &str) -> String {
    url::form_urlencoded::parse(format!("v={s}").as_bytes())
        .find(|(k, _)| k == "v")
        .map(|(_, v)| v.into_owned())
        .unwrap_or_else(|| s.to_string())
}
