mod chat;
mod db;
mod error;
mod mail;
mod secrets;
mod settings;
mod util;

use std::sync::Arc;

use serde::Deserialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};

use chat::ChatEngine;
use db::Db;
use error::Result;
use mail::store::MailStore;
use mail::Providers;

pub struct AppState {
    pub app: AppHandle,
    pub db: Arc<Db>,
    pub mail: MailStore,
    pub providers: Providers,
    pub chat: Arc<ChatEngine>,
    pub sync_guard: mail::commands::SyncGuard,
    pub page_tokens: mail::commands::PageTokens,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    google_client_id: Option<String>,
    google_client_secret: Option<String>,
    chat_display_name: Option<String>,
    chat_port: Option<u16>,
    chat_download_dir: Option<String>,
    mail_show_images: Option<bool>,
    mail_signature: Option<String>,
    /// account id → signature; `None` value removes the override.
    account_signatures: Option<std::collections::HashMap<i64, Option<String>>>,
    mail_poll_seconds: Option<u64>,
    close_to_tray: Option<bool>,
    notifications: Option<bool>,
    notification_sound: Option<bool>,
    sound_mail: Option<String>,
    sound_chat: Option<String>,
    conversation_view: Option<bool>,
    undo_send_seconds: Option<u64>,
}

#[tauri::command]
async fn settings_get(state: State<'_, AppState>) -> Result<settings::SettingsView> {
    settings::view(&state.db)
}

