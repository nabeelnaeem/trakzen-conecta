use std::sync::Arc;

use rusqlite::{params, OptionalExtension, Row};

use crate::db::{now_ms, Db};
use crate::error::{AppError, Result};

use super::types::*;

#[derive(Clone)]
pub struct ChatStore {
    db: Arc<Db>,
}

const PEER_COLS: &str = "p.id, p.peer_id, p.display_name, p.host, p.port, p.last_seen,
    (SELECT COUNT(*) FROM chat_messages m WHERE m.peer_id = p.id AND m.direction = 'in' AND m.status = 'unread'),
    (SELECT CASE m.kind WHEN 'file' THEN m.file_name ELSE m.body END FROM chat_messages m
        WHERE m.peer_id = p.id ORDER BY m.created_at DESC, m.id DESC LIMIT 1),
    (SELECT m.created_at FROM chat_messages m WHERE m.peer_id = p.id ORDER BY m.created_at DESC, m.id DESC LIMIT 1),
    p.is_group, p.group_left";

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
        is_group: r.get::<_, i64>(9).unwrap_or(0) != 0,
        group_left: r.get::<_, i64>(10).unwrap_or(0) != 0,
    })
}

const MSG_COLS: &str =
    "id, msg_id, peer_id, direction, kind, body, file_name, file_path, file_size, status, created_at, reply_to, reactions, edited_at, pinned, preview,
     sender_id, (SELECT display_name FROM chat_peers sp WHERE sp.peer_id = chat_messages.sender_id)";

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
        reply_to: r.get(11)?,
        reactions: serde_json::from_str(&r.get::<_, String>(12)?).unwrap_or_default(),
        edited_at: r.get(13)?,
        pinned: r.get::<_, i64>(14).unwrap_or(0) != 0,
        preview: r.get::<_, Option<String>>(15)?.and_then(|s| serde_json::from_str(&s).ok()),
        sender_id: r.get(16)?,
        sender_name: r.get(17)?,
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
    pub reply_to: Option<&'a str>,
    pub sender_id: Option<&'a str>,
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

    pub fn set_peer_name(&self, id: i64, display_name: &str) -> Result<()> {
        self.db.conn().execute(
            "UPDATE chat_peers SET display_name = ?2 WHERE id = ?1 AND is_group = 0",
            params![id, display_name],
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
                file_path, file_size, status, created_at, reply_to, sender_id)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
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
                m.reply_to,
                m.sender_id,
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

    /// Removes one message; returns it so the caller can clean up its file.
    pub fn delete_message(&self, msg_id: &str) -> Result<Option<ChatMessage>> {
        let m = self.get_message(msg_id)?;
        if m.is_some() {
            self.db
                .conn()
                .execute("DELETE FROM chat_messages WHERE msg_id = ?1", params![msg_id])?;
        }
        Ok(m)
    }

    /// Removes every message with a peer; returns the file paths involved.
    pub fn clear_messages(&self, peer_id: i64) -> Result<Vec<String>> {
        let mut conn = self.db.conn();
        let tx = conn.transaction()?;
        let paths: Vec<String> = tx
            .prepare("SELECT file_path FROM chat_messages WHERE peer_id = ?1 AND file_path IS NOT NULL")?
            .query_map(params![peer_id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        tx.execute("DELETE FROM chat_messages WHERE peer_id = ?1", params![peer_id])?;
        tx.commit()?;
        Ok(paths)
    }

    /// Forgets file paths that no longer exist on disk (after a storage
    /// clean-up) so the UI stops offering to open them.
    pub fn forget_missing_files(&self) -> Result<usize> {
        let conn = self.db.conn();
        let rows: Vec<(i64, String)> = conn
            .prepare("SELECT id, file_path FROM chat_messages WHERE file_path IS NOT NULL")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut n = 0;
        for (id, p) in rows {
            if !std::path::Path::new(&p).exists() {
                conn.execute("UPDATE chat_messages SET file_path = NULL WHERE id = ?1", params![id])?;
                n += 1;
            }
        }
        Ok(n)
    }

    /// Marks incoming messages read; returns their ids so the sender can be
    /// told (read receipts).
    pub fn mark_read(&self, peer_id: i64) -> Result<Vec<String>> {
        let mut conn = self.db.conn();
        let tx = conn.transaction()?;
        let ids: Vec<String> = tx
            .prepare("SELECT msg_id FROM chat_messages WHERE peer_id = ?1 AND direction = 'in' AND status = 'unread'")?
            .query_map(params![peer_id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        tx.execute(
            "UPDATE chat_messages SET status = 'received'
             WHERE peer_id = ?1 AND direction = 'in' AND status = 'unread'",
            params![peer_id],
        )?;
        tx.commit()?;
        Ok(ids)
    }

    /// Applies a peer's receipt to our outgoing messages only; a member's
    /// "read" in a group must not touch messages other people sent us.
    pub fn set_status_many(&self, msg_ids: &[String], status: &str) -> Result<Vec<ChatMessage>> {
        let mut out = Vec::new();
        for id in msg_ids {
            let n = self.db.conn().execute(
                "UPDATE chat_messages SET status = ?2 WHERE msg_id = ?1 AND direction = 'out'",
                params![id, status],
            )?;
            if n > 0 {
                if let Some(m) = self.get_message(id)? {
                    out.push(m);
                }
            }
        }
        Ok(out)
    }

    /// Outgoing text that could not be sent because the peer was offline.
    pub fn queued_messages(&self, peer_id: i64) -> Result<Vec<ChatMessage>> {
        let conn = self.db.conn();
        let sql = format!(
            "SELECT {MSG_COLS} FROM chat_messages
             WHERE peer_id = ?1 AND direction = 'out' AND kind = 'text' AND status = 'queued'
             ORDER BY id ASC"
        );
        let mut stmt = conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(params![peer_id], row_to_message)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Outgoing files waiting for the peer, oldest first.
    pub fn queued_files(&self, peer_id: i64) -> Result<Vec<ChatMessage>> {
        let conn = self.db.conn();
        let sql = format!(
            "SELECT {MSG_COLS} FROM chat_messages
             WHERE peer_id = ?1 AND direction = 'out' AND kind = 'file' AND status = 'queued'
             ORDER BY id ASC"
        );
        let mut stmt = conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(params![peer_id], row_to_message)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Peers that have outgoing files waiting, either directly or as a
    /// member of a group with an undelivered file.
    pub fn peers_with_queued_files(&self) -> Result<Vec<i64>> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare_cached(
            "SELECT DISTINCT peer_id FROM chat_messages
             WHERE direction = 'out' AND kind = 'file' AND status = 'queued'
             UNION
             SELECT DISTINCT r.member_id FROM chat_message_recipients r
             JOIN chat_messages m ON m.msg_id = r.msg_id
             WHERE r.delivered = 0 AND m.kind = 'file' AND m.status <> 'failed'",
        )?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    // ---- group delivery -------------------------------------------------

    pub fn add_recipients(&self, msg_id: &str, member_ids: &[i64]) -> Result<()> {
        let conn = self.db.conn();
        for m in member_ids {
            conn.execute(
                "INSERT OR IGNORE INTO chat_message_recipients(msg_id, member_id) VALUES(?1, ?2)",
                params![msg_id, m],
            )?;
        }
        Ok(())
    }

    /// Records one member's ack and rolls the message status up from the
    /// recipients table in the same statement, so acks landing at the same
    /// time from several members cannot overwrite each other's result.
    /// Returns the message if its status changed.
    pub fn mark_delivered(&self, msg_id: &str, member_id: i64) -> Result<Option<ChatMessage>> {
        let mut conn = self.db.conn();
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE chat_message_recipients SET delivered = 1 WHERE msg_id = ?1 AND member_id = ?2",
            params![msg_id, member_id],
        )?;
        let changed = tx.execute(
            "UPDATE chat_messages SET status = CASE
                 WHEN (SELECT COUNT(*) FROM chat_message_recipients r WHERE r.msg_id = ?1 AND r.delivered = 0) = 0
                 THEN 'delivered' ELSE 'sending' END
             WHERE msg_id = ?1 AND direction = 'out' AND status IN ('queued', 'sending')",
            params![msg_id],
        )?;
        tx.commit()?;
        drop(conn);
        if changed == 0 {
            return Ok(None);
        }
        self.get_message(msg_id)
    }

    pub fn delivered_count(&self, msg_id: &str) -> Result<usize> {
        let n: i64 = self.db.conn().query_row(
            "SELECT COUNT(*) FROM chat_message_recipients WHERE msg_id = ?1 AND delivered = 1",
            params![msg_id],
            |r| r.get(0),
        )?;
        Ok(n as usize)
    }

    /// Group messages of the given kind this member has not acknowledged.
    pub fn pending_for_member(&self, member_id: i64, kind: MessageKind) -> Result<Vec<ChatMessage>> {
        let conn = self.db.conn();
        let sql = format!(
            "SELECT {MSG_COLS} FROM chat_messages
             WHERE kind = ?2 AND status <> 'failed' AND msg_id IN (
                 SELECT msg_id FROM chat_message_recipients WHERE member_id = ?1 AND delivered = 0)
             ORDER BY id ASC"
        );
        let mut stmt = conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(params![member_id, kind.as_str()], row_to_message)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Called once at start-up: anything still marked in flight was cut off
    /// by the previous exit. Outgoing work goes back on the queue; incoming
    /// files wait for the sender to offer them again.
    pub fn requeue_unfinished(&self) -> Result<()> {
        let conn = self.db.conn();
        conn.execute(
            "UPDATE chat_messages SET status = 'queued' WHERE direction = 'out' AND status = 'sending'",
            [],
        )?;
        conn.execute(
            "UPDATE chat_messages SET status = 'interrupted' WHERE direction = 'in' AND status = 'receiving'",
            [],
        )?;
        Ok(())
    }

    /// Full-text search (FTS5, prefix matching on every term); falls back to
    /// LIKE if the query cannot be parsed as an FTS expression.
    pub fn search_messages(&self, peer_id: i64, query: &str, limit: i64) -> Result<Vec<ChatMessage>> {
        let conn = self.db.conn();
        let terms: Vec<String> = query
            .split_whitespace()
            .map(|t| format!("\"{}\"*", t.replace('"', "")))
            .collect();
        let fts = terms.join(" ");
        let sql = format!(
            "SELECT {MSG_COLS} FROM chat_messages
             WHERE peer_id = ?1 AND id IN (SELECT rowid FROM chat_fts WHERE chat_fts MATCH ?2)
             ORDER BY id DESC LIMIT ?3"
        );
        let via_fts: rusqlite::Result<Vec<ChatMessage>> = (|| {
            let mut stmt = conn.prepare_cached(&sql)?;
            let rows = stmt.query_map(params![peer_id, fts, limit], row_to_message)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })();
        let mut rows = match via_fts {
            Ok(rows) => rows,
            Err(_) => {
                let like = format!("%{}%", query.replace('%', "").replace('_', ""));
                let sql = format!(
                    "SELECT {MSG_COLS} FROM chat_messages
                     WHERE peer_id = ?1 AND (body LIKE ?2 OR file_name LIKE ?2)
                     ORDER BY id DESC LIMIT ?3"
                );
                conn.prepare_cached(&sql)?
                    .query_map(params![peer_id, like, limit], row_to_message)?
                    .collect::<rusqlite::Result<_>>()?
            }
        };
        rows.reverse();
        Ok(rows)
    }

    /// Adds or removes `who` under `emoji`; returns the updated message.
    pub fn toggle_reaction(&self, msg_id: &str, emoji: &str, who: &str, add: Option<bool>) -> Result<Option<ChatMessage>> {
        let Some(m) = self.get_message(msg_id)? else { return Ok(None) };
        let mut reactions = m.reactions.clone();
        let list = reactions.entry(emoji.to_string()).or_default();
        let present = list.iter().any(|w| w == who);
        let want = add.unwrap_or(!present);
        if want && !present {
            list.push(who.to_string());
        } else if !want && present {
            list.retain(|w| w != who);
        }
        reactions.retain(|_, v| !v.is_empty());
        self.db.conn().execute(
            "UPDATE chat_messages SET reactions = ?2 WHERE msg_id = ?1",
            params![msg_id, serde_json::to_string(&reactions)?],
        )?;
        self.get_message(msg_id)
    }

    pub fn edit_message(&self, msg_id: &str, body: &str) -> Result<Option<ChatMessage>> {
        self.db.conn().execute(
            "UPDATE chat_messages SET body = ?2, edited_at = ?3 WHERE msg_id = ?1 AND kind = 'text'",
            params![msg_id, body, now_ms()],
        )?;
        self.get_message(msg_id)
    }

    pub fn set_pinned(&self, msg_id: &str, pinned: bool) -> Result<Option<ChatMessage>> {
        self.db.conn().execute(
            "UPDATE chat_messages SET pinned = ?2 WHERE msg_id = ?1",
            params![msg_id, pinned as i64],
        )?;
        self.get_message(msg_id)
    }

    pub fn set_preview(&self, msg_id: &str, preview: &LinkPreview) -> Result<Option<ChatMessage>> {
        self.db.conn().execute(
            "UPDATE chat_messages SET preview = ?2 WHERE msg_id = ?1",
            params![msg_id, serde_json::to_string(preview)?],
        )?;
        self.get_message(msg_id)
    }

    // ---- groups ---------------------------------------------------------

    pub fn create_group(&self, name: &str, member_ids: &[i64]) -> Result<Peer> {
        let gid = format!("group:{}", uuid::Uuid::new_v4());
        let id = self.create_group_row(&gid, name, now_ms())?;
        self.set_group_members(id, member_ids)?;
        self.get_peer(id)
    }

    fn create_group_row(&self, group_uuid: &str, name: &str, rev: i64) -> Result<i64> {
        let conn = self.db.conn();
        conn.execute(
            "INSERT INTO chat_peers(peer_id, display_name, host, port, created_at, is_group, group_rev)
             VALUES(?1, ?2, 'group', 0, ?3, 1, ?4)",
            params![group_uuid, name, now_ms(), rev],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn find_by_uuid(&self, peer_uuid: &str) -> Result<Option<i64>> {
        Ok(self
            .db
            .conn()
            .query_row("SELECT id FROM chat_peers WHERE peer_id = ?1", params![peer_uuid], |r| r.get(0))
            .optional()?)
    }

    /// Row for a peer we may only know from a group roster. Existing rows are
    /// left alone; `bind_peer` refreshes them when the peer actually connects.
    pub fn ensure_peer(&self, m: &GroupMember) -> Result<i64> {
        if let Some(id) = self.find_by_uuid(&m.peer_id)? {
            return Ok(id);
        }
        let conn = self.db.conn();
        conn.execute(
            "INSERT INTO chat_peers(peer_id, display_name, host, port, created_at) VALUES(?1, ?2, ?3, ?4, ?5)",
            params![m.peer_id, m.display_name, m.host, m.port as i64, now_ms()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Applies a roster from a peer; returns the group row if it was newer
    /// than what we had (or the group was unknown), `None` if ignored.
    pub fn apply_group_update(
        &self,
        group_uuid: &str,
        name: &str,
        rev: i64,
        member_rows: &[i64],
        i_am_member: bool,
    ) -> Result<Option<i64>> {
        let id = match self.find_by_uuid(group_uuid)? {
            Some(id) => {
                let current: i64 = self.db.conn().query_row(
                    "SELECT group_rev FROM chat_peers WHERE id = ?1",
                    params![id],
                    |r| r.get(0),
                )?;
                if rev <= current {
                    return Ok(None);
                }
                id
            }
            None => self.create_group_row(group_uuid, name, rev)?,
        };
        self.db.conn().execute(
            "UPDATE chat_peers SET display_name = ?2, group_rev = ?3, group_left = ?4, is_group = 1 WHERE id = ?1",
            params![id, name, rev, (!i_am_member) as i64],
        )?;
        if i_am_member {
            self.set_group_members(id, member_rows)?;
        }
        Ok(Some(id))
    }

    pub fn set_group_members(&self, group_id: i64, member_ids: &[i64]) -> Result<()> {
        let mut conn = self.db.conn();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM chat_group_members WHERE group_id = ?1", params![group_id])?;
        for m in member_ids {
            if *m == group_id {
                continue;
            }
            tx.execute(
                "INSERT OR IGNORE INTO chat_group_members(group_id, member_id) VALUES(?1, ?2)",
                params![group_id, m],
            )?;
        }
        // Nothing is owed to people who are no longer in the group.
        tx.execute(
            "DELETE FROM chat_message_recipients
             WHERE msg_id IN (SELECT msg_id FROM chat_messages WHERE peer_id = ?1)
               AND member_id NOT IN (SELECT member_id FROM chat_group_members WHERE group_id = ?1)",
            params![group_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Bumps the roster revision for a local change; returns the new value.
    pub fn touch_group(&self, group_id: i64, name: Option<&str>) -> Result<i64> {
        let rev = now_ms();
        self.db.conn().execute(
            "UPDATE chat_peers SET group_rev = ?2, display_name = COALESCE(?3, display_name) WHERE id = ?1",
            params![group_id, rev, name],
        )?;
        Ok(rev)
    }

    pub fn group_rev(&self, group_id: i64) -> Result<i64> {
        Ok(self.db.conn().query_row(
            "SELECT group_rev FROM chat_peers WHERE id = ?1",
            params![group_id],
            |r| r.get(0),
        )?)
    }

    pub fn set_group_left(&self, group_id: i64, left: bool) -> Result<()> {
        self.db.conn().execute(
            "UPDATE chat_peers SET group_left = ?2 WHERE id = ?1",
            params![group_id, left as i64],
        )?;
        Ok(())
    }

    pub fn group_members(&self, group_id: i64) -> Result<Vec<i64>> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare("SELECT member_id FROM chat_group_members WHERE group_id = ?1")?;
        let rows = stmt.query_map(params![group_id], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn group_member_peers(&self, group_id: i64) -> Result<Vec<Peer>> {
        let conn = self.db.conn();
        let sql = format!(
            "SELECT {PEER_COLS} FROM chat_peers p
             WHERE p.id IN (SELECT member_id FROM chat_group_members WHERE group_id = ?1)
             ORDER BY p.display_name"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![group_id], row_to_peer)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Groups (not left) that this peer belongs to.
    pub fn groups_with_member(&self, member_id: i64) -> Result<Vec<i64>> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare(
            "SELECT g.group_id FROM chat_group_members g
             JOIN chat_peers p ON p.id = g.group_id
             WHERE g.member_id = ?1 AND p.group_left = 0",
        )?;
        let rows = stmt.query_map(params![member_id], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn is_member(&self, group_id: i64, member_id: i64) -> Result<bool> {
        let n: i64 = self.db.conn().query_row(
            "SELECT COUNT(*) FROM chat_group_members WHERE group_id = ?1 AND member_id = ?2",
            params![group_id, member_id],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    pub fn is_group(&self, peer_id: i64) -> Result<bool> {
        Ok(self.db.conn().query_row(
            "SELECT is_group FROM chat_peers WHERE id = ?1",
            params![peer_id],
            |r| r.get::<_, i64>(0),
        )? != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_text_search_reactions_and_edits() {
        let dir = std::env::temp_dir().join(format!("tc-chat-{}", uuid::Uuid::new_v4()));
        let store = ChatStore::new(Arc::new(Db::open(&dir).unwrap()));
        let peer = store.add_peer("Rabiya", "10.0.0.2", 47800).unwrap();
        fn msg<'a>(peer_id: i64, id: &'a str, body: &'a str) -> NewMessage<'a> {
            NewMessage {
                msg_id: id,
                peer_id,
                direction: Direction::Out,
                kind: MessageKind::Text,
                body,
                file_name: None,
                file_path: None,
                file_size: None,
                status: "sent",
                created_at: 1,
                reply_to: None,
                sender_id: None,
            }
        }
        store.insert_message(&msg(peer.id, "a", "Deploying the health controller tonight")).unwrap();
        store.insert_message(&msg(peer.id, "b", "lunch tomorrow?")).unwrap();

        let hits = store.search_messages(peer.id, "health contr", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].msg_id, "a");
        assert!(store.search_messages(peer.id, "nothing-here", 10).unwrap().is_empty());

        let m = store.toggle_reaction("a", "👍", "me", None).unwrap().unwrap();
        assert_eq!(m.reactions["👍"], vec!["me"]);
        let m = store.toggle_reaction("a", "👍", "peer", Some(true)).unwrap().unwrap();
        assert_eq!(m.reactions["👍"].len(), 2);
        let m = store.toggle_reaction("a", "👍", "me", None).unwrap().unwrap();
        assert_eq!(m.reactions["👍"], vec!["peer"]);

        let m = store.edit_message("a", "Deploying tomorrow instead").unwrap().unwrap();
        assert!(m.edited_at.is_some());
        assert_eq!(store.search_messages(peer.id, "tomorrow", 10).unwrap().len(), 2);
        assert!(store.search_messages(peer.id, "tonight", 10).unwrap().is_empty());
    }

    #[test]
    fn file_queue_and_restart_recovery() {
        let dir = std::env::temp_dir().join(format!("tc-chat-{}", uuid::Uuid::new_v4()));
        let store = ChatStore::new(Arc::new(Db::open(&dir).unwrap()));
        let peer = store.add_peer("Rabiya", "10.0.0.2", 47800).unwrap();
        let file = |id: &'static str, direction, status: &'static str| NewMessage {
            msg_id: id,
            peer_id: peer.id,
            direction,
            kind: MessageKind::File,
            body: "",
            file_name: Some("a.bin"),
            file_path: Some("C:/a.bin"),
            file_size: Some(10),
            status,
            created_at: 1,
            reply_to: None,
            sender_id: None,
        };
        store.insert_message(&file("q1", Direction::Out, "queued")).unwrap();
        store.insert_message(&file("s1", Direction::Out, "sending")).unwrap();
        store.insert_message(&file("d1", Direction::Out, "delivered")).unwrap();
        store.insert_message(&file("r1", Direction::In, "receiving")).unwrap();

        let ids: Vec<String> = store.queued_files(peer.id).unwrap().into_iter().map(|m| m.msg_id).collect();
        assert_eq!(ids, vec!["q1"]);
        assert_eq!(store.peers_with_queued_files().unwrap(), vec![peer.id]);

        store.requeue_unfinished().unwrap();
        let ids: Vec<String> = store.queued_files(peer.id).unwrap().into_iter().map(|m| m.msg_id).collect();
        assert_eq!(ids, vec!["q1", "s1"]);
        assert_eq!(store.get_message("d1").unwrap().unwrap().status, "delivered");
        assert_eq!(store.get_message("r1").unwrap().unwrap().status, "interrupted");
    }

    #[test]
    fn group_roster_and_per_member_delivery() {
        let dir = std::env::temp_dir().join(format!("tc-chat-{}", uuid::Uuid::new_v4()));
        let store = ChatStore::new(Arc::new(Db::open(&dir).unwrap()));
        let a = store.add_peer("A", "10.0.0.2", 47800).unwrap();
        let b = store.add_peer("B", "10.0.0.3", 47800).unwrap();
        let group = store.create_group("Team", &[a.id, b.id]).unwrap();
        assert!(group.is_group && !group.group_left);
        assert_eq!(store.group_members(group.id).unwrap().len(), 2);
        assert!(store.is_member(group.id, a.id).unwrap());
        assert_eq!(store.groups_with_member(a.id).unwrap(), vec![group.id]);

        store
            .insert_message(&NewMessage {
                msg_id: "g1",
                peer_id: group.id,
                direction: Direction::Out,
                kind: MessageKind::Text,
                body: "hi all",
                file_name: None,
                file_path: None,
                file_size: None,
                status: "sending",
                created_at: 1,
                reply_to: None,
                sender_id: None,
            })
            .unwrap();
        store.add_recipients("g1", &[a.id, b.id]).unwrap();
        assert_eq!(store.pending_for_member(a.id, MessageKind::Text).unwrap().len(), 1);
        assert_eq!(store.mark_delivered("g1", a.id).unwrap().unwrap().status, "sending");
        assert!(store.pending_for_member(a.id, MessageKind::Text).unwrap().is_empty());
        assert_eq!(store.pending_for_member(b.id, MessageKind::Text).unwrap().len(), 1);
        assert_eq!(store.mark_delivered("g1", b.id).unwrap().unwrap().status, "delivered");
        store.add_recipients("g1", &[b.id]).unwrap();

        // Removing B drops what was owed to B.
        store.set_group_members(group.id, &[a.id]).unwrap();
        assert!(store.pending_for_member(b.id, MessageKind::Text).unwrap().is_empty());
        assert_eq!(store.delivered_count("g1").unwrap(), 1);

        // A roster from a peer wins only when newer.
        let gid = group.peer_id.clone().unwrap();
        let rev = store.group_rev(group.id).unwrap();
        assert!(store.apply_group_update(&gid, "Old", rev - 1, &[a.id, b.id], true).unwrap().is_none());
        assert_eq!(store.get_peer(group.id).unwrap().display_name, "Team");
        let applied = store.apply_group_update(&gid, "Team 2", rev + 1, &[b.id], true).unwrap();
        assert_eq!(applied, Some(group.id));
        assert_eq!(store.get_peer(group.id).unwrap().display_name, "Team 2");
        assert_eq!(store.group_members(group.id).unwrap(), vec![b.id]);

        // A roster without us means we were removed: history stays, flag set.
        store.apply_group_update(&gid, "Team 2", rev + 2, &[], false).unwrap();
        assert!(store.get_peer(group.id).unwrap().group_left);

        // Incoming group messages carry the author's name via the peer table.
        let c = GroupMember { peer_id: "uuid-c".into(), display_name: "Cara".into(), host: "10.0.0.4".into(), port: 47800 };
        let c_row = store.ensure_peer(&c).unwrap();
        assert_eq!(store.ensure_peer(&c).unwrap(), c_row);
        let m = store
            .insert_message(&NewMessage {
                msg_id: "g2",
                peer_id: group.id,
                direction: Direction::In,
                kind: MessageKind::Text,
                body: "hello",
                file_name: None,
                file_path: None,
                file_size: None,
                status: "unread",
                created_at: 2,
                reply_to: None,
                sender_id: Some("uuid-c"),
            })
            .unwrap();
        assert_eq!(m.sender_name.as_deref(), Some("Cara"));

        // A member's read receipt never touches messages they did not send us.
        assert!(store.set_status_many(&["g2".to_string()], "read").unwrap().is_empty());
        assert_eq!(store.get_message("g2").unwrap().unwrap().status, "unread");
    }
}
