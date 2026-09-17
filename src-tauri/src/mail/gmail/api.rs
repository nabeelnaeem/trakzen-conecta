//! Thin Gmail REST client plus the JSON → domain-type mapping.

use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use base64::{alphabet, Engine};
use serde::Deserialize;
use serde_json::Value;

use crate::error::{AppError, Result};
use crate::mail::types::*;

const BASE: &str = "https://gmail.googleapis.com/gmail/v1/users/me";

// Gmail sometimes pads its URL-safe base64 and sometimes does not.
pub static B64: GeneralPurpose = GeneralPurpose::new(
    &alphabet::URL_SAFE,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

pub struct GmailApi {
    http: reqwest::Client,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub email_address: String,
    pub history_id: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct IdList {
    #[serde(default)]
    pub messages: Vec<IdRef>,
    pub next_page_token: Option<String>,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct IdRef {
    pub id: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPage {
    #[serde(default)]
    pub history: Vec<Value>,
    pub next_page_token: Option<String>,
    pub history_id: Option<String>,
}

pub enum ApiError {
    NotFound,
    Other(AppError),
}

impl GmailApi {
    pub fn new(http: reqwest::Client) -> Self {
        Self { http }
    }

    async fn get_json(&self, token: &str, path: &str, query: &[(&str, &str)]) -> Result<Value> {
        self.get_json_raw(token, path, query)
            .await
            .map_err(|e| match e {
                ApiError::NotFound => AppError::NotFound(path.to_string()),
                ApiError::Other(e) => e,
            })
    }

    async fn get_json_raw(
        &self,
        token: &str,
        path: &str,
        query: &[(&str, &str)],
    ) -> std::result::Result<Value, ApiError> {
        let res = self
            .http
            .get(format!("{BASE}/{path}"))
            .bearer_auth(token)
            .query(query)
            .send()
            .await
            .map_err(|e| ApiError::Other(e.into()))?;
        match res.status().as_u16() {
            200 => res.json().await.map_err(|e| ApiError::Other(e.into())),
            404 => Err(ApiError::NotFound),
            status => {
                let body = res.text().await.unwrap_or_default();
                Err(ApiError::Other(provider_error(status, &body)))
            }
        }
    }

    async fn post_json(&self, token: &str, path: &str, body: &Value) -> Result<Value> {
        let res = self
            .http
            .post(format!("{BASE}/{path}"))
            .bearer_auth(token)
            .json(body)
            .send()
            .await?;
        let status = res.status().as_u16();
        if (200..300).contains(&status) {
            Ok(res.json().await.unwrap_or(Value::Null))
        } else {
            let body = res.text().await.unwrap_or_default();
            Err(provider_error(status, &body))
        }
    }

    pub async fn profile(&self, token: &str) -> Result<Profile> {
        Ok(serde_json::from_value(self.get_json(token, "profile", &[]).await?)?)
    }

    pub async fn list_ids(
        &self,
        token: &str,
        query: Option<&str>,
        max: u32,
        page_token: Option<&str>,
    ) -> Result<IdList> {
        let max = max.to_string();
        let mut q = vec![("maxResults", max.as_str())];
        if let Some(query) = query {
            q.push(("q", query));
        }
        if let Some(pt) = page_token {
            q.push(("pageToken", pt));
        }
        Ok(serde_json::from_value(self.get_json(token, "messages", &q).await?)?)
    }

    /// Metadata only: headers, labels, snippet. Returns `None` when the
    /// message no longer exists (deleted between list and get).
    pub async fn get_metadata(&self, token: &str, id: &str) -> Result<Option<RemoteMessage>> {
        let q = [
            ("format", "metadata"),
            ("metadataHeaders", "From"),
            ("metadataHeaders", "To"),
            ("metadataHeaders", "Cc"),
            ("metadataHeaders", "Subject"),
            ("metadataHeaders", "Message-ID"),
            ("metadataHeaders", "References"),
        ];
        match self.get_json_raw(token, &format!("messages/{id}"), &q).await {
            Ok(v) => Ok(Some(parse_metadata(&v))),
            Err(ApiError::NotFound) => Ok(None),
            Err(ApiError::Other(e)) => Err(e),
        }
    }

    pub async fn get_full(&self, token: &str, id: &str) -> Result<RemoteBody> {
        let v = self
            .get_json(token, &format!("messages/{id}"), &[("format", "full")])
            .await?;
        Ok(parse_body(&v))
    }

    pub async fn get_attachment(&self, token: &str, msg_id: &str, att_id: &str) -> Result<Vec<u8>> {
        let v = self
            .get_json(token, &format!("messages/{msg_id}/attachments/{att_id}"), &[])
            .await?;
        let data = v["data"].as_str().unwrap_or_default();
        B64.decode(data)
            .map_err(|e| AppError::Provider(format!("bad attachment encoding: {e}")))
    }

    pub async fn history(
        &self,
        token: &str,
        start: &str,
        page_token: Option<&str>,
    ) -> std::result::Result<HistoryPage, ApiError> {
        let mut q = vec![
            ("startHistoryId", start),
            ("maxResults", "500"),
            ("historyTypes", "messageAdded"),
            ("historyTypes", "messageDeleted"),
            ("historyTypes", "labelAdded"),
            ("historyTypes", "labelRemoved"),
        ];
        if let Some(pt) = page_token {
            q.push(("pageToken", pt));
        }
        let v = self.get_json_raw(token, "history", &q).await?;
        serde_json::from_value(v).map_err(|e| ApiError::Other(e.into()))
    }

    pub async fn send(&self, token: &str, raw: &[u8], thread_id: Option<&str>) -> Result<()> {
        let mut body = serde_json::json!({ "raw": B64.encode(raw) });
        if let Some(t) = thread_id {
            body["threadId"] = Value::String(t.to_string());
        }
        self.post_json(token, "messages/send", &body).await?;
        Ok(())
    }

    pub async fn modify_labels(
        &self,
        token: &str,
        id: &str,
        add: &[&str],
        remove: &[&str],
    ) -> Result<()> {
        let body = serde_json::json!({ "addLabelIds": add, "removeLabelIds": remove });
        self.post_json(token, &format!("messages/{id}/modify"), &body)
            .await?;
        Ok(())
    }

    pub async fn trash(&self, token: &str, id: &str) -> Result<()> {
        self.post_json(token, &format!("messages/{id}/trash"), &Value::Null)
            .await?;
        Ok(())
    }
}

fn provider_error(status: u16, body: &str) -> AppError {
    let msg = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
        .unwrap_or_else(|| body.chars().take(300).collect());
    match status {
        401 => AppError::Auth(format!("Gmail rejected the token: {msg}")),
        _ => AppError::Provider(format!("Gmail HTTP {status}: {msg}")),
    }
}

// ---- JSON mapping ---------------------------------------------------------

fn header<'a>(payload: &'a Value, name: &str) -> Option<&'a str> {
    payload["headers"]
        .as_array()?
        .iter()
        .find(|h| h["name"].as_str().map_or(false, |n| n.eq_ignore_ascii_case(name)))
        .and_then(|h| h["value"].as_str())
}

/// "Display Name <addr@host>" → (name, addr). Falls back to using the
/// address as the name when there is none.
pub fn split_mailbox(s: &str) -> (String, String) {
    let s = s.trim();
    match (s.find('<'), s.rfind('>')) {
        (Some(a), Some(b)) if b > a => {
            let addr = s[a + 1..b].trim().to_string();
            let name = s[..a].trim().trim_matches('"').trim().to_string();
            if name.is_empty() {
                (addr.clone(), addr)
            } else {
                (name, addr)
            }
        }
        _ => (s.to_string(), s.to_string()),
    }
}

pub fn parse_metadata(v: &Value) -> RemoteMessage {
    let payload = &v["payload"];
    let labels: Vec<String> = v["labelIds"]
        .as_array()
        .map(|a| a.iter().filter_map(|l| l.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let (from_name, from_addr) = split_mailbox(header(payload, "From").unwrap_or_default());
    RemoteMessage {
        remote_id: v["id"].as_str().unwrap_or_default().to_string(),
        thread_id: v["threadId"].as_str().map(str::to_string),
        subject: header(payload, "Subject").unwrap_or_default().to_string(),
        from_name,
        from_addr,
        to_addrs: header(payload, "To").unwrap_or_default().to_string(),
        cc_addrs: header(payload, "Cc").unwrap_or_default().to_string(),
        snippet: decode_entities(v["snippet"].as_str().unwrap_or_default()),
        date: v["internalDate"]
            .as_str()
            .and_then(|d| d.parse().ok())
            .unwrap_or(0),
        is_read: !labels.iter().any(|l| l == "UNREAD"),
        is_starred: labels.iter().any(|l| l == "STARRED"),
        has_attachments: false,
        message_id_hdr: header(payload, "Message-ID").map(str::to_string),
        references_hdr: header(payload, "References").map(str::to_string),
        labels,
    }
}

pub fn parse_body(v: &Value) -> RemoteBody {
    let mut out = RemoteBody::default();
    walk_part(&v["payload"], &mut out);
    out
}

fn walk_part(part: &Value, out: &mut RemoteBody) {
    let mime = part["mimeType"].as_str().unwrap_or_default();
    let filename = part["filename"].as_str().unwrap_or_default();
    let body = &part["body"];

    if !filename.is_empty() {
        if let Some(att_id) = body["attachmentId"].as_str() {
            out.attachments.push(RemoteAttachment {
                remote_id: att_id.to_string(),
                filename: filename.to_string(),
                mime_type: if mime.is_empty() {
                    "application/octet-stream".into()
                } else {
                    mime.to_string()
                },
                size: body["size"].as_i64().unwrap_or(0),
            });
        }
    } else if let Some(data) = body["data"].as_str() {
        let decoded = B64
            .decode(data)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default();
        match mime {
            "text/html" => out.html.get_or_insert_with(String::new).push_str(&decoded),
            "text/plain" => out.text.get_or_insert_with(String::new).push_str(&decoded),
            _ => {}
        }
    }

    if let Some(parts) = part["parts"].as_array() {
        for p in parts {
            walk_part(p, out);
        }
    }
}

/// Gmail's snippet field is HTML-escaped.
fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}
