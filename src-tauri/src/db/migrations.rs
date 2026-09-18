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
    // 3: address book derived from mail traffic, for recipient suggestions
    "
    CREATE TABLE mail_contacts (
        id         INTEGER PRIMARY KEY,
        account_id INTEGER NOT NULL REFERENCES mail_accounts(id) ON DELETE CASCADE,
        email      TEXT    NOT NULL,
        name       TEXT    NOT NULL DEFAULT '',
        count      INTEGER NOT NULL DEFAULT 0,
        last_seen  INTEGER NOT NULL DEFAULT 0,
        UNIQUE(account_id, email)
    );
    CREATE INDEX mail_contacts_lookup ON mail_contacts(account_id, count DESC, last_seen DESC);
    ",
    // 4: local snooze (Gmail does not expose snooze through its API)
    "
    CREATE TABLE mail_snoozes (
        message_id INTEGER PRIMARY KEY REFERENCES mail_messages(id) ON DELETE CASCADE,
        until      INTEGER NOT NULL
    );
    CREATE INDEX mail_snoozes_until ON mail_snoozes(until);
    ",
    // 5: quoted replies in chat
    "ALTER TABLE chat_messages ADD COLUMN reply_to TEXT;",
    // 6: RFC 2369/8058 unsubscribe headers, captured with the other metadata
    "
    ALTER TABLE mail_messages ADD COLUMN list_unsubscribe TEXT;
    ALTER TABLE mail_messages ADD COLUMN list_unsubscribe_post TEXT;
    ",
    // 7: reactions, edits, and full-text search over chat
    "
    ALTER TABLE chat_messages ADD COLUMN reactions TEXT NOT NULL DEFAULT '{}';
    ALTER TABLE chat_messages ADD COLUMN edited_at INTEGER;
    CREATE VIRTUAL TABLE chat_fts USING fts5(body, file_name, content='chat_messages', content_rowid='id', tokenize='unicode61');
    INSERT INTO chat_fts(rowid, body, file_name) SELECT id, body, COALESCE(file_name, '') FROM chat_messages;
    CREATE TRIGGER chat_fts_ai AFTER INSERT ON chat_messages BEGIN
        INSERT INTO chat_fts(rowid, body, file_name) VALUES (new.id, new.body, COALESCE(new.file_name, ''));
    END;
    CREATE TRIGGER chat_fts_ad AFTER DELETE ON chat_messages BEGIN
        INSERT INTO chat_fts(chat_fts, rowid, body, file_name) VALUES ('delete', old.id, old.body, COALESCE(old.file_name, ''));
    END;
    CREATE TRIGGER chat_fts_au AFTER UPDATE OF body, file_name ON chat_messages BEGIN
        INSERT INTO chat_fts(chat_fts, rowid, body, file_name) VALUES ('delete', old.id, old.body, COALESCE(old.file_name, ''));
        INSERT INTO chat_fts(rowid, body, file_name) VALUES (new.id, new.body, COALESCE(new.file_name, ''));
    END;
    ",
    // 8: local mail FTS, scheduled send queue
    "
    CREATE VIRTUAL TABLE mail_fts USING fts5(
        subject, from_name, from_addr, snippet, body_text,
        content='mail_messages', content_rowid='id', tokenize='unicode61'
    );
    INSERT INTO mail_fts(rowid, subject, from_name, from_addr, snippet, body_text)
        SELECT id, subject, from_name, from_addr, snippet, COALESCE(body_text, '') FROM mail_messages;
    CREATE TRIGGER mail_fts_ai AFTER INSERT ON mail_messages BEGIN
        INSERT INTO mail_fts(rowid, subject, from_name, from_addr, snippet, body_text)
        VALUES (new.id, new.subject, new.from_name, new.from_addr, new.snippet, COALESCE(new.body_text, ''));
    END;
    CREATE TRIGGER mail_fts_ad AFTER DELETE ON mail_messages BEGIN
        INSERT INTO mail_fts(mail_fts, rowid, subject, from_name, from_addr, snippet, body_text)
        VALUES ('delete', old.id, old.subject, old.from_name, old.from_addr, old.snippet, COALESCE(old.body_text, ''));
    END;
    CREATE TRIGGER mail_fts_au AFTER UPDATE OF subject, from_name, from_addr, snippet, body_text ON mail_messages BEGIN
        INSERT INTO mail_fts(mail_fts, rowid, subject, from_name, from_addr, snippet, body_text)
        VALUES ('delete', old.id, old.subject, old.from_name, old.from_addr, old.snippet, COALESCE(old.body_text, ''));
        INSERT INTO mail_fts(rowid, subject, from_name, from_addr, snippet, body_text)
        VALUES (new.id, new.subject, new.from_name, new.from_addr, new.snippet, COALESCE(new.body_text, ''));
    END;
    CREATE TABLE mail_outbox (
        id         INTEGER PRIMARY KEY,
        account_id INTEGER NOT NULL REFERENCES mail_accounts(id) ON DELETE CASCADE,
        payload    TEXT    NOT NULL,
        send_at    INTEGER NOT NULL,
        created_at INTEGER NOT NULL
    );
    CREATE INDEX mail_outbox_send_at ON mail_outbox(send_at);
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
