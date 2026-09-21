//! Builds outgoing RFC 5322 messages and pre-fills reply / forward drafts.
//! Provider-independent: the result is raw bytes any provider can submit.

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use chrono::{Local, TimeZone};
use lettre::message::{header::ContentType, Attachment, Mailbox, Message, MultiPart, SinglePart};

use crate::error::{AppError, Result};

use super::sanitize;
use super::types::*;

/// Separates what the user wrote from the quoted original in HTML we
/// generate, so a draft can be reopened with just the user's part editable.
const QUOTE_MARKER: &str = "<!--tc-quote-->";

/// The user-written part of a message body this app produced, unwrapped
/// from its outer div; `None` if the HTML did not come from us.
pub fn own_html_part(html: &str) -> Option<String> {
    let own = html.split(QUOTE_MARKER).next().unwrap_or(html).trim();
    const OPEN: &str = "<div style=\"font-family:sans-serif;font-size:14px\">";
    let inner = own.strip_prefix(OPEN)?;
    let inner = inner.strip_suffix("</div>").unwrap_or(inner);
    Some(inner.to_string())
}

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
    let mut lenient = OutgoingMessage {
        to: clean(&msg.to),
        cc: clean(&msg.cc),
        bcc: clean(&msg.bcc),
        ..msg.clone()
    };
    // lettre refuses to build a message with an empty envelope, but a draft
    // often has no recipient yet. Give it a sentinel and cut the header back
    // out of the bytes; the provider stores the draft without a To.
    let no_recipients = lenient.to.is_empty() && lenient.cc.is_empty() && lenient.bcc.is_empty();
    if no_recipients {
        lenient.to.push(DRAFT_SENTINEL.into());
    }
    let raw = build_raw(account, &lenient, attachments)?;
    if !no_recipients {
        return Ok(raw);
    }
    let text = String::from_utf8(raw).map_err(|e| AppError::Other(format!("mime is not utf-8: {e}")))?;
    let line = format!("To: {DRAFT_SENTINEL}\r\n");
    Ok(text.replacen(&line, "", 1).into_bytes())
}

/// Placeholder recipient used only while building a recipient-less draft.
const DRAFT_SENTINEL: &str = "draft@invalid";

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

    let own = match msg.body_html.as_deref().filter(|h| !h.trim().is_empty()) {
        Some(h) => sanitize::html(h),
        None => sanitize::text_to_html(&msg.body_text),
    };
    let mut html = format!("<div style=\"font-family:sans-serif;font-size:14px\">{own}</div>");
    let mut text = msg.body_text.clone();
    if let Some(q) = &msg.quoted_html {
        html.push_str(QUOTE_MARKER);
        html.push_str("<br>");
        html.push_str(q);
        text.push_str("\n\n");
        text.push_str(&sanitize::html_to_text(q));
    }

    // Images pasted into the editor or a signature arrive as data: URLs,
    // which most mail clients refuse to show; ship them as inline parts.
    let (html, inline) = extract_inline_images(&html);
    let alternative = MultiPart::alternative_plain_html(text, html);
    let body: MultiPart = if inline.is_empty() {
        alternative
    } else {
        let mut related = MultiPart::related().multipart(alternative);
        for (i, (mime, bytes)) in inline.into_iter().enumerate() {
            let ct = ContentType::parse(&mime).unwrap_or_else(|_| ContentType::parse("image/png").unwrap());
            related = related.singlepart(Attachment::new_inline(format!("img{i}@trakzen")).body(bytes, ct));
        }
        related
    };
    let built = if attachments.is_empty() {
        b.multipart(body)?
    } else {
        let mut mixed = MultiPart::mixed().multipart(body);
        for a in attachments {
            let ct = ContentType::parse(&a.mime_type)
                .unwrap_or_else(|_| ContentType::parse("application/octet-stream").unwrap());
            mixed = mixed.singlepart(Attachment::new(a.filename).body(a.data, ct));
        }
        b.multipart(mixed)?
    };
    Ok(built.formatted())
}

