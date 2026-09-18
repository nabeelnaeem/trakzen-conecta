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
    "id, msg_id, peer_id, direction, kind, body, file_name, file_path, file_size, status, created_at, reply_to, reactions, edited_at";

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
                file_path, file_size, status, created_at, reply_to)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
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

    pub fn set_status_many(&self, msg_ids: &[String], status: &str) -> Result<Vec<ChatMessage>> {
        let mut out = Vec::new();
        for id in msg_ids {
            if let Some(m) = self.set_status(id, status)? {
                out.push(m);
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
}
