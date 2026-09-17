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
}

pub fn view(db: &Db) -> Result<SettingsView> {
    Ok(SettingsView {
        google_client_id: get(db, GOOGLE_CLIENT_ID)?.unwrap_or_default(),
        google_client_secret_set: get(db, GOOGLE_CLIENT_SECRET)?
            .map(|s| !s.is_empty())
            .unwrap_or(false),
        chat_display_name: get(db, CHAT_DISPLAY_NAME)?.unwrap_or_default(),
        chat_port: get(db, CHAT_PORT)?
            .and_then(|p| p.parse().ok())
            .unwrap_or(DEFAULT_CHAT_PORT),
        chat_download_dir: get(db, CHAT_DOWNLOAD_DIR)?.unwrap_or_default(),
    })
}
