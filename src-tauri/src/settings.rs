use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::db::Db;
use crate::error::Result;

pub const GOOGLE_CLIENT_ID: &str = "google_client_id";
pub const GOOGLE_CLIENT_SECRET: &str = "google_client_secret";
pub const CHAT_DISPLAY_NAME: &str = "chat_display_name";
pub const CHAT_PEER_ID: &str = "chat_peer_id";
pub const CHAT_PORT: &str = "chat_port";
pub const CHAT_DOWNLOAD_DIR: &str = "chat_download_dir";
pub const MAIL_SHOW_IMAGES: &str = "mail_show_images";
pub const MAIL_SIGNATURE: &str = "mail_signature";
pub const MAIL_POLL_SECONDS: &str = "mail_poll_seconds";
pub const CLOSE_TO_TRAY: &str = "close_to_tray";
pub const NOTIFICATIONS: &str = "notifications";
pub const NOTIFICATION_SOUND: &str = "notification_sound";
pub const SOUND_MAIL: &str = "sound_mail";
pub const SOUND_CHAT: &str = "sound_chat";
pub const CONVERSATION_VIEW: &str = "conversation_view";
pub const UNDO_SEND_SECONDS: &str = "undo_send_seconds";

pub const DEFAULT_MAIL_POLL_SECONDS: u64 = 60;

pub const DEFAULT_CHAT_PORT: u16 = 47800;

pub fn get(db: &Db, key: &str) -> Result<Option<String>> {
    let conn = db.conn();
    Ok(conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |r| r.get(0),
        )
        .optional()?)
}

pub fn set(db: &Db, key: &str, value: &str) -> Result<()> {
    db.conn().execute(
        "INSERT INTO settings(key, value) VALUES(?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// Older builds kept the OAuth client secret in the settings table; move it
/// into the OS credential store alongside the tokens. Safe to run every
/// start — it is a no-op once the row is gone.
pub fn migrate_secret_to_keyring(db: &Db) -> Result<()> {
    if let Some(v) = get(db, GOOGLE_CLIENT_SECRET)? {
        if !v.trim().is_empty() {
            crate::secrets::set(crate::secrets::GOOGLE_CLIENT_SECRET, v.trim())?;
        }
        db.conn()
            .execute("DELETE FROM settings WHERE key = ?1", rusqlite::params![GOOGLE_CLIENT_SECRET])?;
        tracing::info!("moved Google client secret into the credential store");
    }
    Ok(())
}

pub fn get_or_init(db: &Db, key: &str, init: impl FnOnce() -> String) -> Result<String> {
    if let Some(v) = get(db, key)? {
        return Ok(v);
    }
    let v = init();
    set(db, key, &v)?;
    Ok(v)
}

/// Everything the settings screen needs in one round-trip. Secrets are
/// reported as "set / not set" only.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub google_client_id: String,
    pub google_client_secret_set: bool,
    pub chat_display_name: String,
    pub chat_port: u16,
    pub chat_download_dir: String,
    pub mail_show_images: bool,
    pub mail_signature: String,
    /// 0 disables background polling.
    pub mail_poll_seconds: u64,
    pub close_to_tray: bool,
    pub notifications: bool,
    pub notification_sound: bool,
    pub sound_mail: String,
    pub sound_chat: String,
    pub conversation_view: bool,
    pub undo_send_seconds: u64,
}

pub fn view(db: &Db) -> Result<SettingsView> {
    Ok(SettingsView {
        google_client_id: get(db, GOOGLE_CLIENT_ID)?.unwrap_or_default(),
        google_client_secret_set: crate::secrets::get(crate::secrets::GOOGLE_CLIENT_SECRET)?
            .map(|s| !s.is_empty())
            .unwrap_or(false),
        chat_display_name: get(db, CHAT_DISPLAY_NAME)?.unwrap_or_default(),
        chat_port: get(db, CHAT_PORT)?
            .and_then(|p| p.parse().ok())
            .unwrap_or(DEFAULT_CHAT_PORT),
        chat_download_dir: get(db, CHAT_DOWNLOAD_DIR)?.unwrap_or_default(),
        // Off by default: loading remote images tells senders you opened the mail.
        mail_show_images: get(db, MAIL_SHOW_IMAGES)?.map_or(false, |v| v == "true"),
        mail_signature: get(db, MAIL_SIGNATURE)?.unwrap_or_default(),
        mail_poll_seconds: poll_seconds(db)?,
        close_to_tray: flag(db, CLOSE_TO_TRAY, true)?,
        notifications: flag(db, NOTIFICATIONS, true)?,
        notification_sound: flag(db, NOTIFICATION_SOUND, true)?,
        sound_mail: get(db, SOUND_MAIL)?.unwrap_or_else(|| "chime".into()),
        sound_chat: get(db, SOUND_CHAT)?.unwrap_or_else(|| "pop".into()),
        conversation_view: flag(db, CONVERSATION_VIEW, true)?,
        undo_send_seconds: get(db, UNDO_SEND_SECONDS)?
            .and_then(|v| v.parse().ok())
            .unwrap_or(10),
    })
}

fn flag(db: &Db, key: &str, default: bool) -> Result<bool> {
    Ok(get(db, key)?.map_or(default, |v| v == "true"))
}

pub fn poll_seconds(db: &Db) -> Result<u64> {
    Ok(get(db, MAIL_POLL_SECONDS)?
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_MAIL_POLL_SECONDS))
}
