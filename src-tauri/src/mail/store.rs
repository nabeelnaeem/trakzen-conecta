use std::sync::Arc;

use rusqlite::{params, OptionalExtension, Row};

use crate::db::{now_ms, Db};
use crate::error::{AppError, Result};

use super::types::*;

pub struct MailStore {
    db: Arc<Db>,
}

const SUMMARY_COLS: &str = "id, account_id, remote_id, thread_id, subject, from_name, from_addr, \
     to_addrs, cc_addrs, snippet, date, labels, is_read, is_starred, has_attachments";

fn row_to_summary(r: &Row) -> rusqlite::Result<MessageSummary> {
    let labels: String = r.get(11)?;
    Ok(MessageSummary {
        id: r.get(0)?,
        account_id: r.get(1)?,
        remote_id: r.get(2)?,
        thread_id: r.get(3)?,
        subject: r.get(4)?,
        from_name: r.get(5)?,
        from_addr: r.get(6)?,
        to_addrs: r.get(7)?,
        cc_addrs: r.get(8)?,
        snippet: r.get(9)?,
        date: r.get(10)?,
        labels: serde_json::from_str(&labels).unwrap_or_default(),
        is_read: r.get::<_, i64>(12)? != 0,
        is_starred: r.get::<_, i64>(13)? != 0,
        has_attachments: r.get::<_, i64>(14)? != 0,
    })
}

fn row_to_account(r: &Row) -> rusqlite::Result<Account> {
    Ok(Account {
        id: r.get(0)?,
        provider: r.get(1)?,
        email: r.get(2)?,
        display_name: r.get(3)?,
        sync_cursor: r.get(4)?,
    })
}

