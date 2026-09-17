use rusqlite::Connection;

use crate::error::Result;

const MIGRATIONS: &[&str] = &[
    // 1: initial schema
    "
    CREATE TABLE settings (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );

    CREATE TABLE mail_accounts (
        id           INTEGER PRIMARY KEY,
        provider     TEXT    NOT NULL,
        email        TEXT    NOT NULL UNIQUE,
        display_name TEXT,
        sync_cursor  TEXT,
        created_at   INTEGER NOT NULL
    );

    CREATE TABLE mail_messages (
        id              INTEGER PRIMARY KEY,
        account_id      INTEGER NOT NULL REFERENCES mail_accounts(id) ON DELETE CASCADE,
        remote_id       TEXT    NOT NULL,
        thread_id       TEXT,
        subject         TEXT    NOT NULL DEFAULT '',
        from_name       TEXT    NOT NULL DEFAULT '',
        from_addr       TEXT    NOT NULL DEFAULT '',
        to_addrs        TEXT    NOT NULL DEFAULT '',
        cc_addrs        TEXT    NOT NULL DEFAULT '',
        snippet         TEXT    NOT NULL DEFAULT '',
        date            INTEGER NOT NULL,
        labels          TEXT    NOT NULL DEFAULT '[]',
        is_read         INTEGER NOT NULL DEFAULT 0,
        is_starred      INTEGER NOT NULL DEFAULT 0,
        has_attachments INTEGER NOT NULL DEFAULT 0,
        message_id_hdr  TEXT,
        references_hdr  TEXT,
        body_html       TEXT,
        body_text       TEXT,
        body_fetched    INTEGER NOT NULL DEFAULT 0,
        UNIQUE(account_id, remote_id)
    );
    CREATE INDEX mail_messages_account_date ON mail_messages(account_id, date DESC);
    CREATE INDEX mail_messages_thread ON mail_messages(account_id, thread_id);

    CREATE TABLE mail_attachments (
        id         INTEGER PRIMARY KEY,
        message_id INTEGER NOT NULL REFERENCES mail_messages(id) ON DELETE CASCADE,
        remote_id  TEXT    NOT NULL,
        filename   TEXT    NOT NULL,
        mime_type  TEXT    NOT NULL,
        size       INTEGER NOT NULL DEFAULT 0
    );
    CREATE INDEX mail_attachments_message ON mail_attachments(message_id);

    CREATE TABLE chat_peers (
        id           INTEGER PRIMARY KEY,
        peer_id      TEXT UNIQUE,
        display_name TEXT    NOT NULL,
        host         TEXT    NOT NULL,
        port         INTEGER NOT NULL,
        last_seen    INTEGER,
        created_at   INTEGER NOT NULL
    );

    CREATE TABLE chat_messages (
        id         INTEGER PRIMARY KEY,
        msg_id     TEXT    NOT NULL UNIQUE,
        peer_id    INTEGER NOT NULL REFERENCES chat_peers(id) ON DELETE CASCADE,
        direction  TEXT    NOT NULL,
        kind       TEXT    NOT NULL,
        body       TEXT    NOT NULL DEFAULT '',
        file_name  TEXT,
        file_path  TEXT,
        file_size  INTEGER,
        status     TEXT    NOT NULL DEFAULT 'sent',
        created_at INTEGER NOT NULL
    );
    CREATE INDEX chat_messages_peer ON chat_messages(peer_id, created_at);
    ",
    // 2: provider labels (Gmail labels, later IMAP folders)
    "
    CREATE TABLE mail_labels (
        id         INTEGER PRIMARY KEY,
        account_id INTEGER NOT NULL REFERENCES mail_accounts(id) ON DELETE CASCADE,
        remote_id  TEXT    NOT NULL,
        name       TEXT    NOT NULL,
        kind       TEXT    NOT NULL,
        bg_color   TEXT,
        fg_color   TEXT,
        visible    INTEGER NOT NULL DEFAULT 1,
        UNIQUE(account_id, remote_id)
    );
    ",
];

pub fn run(conn: &Connection) -> Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate() {
        let version = i as i64 + 1;
        if version <= current {
            continue;
        }
        conn.execute_batch("BEGIN")?;
        if let Err(e) = conn.execute_batch(sql) {
            let _ = conn.execute_batch("ROLLBACK");
            return Err(e.into());
        }
        conn.pragma_update(None, "user_version", version)?;
        conn.execute_batch("COMMIT")?;
        tracing::info!(version, "applied migration");
    }
    Ok(())
}