#[tauri::command]
async fn settings_update(
    state: State<'_, AppState>,
    patch: SettingsPatch,
) -> Result<settings::SettingsView> {
    if let Some(v) = patch.google_client_id {
        settings::set(&state.db, settings::GOOGLE_CLIENT_ID, v.trim())?;
    }
    if let Some(v) = patch.google_client_secret {
        if v.trim().is_empty() {
            secrets::delete(secrets::GOOGLE_CLIENT_SECRET)?;
        } else {
            secrets::set(secrets::GOOGLE_CLIENT_SECRET, v.trim())?;
        }
    }
    if let Some(v) = patch.chat_display_name {
        state.chat.set_display_name(&v)?;
    }
    if let Some(v) = patch.chat_port {
        // Takes effect on next launch; rebinding a live listener is not
        // worth the complexity yet.
        settings::set(&state.db, settings::CHAT_PORT, &v.to_string())?;
    }
    if let Some(v) = patch.chat_download_dir {
        settings::set(&state.db, settings::CHAT_DOWNLOAD_DIR, v.trim())?;
    }
    if let Some(v) = patch.mail_show_images {
        settings::set(&state.db, settings::MAIL_SHOW_IMAGES, if v { "true" } else { "false" })?;
    }
    if let Some(v) = patch.mail_signature {
        settings::set(&state.db, settings::MAIL_SIGNATURE, v.trim_end())?;
    }
    if let Some(map) = patch.account_signatures {
        for (id, sig) in map {
            let key = settings::account_signature_key(id);
            match sig {
                Some(s) => settings::set(&state.db, &key, s.trim_end())?,
                None => {
                    state.db.conn().execute("DELETE FROM settings WHERE key = ?1", rusqlite::params![key])?;
                }
            }
        }
    }
    if let Some(v) = patch.mail_poll_seconds {
        // Picked up by the poll loop on its next tick; 0 pauses it.
        settings::set(&state.db, settings::MAIL_POLL_SECONDS, &v.to_string())?;
    }
    let flag = |b: bool| if b { "true" } else { "false" };
    if let Some(v) = patch.close_to_tray {
        settings::set(&state.db, settings::CLOSE_TO_TRAY, flag(v))?;
    }
    if let Some(v) = patch.notifications {
        settings::set(&state.db, settings::NOTIFICATIONS, flag(v))?;
    }
    if let Some(v) = patch.notification_sound {
        settings::set(&state.db, settings::NOTIFICATION_SOUND, flag(v))?;
    }
    if let Some(v) = patch.sound_mail {
        settings::set(&state.db, settings::SOUND_MAIL, v.trim())?;
    }
    if let Some(v) = patch.sound_chat {
        settings::set(&state.db, settings::SOUND_CHAT, v.trim())?;
    }
    if let Some(v) = patch.conversation_view {
        settings::set(&state.db, settings::CONVERSATION_VIEW, flag(v))?;
    }
    if let Some(v) = patch.undo_send_seconds {
        settings::set(&state.db, settings::UNDO_SEND_SECONDS, &v.min(60).to_string())?;
    }
    settings::view(&state.db)
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

#[tauri::command]
async fn app_quit(app: AppHandle) {
    app.exit(0);
}

/// Unread total shown on the tray tooltip and window title.
#[tauri::command]
async fn app_set_badge(app: AppHandle, count: u32) {
    let suffix = if count > 0 { format!(" ({count})") } else { String::new() };
    if let Some(t) = app.tray_by_id("main") {
        let _ = t.set_tooltip(Some(format!("Trakzen Conecta{suffix}")));
    }
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.set_title(&format!("Trakzen Conecta{suffix}"));
        let _ = w.set_badge_count(if count > 0 { Some(count as i64) } else { None });
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,trakzen_conecta_lib=debug".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .on_window_event(|window, event| {
            // Closing the main window hides it to the tray unless the user
            // turned that off; the tray menu has an explicit Quit.
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() != "main" {
                    return;
                }
                let state = window.state::<AppState>();
                let to_tray = settings::get(&state.db, settings::CLOSE_TO_TRAY)
                    .ok()
                    .flatten()
                    .map_or(true, |v| v == "true");
                if to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .setup(|app| {
            let show = MenuItem::with_id(app, "show", "Open Trakzen Conecta", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().cloned().expect("bundled icon"))
                .tooltip("Trakzen Conecta")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show_main(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main(tray.app_handle());
                    }
                })
                .build(app)?;

            // Override lets a second instance run against its own database
            // (handy for testing chat on a single machine).
            let data_dir = match std::env::var("TRAKZEN_CONECTA_DATA_DIR") {
                Ok(dir) if !dir.trim().is_empty() => std::path::PathBuf::from(dir),
                _ => app.path().app_data_dir()?,
            };
            let db = Arc::new(Db::open(&data_dir)?);
            tracing::info!(path = %data_dir.join(db::DB_FILE_NAME).display(), "database ready");
            if let Err(e) = settings::migrate_secret_to_keyring(&db) {
                tracing::warn!(%e, "could not move client secret to the credential store");
            }

            let chat = ChatEngine::new(app.handle().clone(), db.clone())?;
            let state = AppState {
                app: app.handle().clone(),
                mail: MailStore::new(db.clone()),
                providers: Providers {
                    gmail: Arc::new(mail::gmail::GmailProvider::new(db.clone())),
                },
                chat: chat.clone(),
                sync_guard: Default::default(),
                page_tokens: Default::default(),
                db,
            };
            app.manage(state);
            chat.start();

            // Refresh every account in the background at launch; the UI
            // renders from the local cache immediately.
            let handle = app.handle().clone();
            match handle.state::<AppState>().mail.backfill_contacts() {
                Ok(n) if n > 0 => tracing::info!(messages = n, "built contacts from cached mail"),
                Ok(_) => {}
                Err(e) => tracing::warn!(%e, "contact backfill failed"),
            }
            if let Ok(n) = handle.state::<AppState>().mail.reclean_snippets() {
                if n > 0 {
                    tracing::info!(rows = n, "re-cleaned cached snippets");
                }
            }
            let accounts = handle.state::<AppState>().mail.list_accounts()?;
            for a in accounts {
                mail::commands::spawn_sync(handle.clone(), a.id);
            }

            // Snoozed mail comes back on a timer; the UI refreshes on the event.
            let snoozer = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                    let due = snoozer
                        .state::<AppState>()
                        .mail
                        .pop_due_snoozes(db::now_ms())
                        .unwrap_or_default();
                    if !due.is_empty() {
                        let _ = snoozer.emit("mail://unsnoozed", &due);
                    }
                }
            });

            // Gmail has no push for installed apps short of Pub/Sub, so poll
            // the history API. Each tick is one small request per account.
            let poller = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    let secs = {
                        let state = poller.state::<AppState>();
                        settings::poll_seconds(&state.db).unwrap_or(settings::DEFAULT_MAIL_POLL_SECONDS)
                    };
                    let wait = if secs == 0 { 30 } else { secs.max(15) };
                    tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                    if secs == 0 {
                        continue;
                    }
                    let accounts = poller
                        .state::<AppState>()
                        .mail
                        .list_accounts()
                        .unwrap_or_default();
                    for a in accounts {
                        mail::commands::spawn_sync(poller.clone(), a.id);
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            settings_get,
            settings_update,
            app_quit,
            mail::commands::mail_list_thread,
            mail::commands::mail_bulk_modify,
            mail::commands::mail_thread_modify,
            mail::commands::mail_mark_view,
            mail::commands::mail_view_count,
            mail::commands::mail_modify_view,
            mail::commands::mail_threads_modify,
            mail::commands::mail_create_label,
            mail::commands::mail_create_filter,
            mail::commands::mail_delete_filter,
            mail::commands::mail_update_filter,
            mail::commands::mail_reauth,
            mail::commands::mail_search_server,
            mail::commands::mail_unsubscribe,
            mail::commands::mail_snooze,
            mail::commands::mail_unsnooze,
            mail::commands::mail_list_snoozed,
            mail::commands::mail_save_draft,
            mail::commands::mail_discard_draft,
            mail::commands::mail_open_draft,
            mail::commands::mail_list_accounts,
            mail::commands::mail_add_account,
            mail::commands::mail_remove_account,
            mail::commands::mail_sync,
            mail::commands::mail_list_messages,
            mail::commands::mail_search,
            mail::commands::mail_unread_count,
            mail::commands::mail_get_message,
            mail::commands::mail_set_flags,
            mail::commands::mail_trash,
            mail::commands::mail_archive,
            mail::commands::mail_compose_draft,
            mail::commands::mail_send,
            mail::commands::mail_save_attachment,
            mail::commands::mail_list_labels,
            mail::commands::mail_modify_labels,
            mail::commands::mail_fetch_more,
            mail::commands::mail_list_filters,
            mail::commands::mail_suggest_contacts,
            chat::commands::chat_identity,
            chat::commands::chat_set_display_name,
            chat::commands::chat_list_peers,
            chat::commands::chat_add_peer,
            chat::commands::chat_update_peer,
            chat::commands::chat_remove_peer,
            chat::commands::chat_connect_peer,
            chat::commands::chat_list_messages,
            chat::commands::chat_mark_read,
            chat::commands::chat_send_text,
            chat::commands::chat_send_file,
            chat::commands::chat_open_file,
            chat::commands::chat_stash_blob,
            chat::commands::chat_file_preview,
            chat::commands::chat_delete_message,
            chat::commands::chat_clear_chat,
            chat::commands::chat_storage_stats,
            chat::commands::chat_clear_storage,
            chat::commands::chat_typing,
            chat::commands::chat_react,
            chat::commands::chat_edit,
            chat::commands::chat_search,
            chat::commands::chat_nearby,
            chat::commands::chat_add_nearby,
            chat::commands::chat_pairing_qr,
            app_set_badge,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
