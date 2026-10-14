//! IMAP + SMTP as an alternative to Gmail OAuth.
//!
//! Credentials live in the OS keychain; host/port sit in settings. Folders
//! map onto the same label ids the rest of the app already understands
//! (INBOX, SENT, DRAFT, TRASH, SPAM, STARRED). Remote ids name the folder
//! and UIDVALIDITY as well as the UID, see [`folders::MsgRef`].

mod folders;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};
use crate::secrets;
use crate::settings;
use crate::db::Db;

use super::store::MailStore;
use super::types::*;
use super::{MailProvider, SyncObserver};

use folders::{FolderMap, FolderScan, ListedFolder, MsgRef, Role};

type Session = ::imap::Session<native_tls::TlsStream<std::net::TcpStream>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SmtpSecurity {
    /// TLS from the first byte (usually port 465).
    Tls,
    /// Plain connection upgraded with STARTTLS (usually port 587).
    Starttls,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImapConfig {
    pub host: String,
    pub port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    /// Accounts added before this was configurable have none stored and
    /// keep the old port-based choice.
    #[serde(default)]
    pub smtp_security: Option<SmtpSecurity>,
    pub username: String,
}

impl ImapConfig {
    pub fn smtp_implicit_tls(&self) -> bool {
        match self.smtp_security {
            Some(SmtpSecurity::Tls) => true,
            Some(SmtpSecurity::Starttls) => false,
            None => self.smtp_port == 465,
        }
    }
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
    /// Folder roles per account email, refreshed on every sync.
    folders: Mutex<HashMap<String, FolderMap>>,
}

impl ImapProvider {
    pub fn new(db: Arc<Db>) -> Self {
        Self {
            db,
            folders: Mutex::new(HashMap::new()),
        }
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

    /// Logs in once so a wrong host or password is reported while the
    /// user is still looking at the form, not by the first sync.
    pub fn check_login(cfg: &ImapConfig, password: &str) -> Result<()> {
        with_session(cfg, password, |_| Ok(()))
    }

    fn config(&self, account: &Account) -> Result<(ImapConfig, String)> {
        let raw = settings::get(&self.db, &cfg_key(&account.email))?
            .ok_or_else(|| AppError::Other("IMAP settings missing; remove and re-add the account".into()))?;
        let cfg: ImapConfig = serde_json::from_str(&raw)?;
        let pass = secrets::get(&pass_key(&account.email))?
            .ok_or_else(|| AppError::Other("IMAP password missing from the credential store".into()))?;
        Ok((cfg, pass))
    }

    fn cached_folders(&self, account: &Account) -> Option<FolderMap> {
        self.folders.lock().unwrap().get(&account.email).cloned()
    }

    fn remember_folders(&self, account: &Account, map: FolderMap) {
        self.folders.lock().unwrap().insert(account.email.clone(), map);
    }
}

fn imap_err(e: ::imap::Error) -> AppError {
    AppError::Other(format!("IMAP: {e}"))
}

fn with_session<T>(
    cfg: &ImapConfig,
    password: &str,
    f: impl FnOnce(&mut Session) -> Result<T>,
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

/// lettre greets with `EHLO [127.0.0.1]` by default, which some servers
/// refuse. Use this machine's name when it is fully qualified, otherwise
/// the sender's domain.
fn ehlo_name(email: &str) -> lettre::transport::smtp::extension::ClientId {
    use lettre::transport::smtp::extension::ClientId;
    let host = std::env::var("COMPUTERNAME")
        .ok()
        .or_else(|| std::env::var("HOSTNAME").ok())
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok());
    match pick_ehlo_name(host.as_deref(), email) {
        Some(name) => ClientId::Domain(name),
        None => ClientId::default(),
    }
}

fn pick_ehlo_name(host: Option<&str>, email: &str) -> Option<String> {
    let fqdn = |h: &str| {
        let labels: Vec<&str> = h.split('.').collect();
        labels.len() >= 2
            && labels
                .iter()
                .all(|l| !l.is_empty() && !l.starts_with('-') && !l.ends_with('-') && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
            && !labels.last().is_some_and(|l| l.chars().all(|c| c.is_ascii_digit()))
            && !["local", "localdomain", "lan", "home", "internal"].iter().any(|t| labels.last().is_some_and(|l| l.eq_ignore_ascii_case(t)))
    };
    host.map(|h| h.trim().trim_end_matches('.').to_ascii_lowercase())
        .filter(|h| fqdn(h))
        .or_else(|| super::compose::sender_domain(email).map(str::to_ascii_lowercase))
}

fn discover_folders(s: &mut Session) -> Result<FolderMap> {
    use ::imap::types::NameAttribute;
    let names = s.list(Some(""), Some("*")).map_err(imap_err)?;
    let listed: Vec<ListedFolder> = names
        .iter()
        .map(|n| ListedFolder {
            name: n.name().to_string(),
            delimiter: n.delimiter().map(str::to_string),
            attributes: n
                .attributes()
                .iter()
                .map(|a| match a {
                    NameAttribute::NoInferiors => "\\Noinferiors".to_string(),
                    NameAttribute::NoSelect => "\\Noselect".to_string(),
                    NameAttribute::Marked => "\\Marked".to_string(),
                    NameAttribute::Unmarked => "\\Unmarked".to_string(),
                    NameAttribute::Custom(c) => c.to_string(),
                })
                .collect(),
        })
        .collect();
    Ok(FolderMap::from_list(&listed))
}

/// Labels from the folder a message sits in plus its flags.
fn labels_for(role: Role, flags: &[::imap::types::Flag<'_>]) -> (Vec<String>, bool, bool) {
    use ::imap::types::Flag;
    let mut labels: Vec<String> = role.label().map(str::to_string).into_iter().collect();
    let seen = flags.contains(&Flag::Seen);
    let starred = flags.contains(&Flag::Flagged);
    if starred {
        labels.push("STARRED".into());
    }
    if flags.contains(&Flag::Draft) && role != Role::Drafts {
        labels.push("DRAFT".into());
    }
    if !seen {
        labels.push("UNREAD".into());
    }
    (labels, seen, starred)
}

fn message_from_header(
    remote_id: String,
    role: Role,
    flags: &[::imap::types::Flag<'_>],
    internal_date: Option<i64>,
    header: &[u8],
) -> RemoteMessage {
    let (labels, is_read, is_starred) = labels_for(role, flags);
    let parsed = mailparse::parse_mail(header).ok();
    let get = |n: &str| {
        parsed.as_ref().and_then(|p| {
            p.headers
                .iter()
                .find(|h| h.get_key().eq_ignore_ascii_case(n))
                .map(|h| h.get_value())
        })
    };
    let from = get("From").unwrap_or_default();
    let (from_name, from_addr) = crate::mail::gmail::split_mailbox(&from);
    // mailparse shrugs off a weekday that does not match the date, which
    // real mail has and chrono's RFC 2822 parser rejects.
    let header_date = get("Date")
        .and_then(|d| mailparse::dateparse(d.trim()).ok())
        .map(|secs| secs * 1000);
    RemoteMessage {
        remote_id,
        thread_id: get("Message-ID"),
        subject: get("Subject").unwrap_or_default(),
        from_name,
        from_addr,
        to_addrs: get("To").unwrap_or_default(),
        cc_addrs: get("Cc").unwrap_or_default(),
        snippet: String::new(),
        date: internal_date.or(header_date).unwrap_or(0),
        labels,
        is_read,
        is_starred,
        has_attachments: false,
        message_id_hdr: get("Message-ID"),
        references_hdr: get("References"),
        list_unsubscribe: None,
        list_unsubscribe_post: None,
    }
}

/// Reads the newest messages of every synced folder in one session.
fn scan_folders(s: &mut Session, map: &FolderMap) -> Result<(Vec<RemoteMessage>, Vec<FolderScan>)> {
    use ::imap::types::Flag;
    let mut messages = Vec::new();
    let mut scans = Vec::new();
    for (role, limit) in Role::SYNCED {
        let Some(folder) = map.get(role) else { continue };
        let mailbox = match s.examine(folder) {
            Ok(m) => m,
            Err(e) if role != Role::Inbox => {
                tracing::warn!(folder, error = %e, "IMAP folder could not be opened");
                continue;
            }
            Err(e) => return Err(imap_err(e)),
        };
        let validity = mailbox.uid_validity.unwrap_or(0);
        let uids = s.uid_search("ALL").map_err(imap_err)?;
        let mut newest: Vec<u32> = uids.iter().copied().collect();
        newest.sort_unstable_by(|a, b| b.cmp(a));
        newest.truncate(limit);
        if !newest.is_empty() {
            let fetches = s
                .uid_fetch(folders::uid_set(&newest), "(UID FLAGS INTERNALDATE BODY.PEEK[HEADER])")
                .map_err(imap_err)?;
            for f in fetches.iter() {
                let Some(uid) = f.uid else { continue };
                // Flagged for removal by another client; gone on its next expunge.
                if f.flags().contains(&Flag::Deleted) {
                    continue;
                }
                messages.push(message_from_header(
                    folders::remote_id(folder, validity, uid),
                    role,
                    f.flags(),
                    f.internal_date().map(|d| d.timestamp_millis()),
                    f.header().unwrap_or(&[]),
                ));
            }
        }
        let uid_next = mailbox
            .uid_next
            .unwrap_or_else(|| uids.iter().max().map_or(1, |m| m + 1));
        scans.push(FolderScan {
            folder: folder.to_string(),
            validity,
            uids,
            uid_next,
        });
    }
    Ok((messages, scans))
}

/// Opens the message's folder, refusing if the folder was recreated since
/// the id was handed out (its UIDs now point at other messages).
fn open_folder(s: &mut Session, r: &MsgRef, write: bool) -> Result<()> {
    let mailbox = if write { s.select(&r.folder) } else { s.examine(&r.folder) }.map_err(imap_err)?;
    match r.validity {
        Some(v) if mailbox.uid_validity != Some(v) => Err(AppError::NotFound(
            "this message changed on the server; sync and try again".into(),
        )),
        _ => Ok(()),
    }
}

fn parse_ref(remote_id: &str) -> Result<MsgRef> {
    MsgRef::parse(remote_id).ok_or_else(|| AppError::NotFound(format!("IMAP message {remote_id}")))
}

fn fetch_raw(s: &mut Session, r: &MsgRef) -> Result<Vec<u8>> {
    open_folder(s, r, false)?;
    let fetches = s.uid_fetch(r.uid.to_string(), "(UID BODY.PEEK[])").map_err(imap_err)?;
    fetches
        .iter()
        .find(|f| f.uid == Some(r.uid))
        .and_then(|f| f.body())
        .map(<[u8]>::to_vec)
        .ok_or_else(|| AppError::NotFound(format!("IMAP message {} in {}", r.uid, r.folder)))
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
        let (map, fetched, scans) = tokio::task::spawn_blocking(move || {
            with_session(&cfg, &pass, |s| {
                let map = discover_folders(s)?;
                let (messages, scans) = scan_folders(s, &map)?;
                Ok((map, messages, scans))
            })
        })
        .await
        .map_err(|e| AppError::Other(e.to_string()))??;
        // Also clears rows from before ids named their folder, which would
        // otherwise sit next to their re-synced copies.
        let stale = folders::stale_ids(&store.known_remote_ids(account.id)?, &scans);
        if !stale.is_empty() {
            store.delete_messages(account.id, &stale)?;
        }
        store.upsert_messages(account.id, &fetched)?;
        store.replace_labels(account.id, &map.labels())?;
        self.remember_folders(account, map);
        observe(SyncEvent::Finished {
            account_id: account.id,
        });
        Ok(())
    }

    async fn fetch_body(&self, account: &Account, remote_id: &str) -> Result<RemoteBody> {
        let (cfg, pass) = self.config(account)?;
        let r = parse_ref(remote_id)?;
        tokio::task::spawn_blocking(move || {
            let raw = with_session(&cfg, &pass, |s| fetch_raw(s, &r))?;
            let parsed = mailparse::parse_mail(&raw).map_err(|e| AppError::Other(e.to_string()))?;
            let mut out = RemoteBody::default();
            walk_parts(&parsed, "", &mut out);
            Ok(out)
        })
        .await
        .map_err(|e| AppError::Other(e.to_string()))?
    }

    /// `att` is the part path handed out by `walk_parts` ("1.2"); the whole
    /// message is fetched again and that part decoded.
    async fn fetch_attachment(&self, account: &Account, remote_id: &str, att: &str) -> Result<Vec<u8>> {
        let (cfg, pass) = self.config(account)?;
        let r = parse_ref(remote_id)?;
        let path = att.to_string();
        tokio::task::spawn_blocking(move || {
            let raw = with_session(&cfg, &pass, |s| fetch_raw(s, &r))?;
            let parsed = mailparse::parse_mail(&raw).map_err(|e| AppError::Other(e.to_string()))?;
            let mut part = &parsed;
            for idx in path.split('.').filter(|s| !s.is_empty()) {
                let i: usize = idx.parse().map_err(|_| AppError::NotFound(format!("part {path}")))?;
                part = part.subparts.get(i).ok_or_else(|| AppError::NotFound(format!("part {path}")))?;
            }
            part.get_body_raw().map_err(|e| AppError::Other(e.to_string()))
        })
        .await
        .map_err(|e| AppError::Other(e.to_string()))?
    }

    async fn send(&self, account: &Account, raw: Vec<u8>, _thread_id: Option<&str>) -> Result<()> {
        let (cfg, pass) = self.config(account)?;
        tokio::task::spawn_blocking(move || {
            use lettre::transport::smtp::authentication::Credentials;
            use lettre::{SmtpTransport, Transport};
            let envelope = envelope_from_raw(&raw, &cfg.username)?;
            let raw = super::compose::without_bcc(&raw);
            let creds = Credentials::new(cfg.username.clone(), pass);
            let mailer = if cfg.smtp_implicit_tls() {
                SmtpTransport::relay(&cfg.smtp_host)
            } else {
                SmtpTransport::starttls_relay(&cfg.smtp_host)
            }
            .map_err(|e| AppError::Other(e.to_string()))?
            .port(cfg.smtp_port)
            .hello_name(ehlo_name(&cfg.username))
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
        let r = parse_ref(remote_id)?;
        tokio::task::spawn_blocking(move || {
            with_session(&cfg, &pass, |s| {
                open_folder(s, &r, true)?;
                let uid = r.uid.to_string();
                let mut store = |query: &str| s.uid_store(&uid, query).map(|_| ()).map_err(imap_err);
                match flags.read {
                    Some(true) => store("+FLAGS.SILENT (\\Seen)")?,
                    Some(false) => store("-FLAGS.SILENT (\\Seen)")?,
                    None => {}
                }
                match flags.starred {
                    Some(true) => store("+FLAGS.SILENT (\\Flagged)")?,
                    Some(false) => store("-FLAGS.SILENT (\\Flagged)")?,
                    None => {}
                }
                Ok(())
            })
        })
        .await
        .map_err(|e| AppError::Other(e.to_string()))?
    }

    async fn trash(&self, account: &Account, remote_id: &str) -> Result<()> {
        let (cfg, pass) = self.config(account)?;
        let r = parse_ref(remote_id)?;
        tokio::task::spawn_blocking(move || {
            with_session(&cfg, &pass, |s| {
                open_folder(s, &r, true)?;
                s.uid_store(r.uid.to_string(), "+FLAGS.SILENT (\\Deleted)").map_err(imap_err)?;
                s.expunge().map_err(imap_err)?;
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

    async fn list_labels(&self, account: &Account) -> Result<Vec<RemoteLabel>> {
        let map = self.cached_folders(account).unwrap_or_else(|| FolderMap::from_list(&[]));
        Ok(map.labels())
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

/// Collects the first text/html and text/plain bodies plus every part that
/// is an attachment or an inline image. Attachment ids are the part path
/// within the message, e.g. "1.0", so they can be found again later.
fn walk_parts(p: &mailparse::ParsedMail, path: &str, out: &mut RemoteBody) {
    let ct = p.ctype.mimetype.to_ascii_lowercase();
    let disposition = p.get_content_disposition();
    let filename = disposition.params.get("filename").cloned().or_else(|| p.ctype.params.get("name").cloned());
    let content_id = p
        .headers
        .iter()
        .find(|h| h.get_key_ref().eq_ignore_ascii_case("Content-ID"))
        .map(|h| strip_angle_brackets(&h.get_value()));
    let is_attachment = disposition.disposition == mailparse::DispositionType::Attachment
        || (disposition.disposition == mailparse::DispositionType::Inline && content_id.is_some())
        || (filename.is_some() && !ct.starts_with("text/") && !ct.starts_with("multipart/"));

    if is_attachment && !path.is_empty() {
        out.attachments.push(RemoteAttachment {
            remote_id: path.to_string(),
            filename: filename.unwrap_or_else(|| "inline".into()),
            mime_type: if ct.is_empty() { "application/octet-stream".into() } else { ct.clone() },
            size: p.get_body_raw().map(|b| b.len() as i64).unwrap_or(0),
            content_id,
        });
    } else if ct == "text/html" {
        if out.html.is_none() {
            out.html = p.get_body().ok();
        }
    } else if ct == "text/plain" && out.text.is_none() {
        out.text = p.get_body().ok();
    }
    for (i, sub) in p.subparts.iter().enumerate() {
        let child = if path.is_empty() { i.to_string() } else { format!("{path}.{i}") };
        walk_parts(sub, &child, out);
    }
}

fn strip_angle_brackets(s: &str) -> String {
    s.trim().trim_start_matches('<').trim_end_matches('>').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(port: u16, security: Option<SmtpSecurity>) -> ImapConfig {
        ImapConfig {
            host: "imap.example.com".into(),
            port: 993,
            smtp_host: "smtp.example.com".into(),
            smtp_port: port,
            smtp_security: security,
            username: "me@example.com".into(),
        }
    }

    #[test]
    fn smtp_security_falls_back_to_port() {
        assert!(cfg(465, None).smtp_implicit_tls());
        assert!(!cfg(587, None).smtp_implicit_tls());
        assert!(cfg(2525, Some(SmtpSecurity::Tls)).smtp_implicit_tls());
        assert!(!cfg(465, Some(SmtpSecurity::Starttls)).smtp_implicit_tls());
    }

    #[test]
    fn ehlo_uses_a_qualified_hostname_or_the_sender_domain() {
        assert_eq!(pick_ehlo_name(Some("mail.office.example.com\n"), "a@trakzenhq.com").as_deref(), Some("mail.office.example.com"));
        assert_eq!(pick_ehlo_name(Some("DESKTOP-4F2K9"), "a@trakzenhq.com").as_deref(), Some("trakzenhq.com"));
        assert_eq!(pick_ehlo_name(Some("macbook.local"), "a@TrakzenHQ.com").as_deref(), Some("trakzenhq.com"));
        assert_eq!(pick_ehlo_name(Some("10.0.0.7"), "a@trakzenhq.com").as_deref(), Some("trakzenhq.com"));
        assert_eq!(pick_ehlo_name(None, "a@trakzenhq.com").as_deref(), Some("trakzenhq.com"));
        assert_eq!(pick_ehlo_name(None, "nobody"), None);
    }

    #[test]
    fn labels_come_from_the_folder_and_flags() {
        use ::imap::types::Flag;
        let (labels, read, starred) = labels_for(Role::Sent, &[Flag::Seen]);
        assert_eq!((labels, read, starred), (vec!["SENT".to_string()], true, false));
        let (labels, read, _) = labels_for(Role::Sent, &[]);
        assert_eq!((labels, read), (vec!["SENT".to_string(), "UNREAD".to_string()], false));
        let (labels, _, starred) = labels_for(Role::Archive, &[Flag::Seen, Flag::Flagged]);
        assert_eq!((labels, starred), (vec!["STARRED".to_string()], true));
        let (labels, _, _) = labels_for(Role::Junk, &[Flag::Seen]);
        assert_eq!(labels, vec!["SPAM"]);
    }

    #[test]
    fn headers_fill_in_a_message_row() {
        use ::imap::types::Flag;
        let header = b"From: \"Rabiya Gull\" <rabiya@example.com>\r\nTo: me@example.com\r\nSubject: Q3 numbers\r\nDate: Tue, 14 Oct 2026 09:00:00 +0000\r\nMessage-ID: <abc@example.com>\r\nReferences: <root@example.com>\r\n\r\n";
        let id = folders::remote_id("Sent Items", 7, 31);
        let m = message_from_header(id.clone(), Role::Inbox, &[Flag::Seen], None, header);
        assert_eq!(m.remote_id, id);
        assert_eq!(m.subject, "Q3 numbers");
        assert_eq!(m.from_addr, "rabiya@example.com");
        assert_eq!(m.from_name, "Rabiya Gull");
        assert_eq!(m.message_id_hdr.as_deref(), Some("<abc@example.com>"));
        assert_eq!(m.references_hdr.as_deref(), Some("<root@example.com>"));
        assert_eq!(m.date, 1_791_968_400_000);
        assert_eq!(m.labels, vec!["INBOX"]);
        // The server's arrival time wins over a sender-supplied Date.
        let m = message_from_header(id, Role::Inbox, &[], Some(5), header);
        assert_eq!(m.date, 5);
        assert!(!m.is_read);
    }

    #[test]
    fn old_configs_without_security_still_parse() {
        let raw = r#"{"host":"imap.x","port":993,"smtp_host":"smtp.x","smtp_port":465,"username":"u"}"#;
        let c: ImapConfig = serde_json::from_str(raw).unwrap();
        assert_eq!(c.smtp_security, None);
        assert!(c.smtp_implicit_tls());
    }
}
