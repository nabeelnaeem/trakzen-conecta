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
pub const CHAT_ASK_FILES: &str = "chat_ask_files";
pub const MAIL_SHOW_IMAGES: &str = "mail_show_images";
pub const MAIL_SIGNATURE: &str = "mail_signature";

/// Per-account override of [`MAIL_SIGNATURE`]; empty string means "none".
pub fn account_signature_key(account_id: i64) -> String {
    format!("mail_signature:{account_id}")
}

/// The signature to append for an account: its own if one is set,
/// otherwise the global one.
pub fn signature_for(db: &Db, account_id: i64) -> Result<String> {
    if let Some(s) = get(db, &account_signature_key(account_id))? {
        return Ok(s);
    }
    Ok(get(db, MAIL_SIGNATURE)?.unwrap_or_default())
}
pub const MAIL_POLL_SECONDS: &str = "mail_poll_seconds";
pub const CLOSE_TO_TRAY: &str = "close_to_tray";
pub const NOTIFICATIONS: &str = "notifications";
pub const NOTIFICATION_SOUND: &str = "notification_sound";
pub const SOUND_MAIL: &str = "sound_mail";
pub const SOUND_CHAT: &str = "sound_chat";
pub const CONVERSATION_VIEW: &str = "conversation_view";
pub const UNDO_SEND_SECONDS: &str = "undo_send_seconds";
pub const MAIL_TEMPLATES: &str = "mail_templates";

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
    /// Ask before an incoming file is written to disk.
    pub chat_ask_files: bool,
    pub mail_show_images: bool,
    pub mail_signature: String,
    /// account id → signature override (absent = use the global one).
    pub account_signatures: std::collections::HashMap<i64, String>,
    /// 0 disables background polling.
    pub mail_poll_seconds: u64,
    pub close_to_tray: bool,
    pub notifications: bool,
    pub notification_sound: bool,
    pub sound_mail: String,
    pub sound_chat: String,
    pub conversation_view: bool,
    pub undo_send_seconds: u64,
    pub mail_templates: String,
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
        chat_ask_files: flag(db, CHAT_ASK_FILES, true)?,
        // Off by default: loading remote images tells senders you opened the mail.
        mail_show_images: get(db, MAIL_SHOW_IMAGES)?.map_or(false, |v| v == "true"),
        mail_signature: get(db, MAIL_SIGNATURE)?.unwrap_or_default(),
        account_signatures: {
            let conn = db.conn();
            let mut stmt = conn.prepare("SELECT key, value FROM settings WHERE key LIKE 'mail_signature:%'")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            rows.filter_map(|r| r.ok())
                .filter_map(|(k, v)| k.rsplit(':').next()?.parse::<i64>().ok().map(|id| (id, v)))
                .collect()
        },
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
        mail_templates: get(db, MAIL_TEMPLATES)?.unwrap_or_else(|| "[]".into()),
    })
}

pub fn flag(db: &Db, key: &str, default: bool) -> Result<bool> {
    Ok(get(db, key)?.map_or(default, |v| v == "true"))
}

pub fn poll_seconds(db: &Db) -> Result<u64> {
    Ok(get(db, MAIL_POLL_SECONDS)?
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_MAIL_POLL_SECONDS))
}
