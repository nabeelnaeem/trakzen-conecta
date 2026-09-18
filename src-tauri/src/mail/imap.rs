//! IMAP + SMTP as an alternative to Gmail OAuth.
//!
//! Credentials live in the OS keychain; host/port sit in settings. Folders
//! map onto the same label ids the rest of the app already understands
//! (INBOX, SENT, DRAFT, TRASH, SPAM, STARRED).

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};
use crate::secrets;
use crate::settings;
use crate::db::Db;

use super::store::MailStore;
use super::types::*;
use super::{MailProvider, SyncObserver};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImapConfig {
    pub host: String,
    pub port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub username: String,
}

fn cfg_key(email: &str) -> String {
    format!("imap_cfg:{email}")
}

fn pass_key(email: &str) -> String {
    format!("imap:{email}")
}

fn unsupported(op: &str) -> AppError {
    AppError::Other(format!("{op} is not supported on IMAP accounts"))
}

pub struct ImapProvider {
    db: Arc<Db>,
}

impl ImapProvider {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }

    pub fn add_account(
        &self,
        store: &MailStore,
        cfg: ImapConfig,
        password: &str,
    ) -> Result<Account> {
        secrets::set(&pass_key(&cfg.username), password)?;
        settings::set(&self.db, &cfg_key(&cfg.username), &serde_json::to_string(&cfg)?)?;
        store.upsert_account(ProviderKind::Imap.as_str(), &cfg.username, None)
    }

    fn config(&self, account: &Account) -> Result<(ImapConfig, String)> {
        let raw = settings::get(&self.db, &cfg_key(&account.email))?
            .ok_or_else(|| AppError::Other("IMAP settings missing; remove and re-add the account".into()))?;
        let cfg: ImapConfig = serde_json::from_str(&raw)?;
        let pass = secrets::get(&pass_key(&account.email))?
            .ok_or_else(|| AppError::Other("IMAP password missing from the credential store".into()))?;
        Ok((cfg, pass))
    }
}

fn with_session<T>(
    cfg: &ImapConfig,
    password: &str,
    f: impl FnOnce(&mut ::imap::Session<native_tls::TlsStream<std::net::TcpStream>>) -> Result<T>,
) -> Result<T> {
    let tls = native_tls::TlsConnector::builder()
        .build()
        .map_err(|e| AppError::Other(e.to_string()))?;
    let client = ::imap::connect((cfg.host.as_str(), cfg.port), &cfg.host, &tls)
        .map_err(|e| AppError::Other(format!("IMAP connect: {e}")))?;
    let mut session = client
        .login(&cfg.username, password)
        .map_err(|(e, _)| AppError::Other(format!("IMAP login: {e}")))?;
    let out = f(&mut session);
    let _ = session.logout();
    out
}

fn envelope_from_raw(raw: &[u8], fallback_from: &str) -> Result<lettre::address::Envelope> {
    let parsed = mailparse::parse_mail(raw).ok();
    let get = |n: &str| {
        parsed.as_ref().and_then(|p| {
            p.headers
                .iter()
                .find(|h| h.get_key().eq_ignore_ascii_case(n))
                .map(|h| h.get_value())
        })
    };
    let parse_list = |s: &str| -> Vec<lettre::Address> {
        s.split(',')
            .filter_map(|part| {
                let (_, email) = crate::mail::gmail::split_mailbox(part.trim());
                email.parse().ok()
            })
            .collect()
    };
    let from_raw = get("From").unwrap_or_else(|| fallback_from.to_string());
    let (_, from_email) = crate::mail::gmail::split_mailbox(&from_raw);
    let from: lettre::Address = from_email
        .parse()
        .or_else(|_| fallback_from.parse())
        .map_err(|e| AppError::Other(format!("{e}")))?;
    let mut to = parse_list(&get("To").unwrap_or_default());
    to.extend(parse_list(&get("Cc").unwrap_or_default()));
    to.extend(parse_list(&get("Bcc").unwrap_or_default()));
    if to.is_empty() {
        return Err(AppError::Other("message has no To/Cc/Bcc recipients".into()));
    }
    lettre::address::Envelope::new(Some(from), to).map_err(|e| AppError::Other(e.to_string()))
}