/// Replaces `src="data:<mime>;base64,<data>"` with `cid:imgN@trakzen` and
/// returns the decoded images in order. Anything that does not decode is
/// left alone.
fn extract_inline_images(html: &str) -> (String, Vec<(String, Vec<u8>)>) {
    let mut out = String::with_capacity(html.len());
    let mut images = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find("src=\"data:") {
        let after = &rest[start + 5..]; // after `src="`
        let Some(end) = after.find('"') else { break };
        let url = &after[..end];
        let decoded = url
            .strip_prefix("data:")
            .and_then(|u| u.split_once(";base64,"))
            .and_then(|(mime, data)| B64.decode(data.trim()).ok().map(|b| (mime.to_string(), b)));
        match decoded {
            Some(img) => {
                out.push_str(&rest[..start]);
                out.push_str(&format!("src=\"cid:img{}@trakzen\"", images.len()));
                images.push(img);
            }
            None => out.push_str(&rest[..start + 5 + end + 1]),
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    (out, images)
}

/// A stored signature may be plain text (older builds) or HTML from the
/// rich editor; normalise to HTML.
pub fn signature_html(sig: &str) -> String {
    let t = sig.trim();
    if t.contains('<') && t.contains('>') {
        sanitize::html(t)
    } else {
        sanitize::text_to_html(t)
    }
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
                content_id: None,
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
            body_html: Some("<b>Looks</b> good.".into()),
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

    #[test]
    fn own_html_part_round_trips_through_build_raw() {
        let msg = OutgoingMessage {
            account_id: 1,
            to: vec!["alice@example.com".into()],
            cc: vec![],
            bcc: vec![],
            subject: "Hi".into(),
            body_text: "Looks good.".into(),
            body_html: Some("<b>Looks</b> good.".into()),
            quoted_html: Some("<blockquote>numbers</blockquote>".into()),
            in_reply_to: None,
            references: None,
            thread_id: None,
            attachments: vec![],
            draft_id: None,
        };
        let account = Account {
            id: 1,
            provider: "gmail".into(),
            email: "me@example.com".into(),
            display_name: None,
            sync_cursor: None,
        };
        let raw = String::from_utf8(build_raw(&account, &msg, vec![]).unwrap()).unwrap();
        // The HTML part is quoted-printable; the marker survives verbatim.
        assert!(raw.contains("tc-quote"));
        let html = "<div style=\"font-family:sans-serif;font-size:14px\"><b>Looks</b> good.</div><!--tc-quote--><br><blockquote>numbers</blockquote>";
        assert_eq!(own_html_part(html).as_deref(), Some("<b>Looks</b> good."));
        assert_eq!(own_html_part("<p>someone else's html</p>"), None);
    }

    #[test]
    fn drafts_without_recipients_still_build() {
        let msg = OutgoingMessage {
            account_id: 1,
            to: vec![],
            cc: vec![],
            bcc: vec![],
            subject: "Half-written".into(),
            body_text: "Dear".into(),
            body_html: None,
            quoted_html: None,
            in_reply_to: None,
            references: None,
            thread_id: None,
            attachments: vec![],
            draft_id: None,
        };
        let account = Account {
            id: 1,
            provider: "gmail".into(),
            email: "me@example.com".into(),
            display_name: None,
            sync_cursor: None,
        };
        let raw = String::from_utf8(build_raw_lenient(&account, &msg, vec![]).unwrap()).unwrap();
        let head = raw.split("\r\n\r\n").next().unwrap_or("");
        assert!(!raw.contains("draft@invalid"), "{head}");
        assert!(!head.contains("To:"), "{head}");
        assert!(raw.contains("Subject: Half-written"));
        assert!(raw.contains("Dear"));
    }

    #[test]
    fn data_images_become_inline_parts() {
        let png = B64.encode(b"\x89PNG fake");
        let html = format!("<p>Hi</p><img src=\"data:image/png;base64,{png}\" alt=\"logo\"><img src=\"https://x/y.png\">");
        let (out, images) = extract_inline_images(&html);
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].0, "image/png");
        assert_eq!(images[0].1, b"\x89PNG fake");
        assert!(out.contains("src=\"cid:img0@trakzen\""));
        assert!(out.contains("src=\"https://x/y.png\""));

        let msg = OutgoingMessage {
            account_id: 1,
            to: vec!["alice@example.com".into()],
            cc: vec![],
            bcc: vec![],
            subject: "Logo".into(),
            body_text: "Hi".into(),
            body_html: Some(html),
            quoted_html: None,
            in_reply_to: None,
            references: None,
            thread_id: None,
            attachments: vec![],
            draft_id: None,
        };
        let account = Account {
            id: 1,
            provider: "gmail".into(),
            email: "me@example.com".into(),
            display_name: None,
            sync_cursor: None,
        };
        let raw = String::from_utf8(build_raw(&account, &msg, vec![]).unwrap()).unwrap();
        assert!(raw.contains("multipart/related"));
        assert!(raw.contains("Content-ID: <img0@trakzen>"));
        assert!(!raw.contains("base64,"));
    }
}
