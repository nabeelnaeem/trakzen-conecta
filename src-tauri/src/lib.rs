mod chat;
mod db;
mod error;
mod mail;
mod secrets;
mod settings;
mod util;

use std::sync::Arc;

use serde::Deserialize;
use tauri::{AppHandle, Manager, State};

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
    mail_poll_seconds: Option<u64>,
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
        settings::set(&state.db, settings::GOOGLE_CLIENT_SECRET, v.trim())?;
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
    if let Some(v) = patch.mail_poll_seconds {
        // Picked up by the poll loop on its next tick; 0 pauses it.
        settings::set(&state.db, settings::MAIL_POLL_SECONDS, &v.to_string())?;
    }
    settings::view(&state.db)
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
        .setup(|app| {
            // Override lets a second instance run against its own database
            // (handy for testing chat on a single machine).
            let data_dir = match std::env::var("TRAKZEN_CONECTA_DATA_DIR") {
                Ok(dir) if !dir.trim().is_empty() => std::path::PathBuf::from(dir),
                _ => app.path().app_data_dir()?,
            };
            let db = Arc::new(Db::open(&data_dir)?);
            tracing::info!(path = %data_dir.join(db::DB_FILE_NAME).display(), "database ready");

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
            let accounts = handle.state::<AppState>().mail.list_accounts()?;
            for a in accounts {
                mail::commands::spawn_sync(handle.clone(), a.id);
            }

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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
