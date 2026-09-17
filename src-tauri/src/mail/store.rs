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
        thread_count: 1,
        thread_unread: 0,
    })
}

fn row_to_thread_summary(r: &Row) -> rusqlite::Result<MessageSummary> {
    let mut m = row_to_summary(r)?;
    m.thread_count = r.get(15)?;
    m.thread_unread = r.get(16)?;
    Ok(m)
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
            let mut contact = tx.prepare_cached(
                "INSERT INTO mail_contacts(account_id, email, name, count, last_seen)
                 VALUES(?1, ?2, ?3, 1, ?4)
                 ON CONFLICT(account_id, email) DO UPDATE SET
                    count = count + 1,
                    last_seen = MAX(last_seen, excluded.last_seen),
                    name = CASE WHEN excluded.name <> '' THEN excluded.name ELSE name END",
            )?;
            let mut exists = tx.prepare_cached(
                "SELECT 1 FROM mail_messages WHERE account_id = ?1 AND remote_id = ?2",
            )?;
            for m in msgs {
                // Only count each message once, not on every label refresh.
                let is_new = !exists.exists(params![account_id, m.remote_id])?;
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
                if !is_new {
                    continue;
                }
                let mut seen = std::collections::HashSet::new();
                let mut record = |raw: &str| -> rusqlite::Result<()> {
                    for mb in super::compose::split_addresses(raw) {
                        let (name, email) = super::gmail::split_mailbox(&mb);
                        let email = email.trim().to_ascii_lowercase();
                        if !email.contains('@') || !seen.insert(email.clone()) {
                            continue;
                        }
                        let name = if name == email { String::new() } else { name };
                        contact.execute(params![account_id, email, name, m.date])?;
                    }
                    Ok(())
                };
                if !m.from_addr.is_empty() {
                    let from = if m.from_name.is_empty() || m.from_name == m.from_addr {
                        m.from_addr.clone()
                    } else {
                        format!("{} <{}>", m.from_name, m.from_addr)
                    };
                    record(&from)?;
                }
                record(&m.to_addrs)?;
                record(&m.cc_addrs)?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// One-off for databases that predate the contacts table: derive it
    /// from the messages already cached. No-op once anything is in there.
    pub fn backfill_contacts(&self) -> Result<usize> {
        let msgs: Vec<(i64, RemoteMessage)> = {
            let conn = self.db.conn();
            let empty: i64 = conn.query_row("SELECT COUNT(*) FROM mail_contacts", [], |r| r.get(0))?;
            if empty > 0 {
                return Ok(0);
            }
            let mut stmt = conn.prepare(
                "SELECT account_id, remote_id, from_name, from_addr, to_addrs, cc_addrs, date FROM mail_messages",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    RemoteMessage {
                        remote_id: r.get(1)?,
                        from_name: r.get(2)?,
                        from_addr: r.get(3)?,
                        to_addrs: r.get(4)?,
                        cc_addrs: r.get(5)?,
                        date: r.get(6)?,
                        ..Default::default()
                    },
                ))
            })?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let mut conn = self.db.conn();
        let tx = conn.transaction()?;
        let mut contact = tx.prepare_cached(
            "INSERT INTO mail_contacts(account_id, email, name, count, last_seen)
             VALUES(?1, ?2, ?3, 1, ?4)
             ON CONFLICT(account_id, email) DO UPDATE SET
                count = count + 1,
                last_seen = MAX(last_seen, excluded.last_seen),
                name = CASE WHEN excluded.name <> '' THEN excluded.name ELSE name END",
        )?;
        let n = msgs.len();
        for (account_id, m) in &msgs {
            let from = if m.from_name.is_empty() || m.from_name == m.from_addr {
                m.from_addr.clone()
            } else {
                format!("{} <{}>", m.from_name, m.from_addr)
            };
            let mut seen = std::collections::HashSet::new();
            for raw in [from.as_str(), m.to_addrs.as_str(), m.cc_addrs.as_str()] {
                for mb in super::compose::split_addresses(raw) {
                    let (name, email) = super::gmail::split_mailbox(&mb);
                    let email = email.trim().to_ascii_lowercase();
                    if !email.contains('@') || !seen.insert(email.clone()) {
                        continue;
                    }
                    let name = if name == email { String::new() } else { name };
                    contact.execute(params![account_id, email, name, m.date])?;
                }
            }
        }
        drop(contact);
        tx.commit()?;
        Ok(n)
    }

    /// Recipient suggestions: prefix/substring match on name or address,
    /// most-used first. The account's own address is excluded.
    pub fn suggest_contacts(&self, account_id: i64, query: &str, limit: i64) -> Result<Vec<Contact>> {
        let like = format!("%{}%", query.trim().replace('%', "").replace('_', ""));
        let conn = self.db.conn();
        let mut stmt = conn.prepare_cached(
            "SELECT c.email, c.name FROM mail_contacts c
             JOIN mail_accounts a ON a.id = c.account_id
             WHERE c.account_id = ?1 AND c.email <> lower(a.email)
               AND (c.email LIKE ?2 OR c.name LIKE ?2)
             ORDER BY (c.email LIKE ?3 OR c.name LIKE ?3) DESC, c.count DESC, c.last_seen DESC
             LIMIT ?4",
        )?;
        let prefix = format!("{}%", query.trim().replace('%', "").replace('_', ""));
        let rows = stmt.query_map(params![account_id, like, prefix, limit], |r| {
            Ok(Contact {
                email: r.get(0)?,
                name: r.get(1)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
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
        query: &ListQuery,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<MessageSummary>> {
        self.list_messages_grouped(account_id, query, limit, offset, false)
    }

    pub fn list_messages_grouped(
        &self,
        account_id: i64,
        query: &ListQuery,
        limit: i64,
        offset: i64,
        conversations: bool,
    ) -> Result<Vec<MessageSummary>> {
        self.list_messages_since(account_id, query, limit, offset, conversations, 0)
    }

    /// `min_date` hides cached rows older than what the server has been
    /// paged to for this view, so the list never skips a gap.
    pub fn list_messages_since(
        &self,
        account_id: i64,
        query: &ListQuery,
        limit: i64,
        offset: i64,
        conversations: bool,
        min_date: i64,
    ) -> Result<Vec<MessageSummary>> {
        // View -> label predicate. Gmail semantics for now; when a second
        // provider lands this moves behind the trait.
        let filter = match (&query.label, query.folder, query.category) {
            (Some(_), _, _) => "NOT has_label(labels, 'TRASH')".to_string(),
            (None, Folder::Inbox, Some(Category::Primary)) => {
                "has_label(labels, 'INBOX') AND NOT has_label(labels, 'TRASH')
                 AND NOT has_label(labels, 'CATEGORY_SOCIAL')
                 AND NOT has_label(labels, 'CATEGORY_PROMOTIONS')
                 AND NOT has_label(labels, 'CATEGORY_UPDATES')
                 AND NOT has_label(labels, 'CATEGORY_FORUMS')"
                    .to_string()
            }
            (None, Folder::Inbox, Some(cat)) => format!(
                "has_label(labels, 'INBOX') AND NOT has_label(labels, 'TRASH') AND has_label(labels, '{}')",
                cat.label_id()
            ),
            (None, Folder::Inbox, None) => {
                "has_label(labels, 'INBOX') AND NOT has_label(labels, 'TRASH')".to_string()
            }
            (None, Folder::Starred, _) => "is_starred = 1 AND NOT has_label(labels, 'TRASH')".to_string(),
            (None, Folder::Sent, _) => "has_label(labels, 'SENT')".to_string(),
            (None, Folder::Drafts, _) => "has_label(labels, 'DRAFT')".to_string(),
            (None, Folder::Archive, _) => {
                "NOT has_label(labels, 'INBOX') AND NOT has_label(labels, 'TRASH')
                 AND NOT has_label(labels, 'SPAM') AND NOT has_label(labels, 'SENT')
                 AND NOT has_label(labels, 'DRAFT')"
                    .to_string()
            }
            (None, Folder::Trash, _) => "has_label(labels, 'TRASH')".to_string(),
            (None, Folder::Spam, _) => "has_label(labels, 'SPAM')".to_string(),
            (None, Folder::Snoozed, _) => {
                "EXISTS (SELECT 1 FROM mail_snoozes s WHERE s.message_id = mail_messages.id)".to_string()
            }
            (None, Folder::All, _) => {
                "NOT has_label(labels, 'TRASH') AND NOT has_label(labels, 'SPAM')".to_string()
            }
        };
        // Snoozed mail hides from the inbox (and tabs) until it is due.
        let snooze = match (&query.label, query.folder) {
            (None, Folder::Inbox) => {
                " AND NOT EXISTS (SELECT 1 FROM mail_snoozes s WHERE s.message_id = mail_messages.id)"
            }
            _ => "",
        };
        // ?4 is bound in every branch so the parameter count is constant.
        let sql = if conversations {
            // One row per thread: the newest matching message represents it,
            // with counts over the whole thread (not just matching members).
            format!(
                "WITH hits AS (
                    SELECT id, COALESCE(thread_id, remote_id) AS tid, date FROM mail_messages
                    WHERE account_id = ?1 AND date >= ?5 AND (?4 = '' OR has_label(labels, ?4)) AND {filter}{snooze}
                 ),
                 newest AS (
                    SELECT id, tid, ROW_NUMBER() OVER (PARTITION BY tid ORDER BY date DESC, id DESC) rn
                    FROM hits
                 )
                 SELECT {SUMMARY_COLS},
                    (SELECT COUNT(*) FROM mail_messages t WHERE t.account_id = ?1
                        AND COALESCE(t.thread_id, t.remote_id) = newest.tid AND NOT has_label(t.labels, 'TRASH')),
                    (SELECT COUNT(*) FROM mail_messages t WHERE t.account_id = ?1
                        AND COALESCE(t.thread_id, t.remote_id) = newest.tid AND t.is_read = 0 AND NOT has_label(t.labels, 'TRASH'))
                 FROM newest JOIN mail_messages USING (id)
                 WHERE rn = 1
                 ORDER BY mail_messages.date DESC LIMIT ?2 OFFSET ?3"
            )
        } else {
            format!(
                "SELECT {SUMMARY_COLS} FROM mail_messages
                 WHERE account_id = ?1 AND date >= ?5 AND (?4 = '' OR has_label(labels, ?4)) AND {filter}{snooze}
                 ORDER BY date DESC LIMIT ?2 OFFSET ?3"
            )
        };
        let conn = self.db.conn();
        let mut stmt = conn.prepare_cached(&sql)?;
        let label = query.label.clone().unwrap_or_default();
        let rows = if conversations {
            stmt.query_map(params![account_id, limit, offset, label, min_date], row_to_thread_summary)?
                .collect::<rusqlite::Result<_>>()?
        } else {
            stmt.query_map(params![account_id, limit, offset, label, min_date], row_to_summary)?
                .collect::<rusqlite::Result<_>>()?
        };
        Ok(rows)
    }

    /// Overwrites labels for cached messages; returns which ids were unknown.
    pub fn apply_label_snapshots(
        &self,
        account_id: i64,
        snapshots: &[(String, Vec<String>)],
    ) -> Result<Vec<String>> {
        let mut conn = self.db.conn();
        let tx = conn.transaction()?;
        let mut unknown = Vec::new();
        {
            let mut stmt = tx.prepare_cached(
                "UPDATE mail_messages SET labels = ?3, is_read = ?4, is_starred = ?5
                 WHERE account_id = ?1 AND remote_id = ?2",
            )?;
            for (id, labels) in snapshots {
                let is_read = !labels.iter().any(|l| l == "UNREAD");
                let is_starred = labels.iter().any(|l| l == "STARRED");
                let n = stmt.execute(params![
                    account_id,
                    id,
                    serde_json::to_string(labels)?,
                    is_read as i64,
                    is_starred as i64
                ])?;
                if n == 0 {
                    unknown.push(id.clone());
                }
            }
        }
        tx.commit()?;
        Ok(unknown)
    }

    pub fn oldest_date_of(&self, account_id: i64, remote_ids: &[String]) -> Result<Option<i64>> {
        if remote_ids.is_empty() {
            return Ok(None);
        }
        let conn = self.db.conn();
        let json = serde_json::to_string(remote_ids)?;
        Ok(conn.query_row(
            "SELECT MIN(date) FROM mail_messages
             WHERE account_id = ?1 AND remote_id IN (SELECT value FROM json_each(?2))",
            params![account_id, json],
            |r| r.get::<_, Option<i64>>(0),
        )?)
    }

    /// Every message of a thread, oldest first (trash excluded).
    pub fn list_thread(&self, account_id: i64, thread_id: &str) -> Result<Vec<MessageSummary>> {
        let conn = self.db.conn();
        let sql = format!(
            "SELECT {SUMMARY_COLS} FROM mail_messages
             WHERE account_id = ?1 AND COALESCE(thread_id, remote_id) = ?2 AND NOT has_label(labels, 'TRASH')
             ORDER BY date ASC, id ASC"
        );
        let mut stmt = conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(params![account_id, thread_id], row_to_summary)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Local (id, remote_id) pairs for a set of local ids, all in one account.
    pub fn remote_ids(&self, ids: &[i64]) -> Result<Vec<(i64, i64, String)>> {
        let conn = self.db.conn();
        let json = serde_json::to_string(ids)?;
        let mut stmt = conn.prepare_cached(
            "SELECT id, account_id, remote_id FROM mail_messages
             WHERE id IN (SELECT value FROM json_each(?1))",
        )?;
        let rows = stmt.query_map(params![json], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn thread_local_ids(&self, account_id: i64, thread_id: &str) -> Result<Vec<i64>> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare_cached(
            "SELECT id FROM mail_messages WHERE account_id = ?1 AND COALESCE(thread_id, remote_id) = ?2",
        )?;
        let rows = stmt.query_map(params![account_id, thread_id], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn set_labels_bulk(&self, ids: &[i64], add: &[&str], remove: &[&str]) -> Result<()> {
        let mut conn = self.db.conn();
        let tx = conn.transaction()?;
        for id in ids {
            let labels: Option<String> = tx
                .query_row(
                    "SELECT labels FROM mail_messages WHERE id = ?1",
                    params![id],
                    |r| r.get(0),
                )
                .optional()?;
            let Some(labels) = labels else { continue };
            let mut labels: Vec<String> = serde_json::from_str(&labels).unwrap_or_default();
            labels.retain(|l| !remove.contains(&l.as_str()));
            for a in add {
                if !labels.iter().any(|l| l == a) {
                    labels.push(a.to_string());
                }
            }
            let is_read = !labels.iter().any(|l| l == "UNREAD");
            let is_starred = labels.iter().any(|l| l == "STARRED");
            tx.execute(
                "UPDATE mail_messages SET labels = ?2, is_read = ?3, is_starred = ?4 WHERE id = ?1",
                params![id, serde_json::to_string(&labels)?, is_read as i64, is_starred as i64],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn summaries_by_remote_ids(&self, account_id: i64, remote_ids: &[String]) -> Result<Vec<MessageSummary>> {
        let conn = self.db.conn();
        let json = serde_json::to_string(remote_ids)?;
        let sql = format!(
            "SELECT {SUMMARY_COLS} FROM mail_messages
             WHERE account_id = ?1 AND remote_id IN (SELECT value FROM json_each(?2))"
        );
        let mut stmt = conn.prepare_cached(&sql)?;
        let mut rows: Vec<MessageSummary> = stmt
            .query_map(params![account_id, json], row_to_summary)?
            .collect::<rusqlite::Result<_>>()?;
        // Keep the server's ranking.
        let order: std::collections::HashMap<&str, usize> =
            remote_ids.iter().enumerate().map(|(i, id)| (id.as_str(), i)).collect();
        rows.sort_by_key(|m| order.get(m.remote_id.as_str()).copied().unwrap_or(usize::MAX));
        Ok(rows)
    }

    // ---- snooze ---------------------------------------------------------

    pub fn snooze(&self, message_id: i64, until: i64) -> Result<()> {
        self.db.conn().execute(
            "INSERT INTO mail_snoozes(message_id, until) VALUES(?1, ?2)
             ON CONFLICT(message_id) DO UPDATE SET until = excluded.until",
            params![message_id, until],
        )?;
        Ok(())
    }

    pub fn unsnooze(&self, message_id: i64) -> Result<()> {
        self.db
            .conn()
            .execute("DELETE FROM mail_snoozes WHERE message_id = ?1", params![message_id])?;
        Ok(())
    }

    pub fn list_snoozed(&self, account_id: i64) -> Result<Vec<(MessageSummary, i64)>> {
        let conn = self.db.conn();
        let sql = format!(
            "SELECT {SUMMARY_COLS}, s.until FROM mail_messages JOIN mail_snoozes s ON s.message_id = mail_messages.id
             WHERE account_id = ?1 ORDER BY s.until ASC"
        );
        let mut stmt = conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(params![account_id], |r| Ok((row_to_summary(r)?, r.get(15)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Removes expired snoozes and returns the messages that just came back.
    pub fn pop_due_snoozes(&self, now: i64) -> Result<Vec<MessageSummary>> {
        let mut conn = self.db.conn();
        let tx = conn.transaction()?;
        let sql = format!(
            "SELECT {SUMMARY_COLS} FROM mail_messages
             WHERE id IN (SELECT message_id FROM mail_snoozes WHERE until <= ?1)"
        );
        let due: Vec<MessageSummary> = tx
            .prepare(&sql)?
            .query_map(params![now], row_to_summary)?
            .collect::<rusqlite::Result<_>>()?;
        tx.execute("DELETE FROM mail_snoozes WHERE until <= ?1", params![now])?;
        tx.commit()?;
        Ok(due)
    }

    // ---- labels ---------------------------------------------------------

    pub fn replace_labels(&self, account_id: i64, labels: &[RemoteLabel]) -> Result<()> {
        let mut conn = self.db.conn();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO mail_labels(account_id, remote_id, name, kind, bg_color, fg_color, visible)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(account_id, remote_id) DO UPDATE SET
                    name = excluded.name, kind = excluded.kind, bg_color = excluded.bg_color,
                    fg_color = excluded.fg_color, visible = excluded.visible",
            )?;
            for l in labels {
                stmt.execute(params![
                    account_id,
                    l.remote_id,
                    l.name,
                    l.kind,
                    l.bg_color,
                    l.fg_color,
                    l.visible as i64
                ])?;
            }
        }
        // Drop labels that no longer exist server-side.
        let keep: Vec<String> = labels.iter().map(|l| l.remote_id.clone()).collect();
        let keep_json = serde_json::to_string(&keep)?;
        tx.execute(
            "DELETE FROM mail_labels WHERE account_id = ?1
             AND remote_id NOT IN (SELECT value FROM json_each(?2))",
            params![account_id, keep_json],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// User labels plus the category labels, each with local counts.
    pub fn list_labels(&self, account_id: i64) -> Result<Vec<Label>> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare_cached(
            "SELECT l.id, l.remote_id, l.name, l.kind, l.bg_color, l.fg_color,
                (SELECT COUNT(*) FROM mail_messages m WHERE m.account_id = l.account_id
                    AND m.is_read = 0 AND has_label(m.labels, l.remote_id) AND NOT has_label(m.labels, 'TRASH')),
                (SELECT COUNT(*) FROM mail_messages m WHERE m.account_id = l.account_id
                    AND has_label(m.labels, l.remote_id) AND NOT has_label(m.labels, 'TRASH'))
             FROM mail_labels l
             WHERE l.account_id = ?1 AND l.visible = 1
               AND (l.kind = 'user' OR l.remote_id LIKE 'CATEGORY_%')
             ORDER BY l.kind DESC, l.name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map(params![account_id], |r| {
            Ok(Label {
                id: r.get(0)?,
                remote_id: r.get(1)?,
                name: r.get(2)?,
                kind: r.get(3)?,
                bg_color: r.get(4)?,
                fg_color: r.get(5)?,
                unread: r.get(6)?,
                total: r.get(7)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn label_names(&self, account_id: i64) -> Result<std::collections::HashMap<String, String>> {
        let conn = self.db.conn();
        let mut stmt =
            conn.prepare_cached("SELECT remote_id, name FROM mail_labels WHERE account_id = ?1")?;
        let rows = stmt.query_map(params![account_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> MailStore {
        let dir = std::env::temp_dir().join(format!("tc-test-{}", uuid::Uuid::new_v4()));
        MailStore::new(Arc::new(Db::open(&dir).unwrap()))
    }

    fn msg(id: &str, labels: &[&str]) -> RemoteMessage {
        RemoteMessage {
            remote_id: id.into(),
            labels: labels.iter().map(|l| l.to_string()).collect(),
            is_read: !labels.contains(&"UNREAD"),
            is_starred: labels.contains(&"STARRED"),
            date: 1,
            ..Default::default()
        }
    }

    #[test]
    fn every_view_queries_without_param_errors() {
        let store = store();
        let acc = store.upsert_account("gmail", "a@b.c", None).unwrap();
        store
            .upsert_messages(
                acc.id,
                &[
                    msg("1", &["INBOX", "UNREAD"]),
                    msg("2", &["INBOX", "CATEGORY_PROMOTIONS"]),
                    msg("3", &["SENT"]),
                    msg("4", &["Label_7", "STARRED"]),
                    msg("5", &["TRASH"]),
                ],
            )
            .unwrap();

        let list = |folder, category, label: Option<&str>| {
            let q = ListQuery {
                folder,
                category,
                label: label.map(str::to_string),
            };
            store
                .list_messages(acc.id, &q, 50, 0)
                .unwrap()
                .into_iter()
                .map(|m| m.remote_id)
                .collect::<Vec<_>>()
        };

        assert_eq!(list(Folder::Inbox, None, None).len(), 2);
        assert_eq!(list(Folder::Inbox, Some(Category::Primary), None), vec!["1"]);
        assert_eq!(list(Folder::Inbox, Some(Category::Promotions), None), vec!["2"]);
        assert_eq!(list(Folder::Sent, None, None), vec!["3"]);
        assert_eq!(list(Folder::Starred, None, None), vec!["4"]);
        assert_eq!(list(Folder::Trash, None, None), vec!["5"]);
        assert_eq!(list(Folder::Archive, None, None), vec!["4"]);
        assert_eq!(list(Folder::All, None, None).len(), 4);
        assert_eq!(list(Folder::Drafts, None, None).len(), 0);
        assert_eq!(list(Folder::Inbox, None, Some("Label_7")), vec!["4"]);

        // Conversation mode: two inbox messages in one thread collapse to the
        // newest, carrying the thread's counts.
        let mut a = msg("6", &["INBOX"]);
        a.thread_id = Some("T".into());
        a.date = 5;
        let mut b = msg("7", &["INBOX", "UNREAD"]);
        b.thread_id = Some("T".into());
        b.date = 9;
        store.upsert_messages(acc.id, &[a, b]).unwrap();
        let q = ListQuery { folder: Folder::Inbox, category: None, label: None };
        let rows = store.list_messages_grouped(acc.id, &q, 50, 0, true).unwrap();
        let t = rows.iter().find(|m| m.remote_id == "7").expect("thread row");
        assert_eq!(t.thread_count, 2);
        assert_eq!(t.thread_unread, 1);
        assert!(!rows.iter().any(|m| m.remote_id == "6"));
        assert_eq!(rows[0].remote_id, "7");
        assert_eq!(store.list_thread(acc.id, "T").unwrap().len(), 2);
        // Every folder works in conversation mode too.
        for f in [Folder::Starred, Folder::Sent, Folder::Drafts, Folder::Archive, Folder::Trash, Folder::Spam, Folder::All] {
            let q = ListQuery { folder: f, category: None, label: None };
            store.list_messages_grouped(acc.id, &q, 50, 0, true).unwrap();
        }
    }
}

#[cfg(test)]
mod contact_tests {
    use super::*;

    #[test]
    fn contacts_are_derived_from_traffic_and_ranked() {
        let dir = std::env::temp_dir().join(format!("tc-test-{}", uuid::Uuid::new_v4()));
        let store = MailStore::new(Arc::new(Db::open(&dir).unwrap()));
        let acc = store.upsert_account("gmail", "me@example.com", None).unwrap();
        let m = |id: &str, from: (&str, &str), to: &str| RemoteMessage {
            remote_id: id.into(),
            from_name: from.0.into(),
            from_addr: from.1.into(),
            to_addrs: to.into(),
            date: 10,
            ..Default::default()
        };
        store
            .upsert_messages(
                acc.id,
                &[
                    m("1", ("Rabiya Gull", "rabiya@example.com"), "Me <me@example.com>"),
                    m("2", ("Rabiya Gull", "rabiya@example.com"), "me@example.com, Rob <rob@example.com>"),
                    m("3", ("Robert", "robert@example.com"), "me@example.com"),
                ],
            )
            .unwrap();
        // Re-upserting the same message must not inflate counts.
        store
            .upsert_messages(acc.id, &[m("3", ("Robert", "robert@example.com"), "me@example.com")])
            .unwrap();

        let got = store.suggest_contacts(acc.id, "r", 10).unwrap();
        let emails: Vec<&str> = got.iter().map(|c| c.email.as_str()).collect();
        assert_eq!(emails, vec!["rabiya@example.com", "rob@example.com", "robert@example.com"]);
        assert_eq!(got[0].name, "Rabiya Gull");
        assert!(store.suggest_contacts(acc.id, "me@", 10).unwrap().is_empty());
    }
}
