//! Builds outgoing RFC 5322 messages and pre-fills reply / forward drafts.
//! Provider-independent: the result is raw bytes any provider can submit.

use chrono::{Local, TimeZone};
use lettre::message::{header::ContentType, Attachment, Mailbox, Message, MultiPart};

use crate::error::{AppError, Result};

use super::sanitize;
use super::types::*;

pub struct ResolvedAttachment {
    pub filename: String,
    pub mime_type: String,
    pub data: Vec<u8>,
}

fn mailbox(s: &str) -> Result<Mailbox> {
    s.trim()
        .parse::<Mailbox>()
        .map_err(|e| AppError::Other(format!("invalid address '{s}': {e}")))
}

/// Drafts may have no recipients yet; skip anything unparsable instead of
/// refusing to save.
pub fn build_raw_lenient(
    account: &Account,
    msg: &OutgoingMessage,
    attachments: Vec<ResolvedAttachment>,
) -> Result<Vec<u8>> {
    let clean = |v: &Vec<String>| -> Vec<String> {
        v.iter().filter(|a| mailbox(a).is_ok()).cloned().collect()
    };
    let lenient = OutgoingMessage {
        to: clean(&msg.to),
        cc: clean(&msg.cc),
        bcc: clean(&msg.bcc),
        ..msg.clone()
    };
    build_raw(account, &lenient, attachments)
}

pub fn build_raw(
    account: &Account,
    msg: &OutgoingMessage,
    attachments: Vec<ResolvedAttachment>,
) -> Result<Vec<u8>> {
    let from = match &account.display_name {
        Some(n) if !n.is_empty() => format!("{} <{}>", n.replace('"', ""), account.email),
        _ => account.email.clone(),
    };

    let mut b = Message::builder().from(mailbox(&from)?).subject(&msg.subject);
    for t in msg.to.iter().filter(|s| !s.trim().is_empty()) {
        b = b.to(mailbox(t)?);
    }
    for c in msg.cc.iter().filter(|s| !s.trim().is_empty()) {
        b = b.cc(mailbox(c)?);
    }
    for c in msg.bcc.iter().filter(|s| !s.trim().is_empty()) {
        b = b.bcc(mailbox(c)?);
    }
    if let Some(irt) = &msg.in_reply_to {
        b = b.in_reply_to(irt.clone());
    }
    if let Some(refs) = &msg.references {
        b = b.references(refs.clone());
    }

    let mut html = format!(
        "<div style=\"font-family:sans-serif;font-size:14px\">{}</div>",
        sanitize::text_to_html(&msg.body_text)
    );
    let mut text = msg.body_text.clone();
    if let Some(q) = &msg.quoted_html {
        html.push_str("<br>");
        html.push_str(q);
        text.push_str("\n\n");
        text.push_str(&sanitize::html_to_text(q));
    }

    let alternative = MultiPart::alternative_plain_html(text, html);
    let built = if attachments.is_empty() {
        b.multipart(alternative)?
    } else {
        let mut mixed = MultiPart::mixed().multipart(alternative);
        for a in attachments {
            let ct = ContentType::parse(&a.mime_type)
                .unwrap_or_else(|_| ContentType::parse("application/octet-stream").unwrap());
            mixed = mixed.singlepart(Attachment::new(a.filename).body(a.data, ct));
        }
        b.multipart(mixed)?
    };
    Ok(built.formatted())
}

/// Splits a header address list on commas, honouring quoted display names.
pub fn split_addresses(list: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    for c in list.chars() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
                cur.push(c);
            }
            ',' if !in_quotes => {
                let t = cur.trim();
                if !t.is_empty() {
                    out.push(t.to_string());
                }
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    let t = cur.trim();
    if !t.is_empty() {
        out.push(t.to_string());
    }
    out
}

pub fn address_part(s: &str) -> String {
    match (s.find('<'), s.rfind('>')) {
        (Some(a), Some(b)) if b > a => s[a + 1..b].trim().to_ascii_lowercase(),
        _ => s.trim().trim_matches('"').to_ascii_lowercase(),
    }
}

fn format_from(m: &MessageSummary) -> String {
    if m.from_name.is_empty() || m.from_name == m.from_addr {
        m.from_addr.clone()
    } else {
        format!("{} <{}>", m.from_name, m.from_addr)
    }
}