fn labels_from_flags(flags: &[::imap::types::Flag<'_>]) -> (Vec<String>, bool, bool) {
    use ::imap::types::Flag;
    let mut labels = vec!["INBOX".into()];
    let mut seen = false;
    let mut starred = false;
    for f in flags {
        match f {
            Flag::Seen => seen = true,
            Flag::Flagged => {
                starred = true;
                labels.push("STARRED".into());
            }
            Flag::Draft => labels.push("DRAFT".into()),
            Flag::Deleted => labels.push("TRASH".into()),
            _ => {}
        }
    }
    if !seen {
        labels.push("UNREAD".into());
    }
    (labels, seen, starred)
}

#[async_trait]
impl MailProvider for ImapProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Imap
    }

    async fn login(&self, _app: &tauri::AppHandle, _store: &MailStore) -> Result<Account> {
        Err(AppError::Other("use Connect IMAP with host and password".into()))
    }

    async fn logout(&self, account: &Account) -> Result<()> {
        let _ = secrets::delete(&pass_key(&account.email));
        let _ = self.db.conn().execute(
            "DELETE FROM settings WHERE key = ?1",
            rusqlite::params![cfg_key(&account.email)],
        );
        Ok(())
    }

    async fn sync(&self, account: &Account, store: &MailStore, observe: SyncObserver<'_>) -> Result<()> {
        observe(SyncEvent::Started {
            account_id: account.id,
            full: true,
        });
        let (cfg, pass) = self.config(account)?;
        let account = account.clone();
        let store = store.clone();
        let fetched = tokio::task::spawn_blocking(move || {
            with_session(&cfg, &pass, |s| {
                s.select("INBOX").map_err(|e| AppError::Other(e.to_string()))?;
                let count = s.search("ALL").map_err(|e| AppError::Other(e.to_string()))?;
                let mut seqs: Vec<u32> = count.into_iter().collect();
                seqs.sort_unstable();
                let take = seqs.into_iter().rev().take(200).collect::<Vec<_>>();
                if take.is_empty() {
                    return Ok(Vec::new());
                }
                let set = take
                    .iter()
                    .map(|n| n.to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                let fetches = s
                    .fetch(&set, "(UID FLAGS BODY.PEEK[HEADER])")
                    .map_err(|e| AppError::Other(e.to_string()))?;
                let mut out = Vec::new();
                for f in fetches.iter() {
                    let uid = f.uid.unwrap_or(0);
                    let (labels, is_read, is_starred) = labels_from_flags(f.flags());
                    let header = f.header().unwrap_or(&[]);
                    let parsed = mailparse::parse_mail(header).ok();
                    let get = |n: &str| {
                        parsed.as_ref().and_then(|p| {
                            p.headers.iter().find(|h| h.get_key().eq_ignore_ascii_case(n)).map(|h| h.get_value())
                        })
                    };
                    let from = get("From").unwrap_or_default();
                    let (from_name, from_addr) = crate::mail::gmail::split_mailbox(&from);
                    out.push(RemoteMessage {
                        remote_id: uid.to_string(),
                        thread_id: get("Message-ID"),
                        subject: get("Subject").unwrap_or_default(),
                        from_name,
                        from_addr,
                        to_addrs: get("To").unwrap_or_default(),
                        cc_addrs: get("Cc").unwrap_or_default(),
                        snippet: String::new(),
                        date: parsed
                            .as_ref()
                            .and_then(|p| p.headers.iter().find(|h| h.get_key().eq_ignore_ascii_case("Date")))
                            .and_then(|h| chrono::DateTime::parse_from_rfc2822(&h.get_value()).ok())
                            .map(|d| d.timestamp_millis())
                            .unwrap_or(0),
                        labels,
                        is_read,
                        is_starred,
                        has_attachments: false,
                        message_id_hdr: get("Message-ID"),
                        references_hdr: None,
                        list_unsubscribe: None,
                        list_unsubscribe_post: None,
                    });
                }
                Ok(out)
            })
        })
        .await
        .map_err(|e| AppError::Other(e.to_string()))??;
        store.upsert_messages(account.id, &fetched)?;
        let _ = store.replace_labels(
            account.id,
            &[RemoteLabel {
                remote_id: "INBOX".into(),
                name: "Inbox".into(),
                kind: "system".into(),
                bg_color: None,
                fg_color: None,
                visible: true,
            }],
        );
        observe(SyncEvent::Finished {
            account_id: account.id,
        });
        Ok(())
    }

    async fn fetch_body(&self, account: &Account, remote_id: &str) -> Result<RemoteBody> {
        let (cfg, pass) = self.config(account)?;
        let uid = remote_id.to_string();
        tokio::task::spawn_blocking(move || {
            with_session(&cfg, &pass, |s| {
                s.select("INBOX").map_err(|e| AppError::Other(e.to_string()))?;
                let fetches = s
                    .uid_fetch(&uid, "BODY.PEEK[]")
                    .map_err(|e| AppError::Other(e.to_string()))?;
                let raw = fetches
                    .iter()
                    .next()
                    .and_then(|f| f.body())
                    .ok_or_else(|| AppError::NotFound(uid.clone()))?;
                let parsed = mailparse::parse_mail(raw).map_err(|e| AppError::Other(e.to_string()))?;
                let mut html = None;
                let mut text = None;
                fn walk(p: &mailparse::ParsedMail, html: &mut Option<String>, text: &mut Option<String>) {
                    let ct = p.ctype.mimetype.to_ascii_lowercase();
                    if ct == "text/html" {
                        if html.is_none() {
                            *html = p.get_body().ok();
                        }
                    } else if ct == "text/plain" && text.is_none() {
                        *text = p.get_body().ok();
                    }
                    for s in &p.subparts {
                        walk(s, html, text);
                    }
                }
                walk(&parsed, &mut html, &mut text);
                Ok(RemoteBody {
                    html,
                    text,
                    attachments: vec![],
                })
            })
        })
        .await
        .map_err(|e| AppError::Other(e.to_string()))?
    }

    async fn fetch_attachment(&self, _account: &Account, _remote_id: &str, _att: &str) -> Result<Vec<u8>> {
        Err(unsupported("IMAP attachment download"))
    }

    async fn send(&self, account: &Account, raw: Vec<u8>, _thread_id: Option<&str>) -> Result<()> {
        let (cfg, pass) = self.config(account)?;
        tokio::task::spawn_blocking(move || {
            use lettre::transport::smtp::authentication::Credentials;
            use lettre::{SmtpTransport, Transport};
            let envelope = envelope_from_raw(&raw, &cfg.username)?;
            let creds = Credentials::new(cfg.username.clone(), pass);
            let mailer = if cfg.smtp_port == 465 {
                SmtpTransport::relay(&cfg.smtp_host)
            } else {
                SmtpTransport::starttls_relay(&cfg.smtp_host)
            }
            .map_err(|e| AppError::Other(e.to_string()))?
            .port(cfg.smtp_port)
            .credentials(creds)
            .build();
            mailer
                .send_raw(&envelope, &raw)
                .map_err(|e| AppError::Other(format!("SMTP: {e}")))?;
            Ok(())
        })
        .await
        .map_err(|e| AppError::Other(e.to_string()))?
    }

    async fn set_flags(&self, account: &Account, remote_id: &str, flags: FlagChange) -> Result<()> {
        let (cfg, pass) = self.config(account)?;
        let uid = remote_id.to_string();
        tokio::task::spawn_blocking(move || {
            with_session(&cfg, &pass, |s| {
                s.select("INBOX").map_err(|e| AppError::Other(e.to_string()))?;
                if flags.read == Some(true) {
                    let _ = s.uid_store(&uid, "+FLAGS (\\Seen)");
                }
                if flags.read == Some(false) {
                    let _ = s.uid_store(&uid, "-FLAGS (\\Seen)");
                }
                if flags.starred == Some(true) {
                    let _ = s.uid_store(&uid, "+FLAGS (\\Flagged)");
                }
                if flags.starred == Some(false) {
                    let _ = s.uid_store(&uid, "-FLAGS (\\Flagged)");
                }
                Ok(())
            })
        })
        .await
        .map_err(|e| AppError::Other(e.to_string()))?
    }

    async fn trash(&self, account: &Account, remote_id: &str) -> Result<()> {
        let (cfg, pass) = self.config(account)?;
        let uid = remote_id.to_string();
        tokio::task::spawn_blocking(move || {
            with_session(&cfg, &pass, |s| {
                s.select("INBOX").map_err(|e| AppError::Other(e.to_string()))?;
                let _ = s.uid_store(&uid, "+FLAGS (\\Deleted)");
                let _ = s.expunge();
                Ok(())
            })
        })
        .await
        .map_err(|e| AppError::Other(e.to_string()))?
    }

    async fn archive(&self, account: &Account, remote_id: &str) -> Result<()> {
        self.trash(account, remote_id).await
    }

    async fn modify_labels(&self, _a: &Account, _id: &str, _add: &[String], _remove: &[String]) -> Result<()> {
        Ok(())
    }

    async fn list_labels(&self, _account: &Account) -> Result<Vec<RemoteLabel>> {
        Ok(vec![
            RemoteLabel { remote_id: "INBOX".into(), name: "Inbox".into(), kind: "system".into(), bg_color: None, fg_color: None, visible: true },
        ])
    }

    async fn create_label(&self, _a: &Account, _name: &str) -> Result<RemoteLabel> {
        Err(unsupported("IMAP labels"))
    }

    async fn list_filters(&self, _a: &Account) -> Result<Vec<MailFilter>> {
        Ok(vec![])
    }

    async fn create_filter(&self, _a: &Account, _f: &NewFilter) -> Result<MailFilter> {
        Err(unsupported("IMAP filters"))
    }

    async fn delete_filter(&self, _a: &Account, _id: &str) -> Result<()> {
        Err(unsupported("IMAP filters"))
    }

    async fn batch_modify(&self, _a: &Account, _ids: &[String], _add: &[String], _remove: &[String]) -> Result<()> {
        Ok(())
    }

    async fn modify_thread(&self, _a: &Account, _t: &str, _add: &[String], _remove: &[String]) -> Result<()> {
        Ok(())
    }

    async fn modify_threads(&self, _a: &Account, _t: &[String], _add: &[String], _remove: &[String]) -> Result<()> {
        Ok(())
    }

    async fn count_view(&self, _a: &Account, _q: &ListQuery) -> Result<u64> {
        Ok(0)
    }

    async fn modify_view(&self, _a: &Account, _q: &ListQuery, _add: &[String], _remove: &[String]) -> Result<usize> {
        Ok(0)
    }

    async fn save_draft(&self, _a: &Account, _raw: Vec<u8>, _t: Option<&str>, _d: Option<&str>) -> Result<String> {
        Err(unsupported("IMAP drafts"))
    }

    async fn delete_draft(&self, _a: &Account, _d: &str) -> Result<()> {
        Ok(())
    }

    async fn open_draft(&self, _a: &Account, _id: &str) -> Result<DraftContent> {
        Err(unsupported("IMAP drafts"))
    }

    async fn search(&self, _a: &Account, _s: &MailStore, _q: &str, _max: u32) -> Result<Vec<String>> {
        Ok(vec![])
    }

    async fn fetch_label_page(
        &self,
        account: &Account,
        store: &MailStore,
        _label_id: &str,
        _page: Option<&str>,
    ) -> Result<PageFetched> {
        self.sync(account, store, &|_| {}).await?;
        Ok(PageFetched {
            added: 0,
            next_page: None,
            oldest: None,
        })
    }
}
