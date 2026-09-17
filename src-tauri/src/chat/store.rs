use std::sync::Arc;

use rusqlite::{params, OptionalExtension, Row};

use crate::db::{now_ms, Db};
use crate::error::{AppError, Result};

use super::types::*;

pub struct ChatStore {
    db: Arc<Db>,
}

const PEER_COLS: &str = "p.id, p.peer_id, p.display_name, p.host, p.port, p.last_seen,
    (SELECT COUNT(*) FROM chat_messages m WHERE m.peer_id = p.id AND m.direction = 'in' AND m.status = 'unread'),
    (SELECT CASE m.kind WHEN 'file' THEN m.file_name ELSE m.body END FROM chat_messages m
        WHERE m.peer_id = p.id ORDER BY m.created_at DESC, m.id DESC LIMIT 1),
    (SELECT m.created_at FROM chat_messages m WHERE m.peer_id = p.id ORDER BY m.created_at DESC, m.id DESC LIMIT 1)";

fn row_to_peer(r: &Row) -> rusqlite::Result<Peer> {
    Ok(Peer {
        id: r.get(0)?,
        peer_id: r.get(1)?,
        display_name: r.get(2)?,
        host: r.get(3)?,
        port: r.get::<_, i64>(4)? as u16,
        last_seen: r.get(5)?,
        online: false,
        unread: r.get(6)?,
        last_message: r.get(7)?,
        last_message_at: r.get(8)?,
    })
}

const MSG_COLS: &str =
    "id, msg_id, peer_id, direction, kind, body, file_name, file_path, file_size, status, created_at";

fn row_to_message(r: &Row) -> rusqlite::Result<ChatMessage> {
    let direction: String = r.get(3)?;
    let kind: String = r.get(4)?;
    Ok(ChatMessage {
        id: r.get(0)?,
        msg_id: r.get(1)?,
        peer_id: r.get(2)?,
        direction: if direction == "in" {
            Direction::In
        } else {
            Direction::Out
        },
        kind: if kind == "file" {
            MessageKind::File
        } else {
            MessageKind::Text
        },
        body: r.get(5)?,
        file_name: r.get(6)?,
        file_path: r.get(7)?,
        file_size: r.get(8)?,
        status: r.get(9)?,
        created_at: r.get(10)?,
    })
}

pub struct NewMessage<'a> {
    pub msg_id: &'a str,
    pub peer_id: i64,
    pub direction: Direction,
    pub kind: MessageKind,
    pub body: &'a str,
    pub file_name: Option<&'a str>,
    pub file_path: Option<&'a str>,
    pub file_size: Option<i64>,
    pub status: &'a str,
    pub created_at: i64,
}