impl MailStore {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }

    // ---- accounts -------------------------------------------------------

    pub fn upsert_account(
        &self,
        provider: &str,
        email: &str,
        display_name: Option<&str>,
    ) -> Result<Account> {
        let conn = self.db.conn();
        conn.execute(
            "INSERT INTO mail_accounts(provider, email, display_name, created_at)
             VALUES(?1, ?2, ?3, ?4)
             ON CONFLICT(email) DO UPDATE SET display_name = COALESCE(excluded.display_name, display_name)",
            params![provider, email, display_name, now_ms()],
        )?;
        conn.query_row(
            "SELECT id, provider, email, display_name, sync_cursor FROM mail_accounts WHERE email = ?1",
            params![email],
            row_to_account,
        )
        .map_err(Into::into)
    }

    pub fn list_accounts(&self) -> Result<Vec<Account>> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare(
            "SELECT id, provider, email, display_name, sync_cursor FROM mail_accounts ORDER BY id",
        )?;
        let rows = stmt.query_map([], row_to_account)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn get_account(&self, id: i64) -> Result<Account> {
        self.db
            .conn()
            .query_row(
                "SELECT id, provider, email, display_name, sync_cursor FROM mail_accounts WHERE id = ?1",
                params![id],
                row_to_account,
            )
            .optional()?
            .ok_or_else(|| AppError::NotFound(format!("account {id}")))
    }

    pub fn delete_account(&self, id: i64) -> Result<()> {
        self.db
            .conn()
            .execute("DELETE FROM mail_accounts WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn set_cursor(&self, account_id: i64, cursor: Option<&str>) -> Result<()> {
        self.db.conn().execute(
            "UPDATE mail_accounts SET sync_cursor = ?2 WHERE id = ?1",
            params![account_id, cursor],
        )?;
        Ok(())
    }

    // ---- messages -------------------------------------------------------

    pub fn upsert_messages(&self, account_id: i64, msgs: &[RemoteMessage]) -> Result<()> {
        let mut conn = self.db.conn();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO mail_messages(account_id, remote_id, thread_id, subject, from_name,
                    from_addr, to_addrs, cc_addrs, snippet, date, labels, is_read, is_starred,
                    has_attachments, message_id_hdr, references_hdr)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
                 ON CONFLICT(account_id, remote_id) DO UPDATE SET
                    thread_id = excluded.thread_id,
                    subject = excluded.subject,
                    from_name = excluded.from_name,
                    from_addr = excluded.from_addr,
                    to_addrs = excluded.to_addrs,
                    cc_addrs = excluded.cc_addrs,
                    snippet = excluded.snippet,
                    date = excluded.date,
                    labels = excluded.labels,
                    is_read = excluded.is_read,
                    is_starred = excluded.is_starred,
                    has_attachments = has_attachments OR excluded.has_attachments,
                    message_id_hdr = COALESCE(excluded.message_id_hdr, message_id_hdr),
                    references_hdr = COALESCE(excluded.references_hdr, references_hdr)",
            )?;
            for m in msgs {
                stmt.execute(params![
                    account_id,
                    m.remote_id,
                    m.thread_id,
                    m.subject,
                    m.from_name,
                    m.from_addr,
                    m.to_addrs,
                    m.cc_addrs,
                    m.snippet,
                    m.date,
                    serde_json::to_string(&m.labels)?,
                    m.is_read as i64,
                    m.is_starred as i64,
                    m.has_attachments as i64,
                    m.message_id_hdr,
                    m.references_hdr,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn delete_messages(&self, account_id: i64, remote_ids: &[String]) -> Result<()> {
        let mut conn = self.db.conn();
        let tx = conn.transaction()?;
        for id in remote_ids {
            tx.execute(
                "DELETE FROM mail_messages WHERE account_id = ?1 AND remote_id = ?2",
                params![account_id, id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn clear_messages(&self, account_id: i64) -> Result<()> {
        self.db.conn().execute(
            "DELETE FROM mail_messages WHERE account_id = ?1",
            params![account_id],
        )?;
        Ok(())
    }

    pub fn mark_has_attachments(&self, account_id: i64, remote_ids: &[String]) -> Result<()> {
        let mut conn = self.db.conn();
        let tx = conn.transaction()?;
        for id in remote_ids {
            tx.execute(
                "UPDATE mail_messages SET has_attachments = 1 WHERE account_id = ?1 AND remote_id = ?2",
                params![account_id, id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn known_remote_ids(&self, account_id: i64) -> Result<Vec<String>> {
        let conn = self.db.conn();
        let mut stmt =
            conn.prepare("SELECT remote_id FROM mail_messages WHERE account_id = ?1")?;
        let rows = stmt.query_map(params![account_id], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn list_messages(
        &self,
        account_id: i64,
        folder: Folder,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<MessageSummary>> {
        // Folder -> label predicate. Gmail semantics for now; when a second
        // provider lands this moves behind the trait.
        let filter = match folder {
            Folder::Inbox => "has_label(labels, 'INBOX') AND NOT has_label(labels, 'TRASH')",
            Folder::Starred => "is_starred = 1 AND NOT has_label(labels, 'TRASH')",
            Folder::Sent => "has_label(labels, 'SENT')",
            Folder::Drafts => "has_label(labels, 'DRAFT')",
            Folder::Archive => {
                "NOT has_label(labels, 'INBOX') AND NOT has_label(labels, 'TRASH')
                 AND NOT has_label(labels, 'SPAM') AND NOT has_label(labels, 'SENT')
                 AND NOT has_label(labels, 'DRAFT')"
            }
            Folder::Trash => "has_label(labels, 'TRASH')",
            Folder::All => "NOT has_label(labels, 'TRASH') AND NOT has_label(labels, 'SPAM')",
        };
        let sql = format!(
            "SELECT {SUMMARY_COLS} FROM mail_messages
             WHERE account_id = ?1 AND {filter}
             ORDER BY date DESC LIMIT ?2 OFFSET ?3"
        );
        let conn = self.db.conn();
        let mut stmt = conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(params![account_id, limit, offset], row_to_summary)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn search_messages(
        &self,
        account_id: i64,
        query: &str,
        limit: i64,
    ) -> Result<Vec<MessageSummary>> {
        let like = format!("%{}%", query.replace('%', "\\%").replace('_', "\\_"));
        let sql = format!(
            "SELECT {SUMMARY_COLS} FROM mail_messages
             WHERE account_id = ?1 AND NOT has_label(labels, 'TRASH')
               AND (subject LIKE ?2 ESCAPE '\\' OR from_name LIKE ?2 ESCAPE '\\'
                    OR from_addr LIKE ?2 ESCAPE '\\' OR snippet LIKE ?2 ESCAPE '\\')
             ORDER BY date DESC LIMIT ?3"
        );
        let conn = self.db.conn();
        let mut stmt = conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(params![account_id, like, limit], row_to_summary)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn unread_count(&self, account_id: i64) -> Result<i64> {
        Ok(self.db.conn().query_row(
            "SELECT COUNT(*) FROM mail_messages
             WHERE account_id = ?1 AND is_read = 0 AND has_label(labels, 'INBOX')
               AND NOT has_label(labels, 'TRASH')",
            params![account_id],
            |r| r.get(0),
        )?)
    }

    pub fn get_message(&self, id: i64) -> Result<MessageDetail> {
        let conn = self.db.conn();
        let sql = format!(
            "SELECT {SUMMARY_COLS}, body_html, body_text, message_id_hdr, references_hdr
             FROM mail_messages WHERE id = ?1"
        );
        let detail = conn
            .query_row(&sql, params![id], |r| {
                Ok(MessageDetail {
                    summary: row_to_summary(r)?,
                    body_html: r.get(15)?,
                    body_text: r.get(16)?,
                    message_id_hdr: r.get(17)?,
                    references_hdr: r.get(18)?,
                    attachments: Vec::new(),
                })
            })
            .optional()?
            .ok_or_else(|| AppError::NotFound(format!("message {id}")))?;

        let mut stmt = conn.prepare_cached(
            "SELECT id, remote_id, filename, mime_type, size FROM mail_attachments
             WHERE message_id = ?1 ORDER BY id",
        )?;
        let attachments = stmt
            .query_map(params![id], |r| {
                Ok(AttachmentInfo {
                    id: r.get(0)?,
                    remote_id: r.get(1)?,
                    filename: r.get(2)?,
                    mime_type: r.get(3)?,
                    size: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(MessageDetail {
            attachments,
            ..detail
        })
    }

    pub fn body_fetched(&self, id: i64) -> Result<bool> {
        Ok(self.db.conn().query_row(
            "SELECT body_fetched FROM mail_messages WHERE id = ?1",
            params![id],
            |r| r.get::<_, i64>(0),
        )? != 0)
    }

    pub fn set_body(&self, id: i64, body: &RemoteBody) -> Result<()> {
        let mut conn = self.db.conn();
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE mail_messages SET body_html = ?2, body_text = ?3, body_fetched = 1,
                has_attachments = has_attachments OR ?4
             WHERE id = ?1",
            params![id, body.html, body.text, !body.attachments.is_empty() as i64],
        )?;
        tx.execute(
            "DELETE FROM mail_attachments WHERE message_id = ?1",
            params![id],
        )?;
        for a in &body.attachments {
            tx.execute(
                "INSERT INTO mail_attachments(message_id, remote_id, filename, mime_type, size)
                 VALUES(?1, ?2, ?3, ?4, ?5)",
                params![id, a.remote_id, a.filename, a.mime_type, a.size],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn get_attachment(&self, id: i64) -> Result<(i64, AttachmentInfo)> {
        self.db
            .conn()
            .query_row(
                "SELECT message_id, id, remote_id, filename, mime_type, size
                 FROM mail_attachments WHERE id = ?1",
                params![id],
                |r| {
                    Ok((
                        r.get(0)?,
                        AttachmentInfo {
                            id: r.get(1)?,
                            remote_id: r.get(2)?,
                            filename: r.get(3)?,
                            mime_type: r.get(4)?,
                            size: r.get(5)?,
                        },
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| AppError::NotFound(format!("attachment {id}")))
    }

    pub fn apply_flags(&self, id: i64, flags: FlagChange) -> Result<()> {
        let conn = self.db.conn();
        if let Some(read) = flags.read {
            conn.execute(
                "UPDATE mail_messages SET is_read = ?2,
                    labels = CASE WHEN ?2 THEN remove_label(labels, 'UNREAD') ELSE add_label(labels, 'UNREAD') END
                 WHERE id = ?1",
                params![id, read as i64],
            )?;
        }
        if let Some(starred) = flags.starred {
            conn.execute(
                "UPDATE mail_messages SET is_starred = ?2,
                    labels = CASE WHEN ?2 THEN add_label(labels, 'STARRED') ELSE remove_label(labels, 'STARRED') END
                 WHERE id = ?1",
                params![id, starred as i64],
            )?;
        }
        Ok(())
    }

    pub fn set_labels(&self, id: i64, add: &[&str], remove: &[&str]) -> Result<()> {
        let conn = self.db.conn();
        let labels: String = conn.query_row(
            "SELECT labels FROM mail_messages WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )?;
        let mut labels: Vec<String> = serde_json::from_str(&labels).unwrap_or_default();
        labels.retain(|l| !remove.contains(&l.as_str()));
        for a in add {
            if !labels.iter().any(|l| l == a) {
                labels.push(a.to_string());
            }
        }
        conn.execute(
            "UPDATE mail_messages SET labels = ?2 WHERE id = ?1",
            params![id, serde_json::to_string(&labels)?],
        )?;
        Ok(())
    }
}

/// SQL helpers over the JSON label array. Registered once per connection.
pub fn register_sql_functions(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    use rusqlite::functions::FunctionFlags;

    let flags = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC;

    conn.create_scalar_function("has_label", 2, flags, |ctx| {
        let labels = ctx.get::<String>(0)?;
        let label = ctx.get::<String>(1)?;
        let parsed: Vec<String> = serde_json::from_str(&labels).unwrap_or_default();
        Ok(parsed.iter().any(|l| *l == label))
    })?;
    conn.create_scalar_function("add_label", 2, flags, |ctx| {
        let labels = ctx.get::<String>(0)?;
        let label = ctx.get::<String>(1)?;
        let mut parsed: Vec<String> = serde_json::from_str(&labels).unwrap_or_default();
        if !parsed.contains(&label) {
            parsed.push(label);
        }
        Ok(serde_json::to_string(&parsed).unwrap_or_else(|_| "[]".into()))
    })?;
    conn.create_scalar_function("remove_label", 2, flags, |ctx| {
        let labels = ctx.get::<String>(0)?;
        let label = ctx.get::<String>(1)?;
        let mut parsed: Vec<String> = serde_json::from_str(&labels).unwrap_or_default();
        parsed.retain(|l| *l != label);
        Ok(serde_json::to_string(&parsed).unwrap_or_else(|_| "[]".into()))
    })?;
    Ok(())
}
