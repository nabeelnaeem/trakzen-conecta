//! Minimal iCalendar (RFC 5545) reader for Accept / Decline on invites.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarInvite {
    pub summary: String,
    pub when: Option<String>,
    pub organizer: Option<String>,
    pub method: String,
    pub uid: Option<String>,
}

pub fn parse(ics: &str) -> Option<CalendarInvite> {
    if !ics.to_ascii_uppercase().contains("BEGIN:VEVENT") {
        return None;
    }
    let unfolded = unfold(ics);
    let summary = prop(&unfolded, "SUMMARY").unwrap_or_else(|| "(no title)".into());
    let dtstart = prop(&unfolded, "DTSTART");
    let when = dtstart.map(|v| humanize_dt(&v));
    let organizer = prop(&unfolded, "ORGANIZER").map(mailto_addr);
    let method = prop(&unfolded, "METHOD").unwrap_or_else(|| "REQUEST".into());
    let uid = prop(&unfolded, "UID");
    Some(CalendarInvite {
        summary,
        when,
        organizer,
        method,
        uid,
    })
}

/// Reply payload the composer can send as `text/calendar`.
pub fn reply(original: &str, accepted: bool, attendee: &str) -> String {
    let unfolded = unfold(original);
    let uid = prop(&unfolded, "UID").unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let summary = prop(&unfolded, "SUMMARY").unwrap_or_default();
    let dtstart = prop(&unfolded, "DTSTART").unwrap_or_default();
    let dtend = prop(&unfolded, "DTEND").unwrap_or_default();
    let status = if accepted { "ACCEPTED" } else { "DECLINED" };
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Trakzen Conecta//EN\r\nMETHOD:REPLY\r\nBEGIN:VEVENT\r\nUID:{uid}\r\nSUMMARY:{summary}\r\nDTSTART:{dtstart}\r\nDTEND:{dtend}\r\nATTENDEE;PARTSTAT={status}:mailto:{attendee}\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
    )
}

fn unfold(s: &str) -> String {
    s.replace("\r\n ", "").replace("\n ", "").replace("\r\n\t", "").replace("\n\t", "")
}

fn prop(ics: &str, name: &str) -> Option<String> {
    let prefix = format!("{name}:");
    let prefix_param = format!("{name};");
    for line in ics.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(rest) = line.strip_prefix(&prefix) {
            return Some(unescape(rest));
        }
        if let Some(rest) = line.strip_prefix(&prefix_param) {
            if let Some((_, v)) = rest.split_once(':') {
                return Some(unescape(v));
            }
        }
    }
    None
}

fn unescape(s: &str) -> String {
    s.replace("\\n", "\n").replace("\\,", ",").replace("\\;", ";").replace("\\\\", "\\")
}

fn mailto_addr(s: String) -> String {
    s.trim()
        .trim_start_matches("mailto:")
        .trim_start_matches("MAILTO:")
        .to_string()
}

fn humanize_dt(v: &str) -> String {
    // 20240918T140000Z or 20240918
    if v.len() >= 15 && v.as_bytes().get(8) == Some(&b'T') {
        format!("{}-{}-{} {}:{} UTC", &v[0..4], &v[4..6], &v[6..8], &v[9..11], &v[11..13])
    } else if v.len() >= 8 {
        format!("{}-{}-{}", &v[0..4], &v[4..6], &v[6..8])
    } else {
        v.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_request() {
        let raw = "BEGIN:VCALENDAR\nMETHOD:REQUEST\nBEGIN:VEVENT\nUID:1\nSUMMARY:Standup\nDTSTART:20240918T140000Z\nORGANIZER:mailto:boss@example.com\nEND:VEVENT\nEND:VCALENDAR\n";
        let inv = parse(raw).unwrap();
        assert_eq!(inv.summary, "Standup");
        assert_eq!(inv.organizer.as_deref(), Some("boss@example.com"));
        assert!(inv.when.unwrap().contains("2024-09-18"));
    }
}