impl ChatStore {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }

    // ---- peers ----------------------------------------------------------

    pub fn list_peers(&self) -> Result<Vec<Peer>> {
        let conn = self.db.conn();
        let sql = format!(
            "SELECT {PEER_COLS} FROM chat_peers p
             ORDER BY COALESCE(p.last_seen, 0) DESC, p.display_name"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], row_to_peer)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn get_peer(&self, id: i64) -> Result<Peer> {
        let conn = self.db.conn();
        let sql = format!("SELECT {PEER_COLS} FROM chat_peers p WHERE p.id = ?1");
        conn.query_row(&sql, params![id], row_to_peer)
            .optional()?
            .ok_or_else(|| AppError::NotFound(format!("peer {id}")))
    }

    pub fn add_peer(&self, display_name: &str, host: &str, port: u16) -> Result<Peer> {
        let conn = self.db.conn();
        conn.execute(
            "INSERT INTO chat_peers(display_name, host, port, created_at) VALUES(?1, ?2, ?3, ?4)",
            params![display_name, host, port as i64, now_ms()],
        )?;
        let id = conn.last_insert_rowid();
        drop(conn);
        self.get_peer(id)
    }

    pub fn update_peer(&self, id: i64, display_name: &str, host: &str, port: u16) -> Result<()> {
        self.db.conn().execute(
            "UPDATE chat_peers SET display_name = ?2, host = ?3, port = ?4 WHERE id = ?1",
            params![id, display_name, host, port as i64],
        )?;
        Ok(())
    }

    pub fn remove_peer(&self, id: i64) -> Result<()> {
        self.db
            .conn()
            .execute("DELETE FROM chat_peers WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn touch_peer(&self, id: i64) -> Result<()> {
        self.db.conn().execute(
            "UPDATE chat_peers SET last_seen = ?2 WHERE id = ?1",
            params![id, now_ms()],
        )?;
        Ok(())
    }

    /// Resolves the local row for a remote peer once we know its stable id.
    /// `hint_row` is the row we dialled out on (if any); it gets bound to the
    /// peer id unless another row already owns it, in which case the two are
    /// merged so history is not split across duplicates.
    pub fn bind_peer(
        &self,
        hint_row: Option<i64>,
        peer_id: &str,
        display_name: &str,
        host: &str,
        port: u16,
    ) -> Result<i64> {
        let mut conn = self.db.conn();
        let tx = conn.transaction()?;
        let existing: Option<i64> = tx
            .query_row(
                "SELECT id FROM chat_peers WHERE peer_id = ?1",
                params![peer_id],
                |r| r.get(0),
            )
            .optional()?;

        let id = match (existing, hint_row) {
            (Some(existing), Some(hint)) if existing != hint => {
                tx.execute(
                    "UPDATE chat_messages SET peer_id = ?1 WHERE peer_id = ?2",
                    params![existing, hint],
                )?;
                tx.execute("DELETE FROM chat_peers WHERE id = ?1", params![hint])?;
                existing
            }
            (Some(existing), _) => existing,
            (None, Some(hint)) => {
                tx.execute(
                    "UPDATE chat_peers SET peer_id = ?2 WHERE id = ?1",
                    params![hint, peer_id],
                )?;
                hint
            }
            (None, None) => {
                // Maybe the user pre-added this machine by IP without a name.
                let by_host: Option<i64> = tx
                    .query_row(
                        "SELECT id FROM chat_peers WHERE peer_id IS NULL AND host = ?1",
                        params![host],
                        |r| r.get(0),
                    )
                    .optional()?;
                match by_host {
                    Some(id) => {
                        tx.execute(
                            "UPDATE chat_peers SET peer_id = ?2 WHERE id = ?1",
                            params![id, peer_id],
                        )?;
                        id
                    }
                    None => {
                        tx.execute(
                            "INSERT INTO chat_peers(peer_id, display_name, host, port, created_at)
                             VALUES(?1, ?2, ?3, ?4, ?5)",
                            params![peer_id, display_name, host, port as i64, now_ms()],
                        )?;
                        tx.last_insert_rowid()
                    }
                }
            }
        };

        tx.execute(
            "UPDATE chat_peers SET display_name = ?2, host = ?3, port = ?4, last_seen = ?5 WHERE id = ?1",
            params![id, display_name, host, port as i64, now_ms()],
        )?;
        tx.commit()?;
        Ok(id)
    }

    // ---- messages -------------------------------------------------------

    pub fn insert_message(&self, m: &NewMessage) -> Result<ChatMessage> {
        let conn = self.db.conn();
        conn.execute(
            "INSERT INTO chat_messages(msg_id, peer_id, direction, kind, body, file_name,
                file_path, file_size, status, created_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(msg_id) DO NOTHING",
            params![
                m.msg_id,
                m.peer_id,
                m.direction.as_str(),
                m.kind.as_str(),
                m.body,
                m.file_name,
                m.file_path,
                m.file_size,
                m.status,
                m.created_at,
            ],
        )?;
        let sql = format!("SELECT {MSG_COLS} FROM chat_messages WHERE msg_id = ?1");
        Ok(conn.query_row(&sql, params![m.msg_id], row_to_message)?)
    }

    pub fn get_message(&self, msg_id: &str) -> Result<Option<ChatMessage>> {
        let conn = self.db.conn();
        let sql = format!("SELECT {MSG_COLS} FROM chat_messages WHERE msg_id = ?1");
        Ok(conn
            .query_row(&sql, params![msg_id], row_to_message)
            .optional()?)
    }

    pub fn set_status(&self, msg_id: &str, status: &str) -> Result<Option<ChatMessage>> {
        self.db.conn().execute(
            "UPDATE chat_messages SET status = ?2 WHERE msg_id = ?1",
            params![msg_id, status],
        )?;
        self.get_message(msg_id)
    }

    pub fn set_file_result(
        &self,
        msg_id: &str,
        status: &str,
        file_path: Option<&str>,
    ) -> Result<Option<ChatMessage>> {
        self.db.conn().execute(
            "UPDATE chat_messages SET status = ?2, file_path = COALESCE(?3, file_path) WHERE msg_id = ?1",
            params![msg_id, status, file_path],
        )?;
        self.get_message(msg_id)
    }

    pub fn list_messages(
        &self,
        peer_id: i64,
        limit: i64,
        before_id: Option<i64>,
    ) -> Result<Vec<ChatMessage>> {
        let conn = self.db.conn();
        let sql = format!(
            "SELECT {MSG_COLS} FROM chat_messages
             WHERE peer_id = ?1 AND (?2 IS NULL OR id < ?2)
             ORDER BY id DESC LIMIT ?3"
        );
        let mut stmt = conn.prepare_cached(&sql)?;
        let mut rows: Vec<ChatMessage> = stmt
            .query_map(params![peer_id, before_id, limit], row_to_message)?
            .collect::<rusqlite::Result<_>>()?;
        rows.reverse();
        Ok(rows)
    }

    pub fn mark_read(&self, peer_id: i64) -> Result<()> {
        self.db.conn().execute(
            "UPDATE chat_messages SET status = 'received'
             WHERE peer_id = ?1 AND direction = 'in' AND status = 'unread'",
            params![peer_id],
        )?;
        Ok(())
    }
}