fn prefixed(subject: &str, prefix: &str, existing: &[&str]) -> String {
    let lower = subject.trim().to_ascii_lowercase();
    if existing.iter().any(|p| lower.starts_with(p)) {
        subject.trim().to_string()
    } else {
        format!("{prefix} {}", subject.trim())
    }
}

fn format_date(ms: i64) -> String {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|d| d.format("%a, %-d %b %Y at %H:%M").to_string())
        .unwrap_or_default()
}

pub fn draft_for(account: &Account, msg: &MessageDetail, mode: ReplyMode) -> ComposeDraft {
    let s = &msg.summary;
    let me = account.email.to_ascii_lowercase();
    let not_me = |a: &String| address_part(a) != me;

    let body_html = msg
        .body_html
        .clone()
        .or_else(|| msg.body_text.as_deref().map(sanitize::text_to_html))
        .unwrap_or_default();
    let body_text = msg
        .body_text
        .clone()
        .or_else(|| msg.body_html.as_deref().map(sanitize::html_to_text))
        .unwrap_or_default();

    let (to, cc, subject, quoted_html, quoted_text, in_reply_to, references, thread_id) =
        match mode {
            ReplyMode::Reply | ReplyMode::ReplyAll => {
                let mut to = if address_part(&s.from_addr) == me {
                    split_addresses(&s.to_addrs)
                } else {
                    vec![format_from(s)]
                };
                let mut cc = Vec::new();
                if mode == ReplyMode::ReplyAll {
                    to.extend(split_addresses(&s.to_addrs).into_iter().filter(not_me));
                    cc.extend(split_addresses(&s.cc_addrs).into_iter().filter(not_me));
                    to.dedup_by_key(|a| address_part(a));
                    cc.retain(|c| !to.iter().any(|t| address_part(t) == address_part(c)));
                }
                let header = format!(
                    "On {}, {} wrote:",
                    format_date(s.date),
                    sanitize::escape(&format_from(s))
                );
                let quoted_html = format!(
                    "<div>{header}</div><blockquote style=\"margin:0 0 0 .8ex;border-left:1px solid #ccc;padding-left:1ex\">{body_html}</blockquote>"
                );
                let quoted_text = body_text
                    .lines()
                    .map(|l| format!("> {l}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                let refs = match (&msg.references_hdr, &msg.message_id_hdr) {
                    (Some(r), Some(m)) => Some(format!("{r} {m}")),
                    (None, Some(m)) => Some(m.clone()),
                    _ => None,
                };
                (
                    to,
                    cc,
                    prefixed(&s.subject, "Re:", &["re:"]),
                    quoted_html,
                    quoted_text,
                    msg.message_id_hdr.clone(),
                    refs,
                    s.thread_id.clone(),
                )
            }
            ReplyMode::Forward => {
                let header = format!(
                    "---------- Forwarded message ---------<br>From: {}<br>Date: {}<br>Subject: {}<br>To: {}",
                    sanitize::escape(&format_from(s)),
                    format_date(s.date),
                    sanitize::escape(&s.subject),
                    sanitize::escape(&s.to_addrs)
                );
                let quoted_html = format!("<div>{header}</div><br>{body_html}");
                let quoted_text = format!(
                    "---------- Forwarded message ---------\nFrom: {}\nDate: {}\nSubject: {}\nTo: {}\n\n{}",
                    format_from(s),
                    format_date(s.date),
                    s.subject,
                    s.to_addrs,
                    body_text
                );
                (
                    Vec::new(),
                    Vec::new(),
                    prefixed(&s.subject, "Fwd:", &["fwd:", "fw:"]),
                    quoted_html,
                    quoted_text,
                    None,
                    None,
                    None,
                )
            }
        };

    let (attachments, attachment_names) = if mode == ReplyMode::Forward {
        (
            msg.attachments
                .iter()
                .map(|a| OutgoingAttachment::Stored {
                    message_id: s.id,
                    attachment_id: a.id,
                })
                .collect(),
            msg.attachments.iter().map(|a| a.filename.clone()).collect(),
        )
    } else {
        (Vec::new(), Vec::new())
    };

    ComposeDraft {
        account_id: account.id,
        to,
        cc,
        subject,
        quoted_html: Some(quoted_html),
        quoted_text,
        in_reply_to,
        references,
        thread_id,
        attachments,
        attachment_names,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account() -> Account {
        Account {
            id: 1,
            provider: "gmail".into(),
            email: "me@example.com".into(),
            display_name: Some("Me".into()),
            sync_cursor: None,
        }
    }

    fn message() -> MessageDetail {
        MessageDetail {
            summary: MessageSummary {
                id: 10,
                account_id: 1,
                remote_id: "r1".into(),
                thread_id: Some("t1".into()),
                subject: "Budget".into(),
                from_name: "Alice".into(),
                from_addr: "alice@example.com".into(),
                to_addrs: "Me <me@example.com>, \"Smith, Bob\" <bob@example.com>".into(),
                cc_addrs: "carol@example.com".into(),
                snippet: String::new(),
                date: 1_700_000_000_000,
                labels: vec!["INBOX".into()],
                is_read: true,
                is_starred: false,
                has_attachments: true,
                thread_count: 1,
                thread_unread: 0,
            },
            body_html: Some("<p>numbers</p>".into()),
            body_text: Some("numbers".into()),
            message_id_hdr: Some("<m1@example.com>".into()),
            references_hdr: Some("<m0@example.com>".into()),
            attachments: vec![AttachmentInfo {
                id: 5,
                remote_id: "a".into(),
                filename: "q3.xlsx".into(),
                mime_type: "application/vnd.ms-excel".into(),
                size: 10,
            }],
            can_unsubscribe: false,
            invite: None,
        }
    }

    #[test]
    fn split_addresses_respects_quoted_commas() {
        let parts = split_addresses("Me <me@example.com>, \"Smith, Bob\" <bob@example.com>");
        assert_eq!(parts.len(), 2);
        assert_eq!(address_part(&parts[1]), "bob@example.com");
    }

    #[test]
    fn reply_targets_sender_and_threads() {
        let d = draft_for(&account(), &message(), ReplyMode::Reply);
        assert_eq!(d.to, vec!["Alice <alice@example.com>"]);
        assert!(d.cc.is_empty());
        assert_eq!(d.subject, "Re: Budget");
        assert_eq!(d.in_reply_to.as_deref(), Some("<m1@example.com>"));
        assert_eq!(d.references.as_deref(), Some("<m0@example.com> <m1@example.com>"));
        assert_eq!(d.thread_id.as_deref(), Some("t1"));
        assert!(d.attachments.is_empty());
    }

    #[test]
    fn reply_all_drops_self_and_keeps_others() {
        let d = draft_for(&account(), &message(), ReplyMode::ReplyAll);
        let to: Vec<String> = d.to.iter().map(|a| address_part(a)).collect();
        assert_eq!(to, vec!["alice@example.com", "bob@example.com"]);
        assert_eq!(d.cc, vec!["carol@example.com"]);
    }

    #[test]
    fn forward_carries_attachments_and_no_threading() {
        let d = draft_for(&account(), &message(), ReplyMode::Forward);
        assert!(d.to.is_empty());
        assert_eq!(d.subject, "Fwd: Budget");
        assert!(d.in_reply_to.is_none());
        assert!(d.thread_id.is_none());
        assert_eq!(d.attachment_names, vec!["q3.xlsx"]);
        assert!(d.quoted_text.contains("Forwarded message"));
    }

    #[test]
    fn subject_prefix_is_not_doubled() {
        assert_eq!(prefixed("RE: hi", "Re:", &["re:"]), "RE: hi");
        assert_eq!(prefixed("hi", "Fwd:", &["fwd:", "fw:"]), "Fwd: hi");
    }

    #[test]
    fn build_raw_produces_multipart_with_headers() {
        let msg = OutgoingMessage {
            account_id: 1,
            to: vec!["alice@example.com".into()],
            cc: vec![],
            bcc: vec![],
            subject: "Re: Budget".into(),
            body_text: "Looks good.".into(),
            quoted_html: Some("<blockquote>numbers</blockquote>".into()),
            in_reply_to: Some("<m1@example.com>".into()),
            references: None,
            thread_id: None,
            attachments: vec![],
            draft_id: None,
        };
        let raw = build_raw(&account(), &msg, vec![]).unwrap();
        let text = String::from_utf8_lossy(&raw);
        assert!(text.contains("From: \"Me\" <me@example.com>") || text.contains("From: Me <me@example.com>"));
        assert!(text.contains("In-Reply-To: <m1@example.com>"));
        assert!(text.contains("multipart/alternative"));
        assert!(text.contains("Looks good."));
    }
}
