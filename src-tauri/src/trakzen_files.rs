//! Trakzen Files: save chat files and mail attachments to the household file
//! server.
//!
//! Connecting uses the server's app-approval flow (authorisation code + PKCE
//! with a loopback redirect, like Gmail): the browser opens the server's
//! `/connect` page, the user approves, and the code is exchanged for a token
//! kept in the OS credential store. All requests go from Rust, because the
//! webview's CSP only allows IPC.
//!
//! Uploads use the server's resumable upload API and never overwrite: if the
//! name is taken the server answers `409 name_taken` with a suggested name,
//! which is passed to the UI so the user can pick another name or cancel.

use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_opener::OpenerExt;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::net::TcpListener;

use crate::error::{AppError, Result};
use crate::loopback::{pkce_challenge, random_urlsafe, wait_for_code};
use crate::{secrets, settings, AppState};

const SERVICE_TYPE: &str = "_trakzen-files._tcp.local.";
const APP_NAME: &str = "Trakzen Conecta";
const CHUNK: u64 = 8 * 1024 * 1024;
const MAX_RETRIES: u32 = 5;
pub const EVENT_UPLOAD: &str = "files://upload";

static HTTP: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .user_agent(concat!("trakzen-conecta/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .expect("http client")
});

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesStatus {
    connected: bool,
    url: Option<String>,
    username: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct FoundServer {
    name: String,
    url: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesEntry {
    name: String,
    path: String,
    is_dir: bool,
    kind: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum UploadSource {
    /// A file on this computer, e.g. one received in chat.
    #[serde(rename_all = "camelCase")]
    LocalFile { path: String },
    /// A mail attachment, fetched from the mail provider.
    #[serde(rename_all = "camelCase")]
    MailAttachment { attachment_id: i64 },
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum UploadOutcome {
    Done {
        path: String,
    },
    /// The name is used in that folder. `upload_id` is set when every byte
    /// already reached the server, so a rename finishes it without resending.
    #[serde(rename_all = "camelCase")]
    NameTaken {
        name: String,
        suggested: String,
        upload_id: Option<String>,
    },
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UploadProgress {
    key: String,
    sent: u64,
    total: u64,
}

/// `192.168.1.24:54380` → `http://192.168.1.24:54380`, no trailing slash.
fn normalize_url(raw: &str) -> Result<String> {
    let raw = raw.trim().trim_end_matches('/');
    if raw.is_empty() {
        return Err(AppError::Other("enter the server address".into()));
    }
    let with_scheme = if raw.contains("://") {
        raw.to_string()
    } else {
        format!("http://{raw}")
    };
    let parsed =
        url::Url::parse(&with_scheme).map_err(|_| AppError::Other(format!("'{raw}' is not a valid address")))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(AppError::Other(format!("'{raw}' is not a valid address")));
    }
    Ok(with_scheme.trim_end_matches('/').to_string())
}

fn device_name() -> String {
    std::env::var("COMPUTERNAME")
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "this computer".into())
}

struct Connection {
    url: String,
    token: String,
}

fn connection(state: &AppState) -> Result<Connection> {
    let url = settings::get(&state.db, settings::TRAKZEN_FILES_URL)?;
    let token = secrets::get(secrets::TRAKZEN_FILES_TOKEN)?;
    match (url, token) {
        (Some(url), Some(token)) => Ok(Connection { url, token }),
        _ => Err(AppError::NotConfigured(
            "connect Trakzen Files first (Settings → Trakzen Files)".into(),
        )),
    }
}

/// Turns a server reply into JSON, or into an error message the UI can show.
/// `409 name_taken` is returned as JSON so callers can offer a rename.
async fn read_json(resp: reqwest::Response) -> Result<Value> {
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    if status.is_success() || (status.as_u16() == 409 && body["name_taken"] == true) {
        return Ok(body);
    }
    let message = body["error"].as_str().unwrap_or("request failed").to_string();
    Err(match status.as_u16() {
        401 => AppError::Auth(
            "Trakzen Files no longer accepts this app. Connect again in Settings → Trakzen Files.".into(),
        ),
        404 => AppError::NotFound(message),
        _ => AppError::Provider(format!("Trakzen Files: {message}")),
    })
}

impl Connection {
    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        HTTP.request(method, format!("{}/api{path}", self.url))
            .bearer_auth(&self.token)
            // The server requires this on state-changing requests (CSRF guard).
            .header("x-trakzen", "1")
    }

    async fn get(&self, path: &str) -> Result<Value> {
        read_json(self.request(reqwest::Method::GET, path).send().await?).await
    }

    async fn post(&self, path: &str, body: Value) -> Result<Value> {
        read_json(self.request(reqwest::Method::POST, path).json(&body).send().await?).await
    }
}

// ---------------------------------------------------------------- connecting

#[tauri::command]
pub async fn trakzen_files_status(state: State<'_, AppState>, check: bool) -> Result<FilesStatus> {
    let url = settings::get(&state.db, settings::TRAKZEN_FILES_URL)?;
    let username = settings::get(&state.db, settings::TRAKZEN_FILES_USER)?;
    let has_token = secrets::get(secrets::TRAKZEN_FILES_TOKEN)?.is_some();
    let mut connected = url.is_some() && has_token;
    if connected && check {
        // A revoked token shows up as `user: null`; an unreachable server
        // is not treated as disconnected.
        if let Ok(conn) = connection(&state) {
            if let Ok(body) = conn.get("/auth/state").await {
                connected = !body["user"].is_null();
            }
        }
    }
    Ok(FilesStatus {
        connected,
        url,
        username,
    })
}

/// Looks for servers announcing themselves on the local network.
#[tauri::command]
pub async fn trakzen_files_discover() -> Result<Vec<FoundServer>> {
    tokio::task::spawn_blocking(|| {
        let daemon = mdns_sd::ServiceDaemon::new()
            .map_err(|e| AppError::Other(format!("network discovery unavailable: {e}")))?;
        let receiver = daemon
            .browse(SERVICE_TYPE)
            .map_err(|e| AppError::Other(format!("network discovery failed: {e}")))?;
        let deadline = Instant::now() + Duration::from_millis(2500);
        let mut found: Vec<FoundServer> = Vec::new();
        while let Ok(event) = receiver.recv_deadline(deadline) {
            if let mdns_sd::ServiceEvent::ServiceResolved(info) = event {
                let name = info
                    .get_fullname()
                    .split(&format!(".{SERVICE_TYPE}"))
                    .next()
                    .unwrap_or("Trakzen Files")
                    .replace("\\032", " ");
                let Some(ip) = info
                    .get_addresses()
                    .iter()
                    .find(|ip| matches!(ip, IpAddr::V4(v) if !v.is_loopback() && !v.is_link_local()))
                    .copied()
                else {
                    continue;
                };
                let url = format!("http://{ip}:{}", info.get_port());
                if !found.iter().any(|f| f.url == url) {
                    found.push(FoundServer { name, url });
                }
            }
        }
        let _ = daemon.shutdown();
        Ok(found)
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))?
}

/// Opens the server's approval page in the browser and waits for the user to
/// allow the connection.
#[tauri::command]
pub async fn trakzen_files_connect(app: AppHandle, state: State<'_, AppState>, url: String) -> Result<FilesStatus> {
    let url = normalize_url(&url)?;
    let probe = HTTP
        .get(format!("{url}/api/auth/state"))
        .send()
        .await
        .map_err(|_| AppError::Other(format!("could not reach {url}. Is the server on and on this network?")))?;
    let probe: Value = probe.json().await.unwrap_or(Value::Null);
    if probe.get("needs_setup").is_none() {
        return Err(AppError::Other(format!(
            "{url} doesn't look like a Trakzen Files server"
        )));
    }

    let verifier = random_urlsafe(64);
    let oauth_state = random_urlsafe(24);
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let redirect_uri = format!("http://127.0.0.1:{}/trakzen-files", listener.local_addr()?.port());
    let mut approve = url::Url::parse(&format!("{url}/connect")).map_err(|e| AppError::Other(e.to_string()))?;
    approve
        .query_pairs_mut()
        .append_pair("app", APP_NAME)
        .append_pair("device", &device_name())
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("code_challenge", &pkce_challenge(&verifier))
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", &oauth_state);
    app.opener()
        .open_url(approve.as_str(), None::<&str>)
        .map_err(|e| AppError::Other(format!("could not open the browser: {e}")))?;

    let code = tokio::time::timeout(
        Duration::from_secs(300),
        wait_for_code(listener, &oauth_state, "Trakzen Files"),
    )
    .await
    .map_err(|_| AppError::Auth("timed out waiting for approval in the browser".into()))??;

    let (token, username) = exchange_code(&url, &code, &verifier, &redirect_uri).await?;
    secrets::set(secrets::TRAKZEN_FILES_TOKEN, &token)?;
    settings::set(&state.db, settings::TRAKZEN_FILES_URL, &url)?;
    settings::set(&state.db, settings::TRAKZEN_FILES_USER, &username)?;
    Ok(FilesStatus {
        connected: true,
        url: Some(url),
        username: Some(username),
    })
}

/// Trades the one-time code from the approval redirect for a token and the
/// account's username.
async fn exchange_code(url: &str, code: &str, verifier: &str, redirect_uri: &str) -> Result<(String, String)> {
    let resp = HTTP
        .post(format!("{url}/api/apps/token"))
        .header("x-trakzen", "1")
        .json(&json!({ "code": code, "code_verifier": verifier, "redirect_uri": redirect_uri }))
        .send()
        .await?;
    let body = read_json(resp).await?;
    let token = body["token"]
        .as_str()
        .ok_or_else(|| AppError::Auth("the server did not return a token".into()))?
        .to_string();
    let username = body["user"]["username"].as_str().unwrap_or_default().to_string();
    Ok((token, username))
}

/// Forgets the connection here and asks the server to revoke the token.
#[tauri::command]
pub async fn trakzen_files_disconnect(state: State<'_, AppState>) -> Result<()> {
    if let Ok(conn) = connection(&state) {
        // Best effort: the server may be off; the token is still forgotten here.
        let _ = conn.request(reqwest::Method::DELETE, "/apps/self").send().await;
    }
    secrets::delete(secrets::TRAKZEN_FILES_TOKEN)?;
    settings::set(&state.db, settings::TRAKZEN_FILES_USER, "")?;
    Ok(())
}

// ---------------------------------------------------------------- browsing

#[tauri::command]
pub async fn trakzen_files_list(state: State<'_, AppState>, path: String) -> Result<Vec<FilesEntry>> {
    let conn = connection(&state)?;
    let body = conn
        .get(&format!(
            "/fs/list?path={}",
            url::form_urlencoded::byte_serialize(path.as_bytes()).collect::<String>()
        ))
        .await?;
    Ok(serde_json::from_value(body["entries"].clone())?)
}

#[tauri::command]
pub async fn trakzen_files_mkdir(state: State<'_, AppState>, path: String, name: String) -> Result<String> {
    let conn = connection(&state)?;
    let body = conn.post("/fs/mkdir", json!({ "path": path, "name": name })).await?;
    Ok(body["path"].as_str().unwrap_or_default().to_string())
}

// ---------------------------------------------------------------- uploading

enum Payload {
    File(PathBuf),
    Bytes(Vec<u8>),
}

impl Payload {
    async fn size(&self) -> Result<u64> {
        Ok(match self {
            Payload::File(p) => tokio::fs::metadata(p).await?.len(),
            Payload::Bytes(b) => b.len() as u64,
        })
    }

    async fn chunk(&self, offset: u64, len: u64) -> Result<Vec<u8>> {
        match self {
            Payload::File(p) => {
                let mut f = tokio::fs::File::open(p).await?;
                f.seek(std::io::SeekFrom::Start(offset)).await?;
                let mut buf = Vec::with_capacity(len as usize);
                f.take(len).read_to_end(&mut buf).await?;
                Ok(buf)
            }
            Payload::Bytes(b) => Ok(b[offset as usize..(offset + len) as usize].to_vec()),
        }
    }
}

async fn load_source(state: &AppState, source: &UploadSource) -> Result<(Payload, String, String)> {
    match source {
        UploadSource::LocalFile { path } => {
            let p = PathBuf::from(path);
            let meta = tokio::fs::metadata(&p)
                .await
                .map_err(|_| AppError::NotFound("the file is no longer on this computer".into()))?;
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            // Same file → same fingerprint, so an interrupted upload resumes.
            let fingerprint = format!("conecta:file:{path}:{}:{modified}", meta.len());
            Ok((Payload::File(p), name, fingerprint))
        }
        UploadSource::MailAttachment { attachment_id } => {
            let (msg_id, info) = state.mail.get_attachment(*attachment_id)?;
            let detail = state.mail.get_message(msg_id)?;
            let account = state.mail.get_account(detail.summary.account_id)?;
            let provider = state.providers.provider_for(&account.provider)?;
            let data = provider
                .fetch_attachment(&account, &detail.summary.remote_id, &info.remote_id)
                .await?;
            let fingerprint = format!(
                "conecta:mail:{}:{}:{}",
                detail.summary.remote_id,
                info.remote_id,
                data.len()
            );
            Ok((
                Payload::Bytes(data),
                sanitize_filename::sanitize(&info.filename),
                fingerprint,
            ))
        }
    }
}

fn name_taken(body: &Value, upload_id: Option<String>) -> UploadOutcome {
    UploadOutcome::NameTaken {
        name: body["name"].as_str().unwrap_or_default().to_string(),
        suggested: body["suggested"].as_str().unwrap_or_default().to_string(),
        upload_id,
    }
}

/// Uploads a chat file or mail attachment into `dir` on the server. `name`
/// overrides the file's own name (used after a name clash). Progress is
/// emitted as `files://upload` with the caller's `key`.
#[tauri::command]
pub async fn trakzen_files_upload(
    app: AppHandle,
    state: State<'_, AppState>,
    key: String,
    source: UploadSource,
    dir: String,
    name: Option<String>,
) -> Result<UploadOutcome> {
    let conn = connection(&state)?;
    let (payload, own_name, fingerprint) = load_source(&state, &source).await?;
    let name = name.filter(|n| !n.trim().is_empty()).unwrap_or(own_name);
    let size = payload.size().await?;
    upload(&conn, &payload, &dir, &name, &fingerprint, |sent| {
        let _ = app.emit(
            EVENT_UPLOAD,
            UploadProgress {
                key: key.clone(),
                sent,
                total: size,
            },
        );
    })
    .await
}

/// The server's resumable upload protocol: create, then send chunks at an
/// explicit offset, asking for the server's offset again after a failure.
async fn upload(
    conn: &Connection,
    payload: &Payload,
    dir: &str,
    name: &str,
    fingerprint: &str,
    progress: impl Fn(u64),
) -> Result<UploadOutcome> {
    let size = payload.size().await?;
    let created = conn
        .post(
            "/uploads",
            json!({ "dir": dir, "name": name, "size": size, "fingerprint": fingerprint }),
        )
        .await?;
    if created["name_taken"] == true {
        return Ok(name_taken(&created, None));
    }
    let id = created["id"]
        .as_str()
        .ok_or_else(|| AppError::Provider("Trakzen Files: unexpected reply to upload".into()))?
        .to_string();
    if created["done"] == true {
        progress(size);
        return Ok(UploadOutcome::Done {
            path: created["path"].as_str().unwrap_or_default().to_string(),
        });
    }

    let mut offset = created["offset"].as_u64().unwrap_or(0);
    let mut failures = 0;
    progress(offset);
    while offset < size {
        let len = CHUNK.min(size - offset);
        let data = payload.chunk(offset, len).await?;
        let sent = conn
            .request(reqwest::Method::PATCH, &format!("/uploads/{id}"))
            .header("upload-offset", offset)
            .header("content-type", "application/octet-stream")
            .body(data)
            .send()
            .await;
        let reply = match sent {
            Ok(resp) => {
                let status = resp.status();
                let body: Value = resp.json().await.unwrap_or(Value::Null);
                (status, body)
            }
            Err(e) => {
                failures += 1;
                if failures > MAX_RETRIES {
                    return Err(e.into());
                }
                tokio::time::sleep(Duration::from_secs(1 << failures.min(4))).await;
                // Ask where the server got to before resending.
                if let Ok(status) = conn.get(&format!("/uploads/{id}")).await {
                    offset = status["offset"].as_u64().unwrap_or(offset);
                }
                continue;
            }
        };
        match reply {
            (s, body) if s.is_success() => {
                failures = 0;
                offset = body["offset"].as_u64().unwrap_or(offset + len);
                progress(offset);
                if body["done"] == true {
                    return Ok(UploadOutcome::Done {
                        path: body["path"].as_str().unwrap_or_default().to_string(),
                    });
                }
            }
            (s, body) if s.as_u16() == 409 && body["name_taken"] == true => {
                // Someone saved the same name while this was uploading.
                return Ok(name_taken(&body, Some(id)));
            }
            (s, body) if s.as_u16() == 409 || body["offset"].is_u64() => {
                failures += 1;
                if failures > MAX_RETRIES {
                    return Err(AppError::Provider("Trakzen Files: the upload kept failing".into()));
                }
                offset = body["offset"].as_u64().unwrap_or(offset);
            }
            (s, body) => {
                let message = body["error"].as_str().unwrap_or("upload failed");
                return Err(if s.as_u16() == 401 {
                    AppError::Auth(
                        "Trakzen Files no longer accepts this app. Connect again in Settings → Trakzen Files.".into(),
                    )
                } else {
                    AppError::Provider(format!("Trakzen Files: {message}"))
                });
            }
        }
    }
    Err(AppError::Provider("Trakzen Files: the upload did not finish".into()))
}

/// Finishes an upload whose data all arrived but whose name was taken.
#[tauri::command]
pub async fn trakzen_files_rename_upload(
    state: State<'_, AppState>,
    upload_id: String,
    name: String,
) -> Result<UploadOutcome> {
    rename_upload(&connection(&state)?, upload_id, &name).await
}

async fn rename_upload(conn: &Connection, upload_id: String, name: &str) -> Result<UploadOutcome> {
    let body = conn
        .post(&format!("/uploads/{upload_id}/rename"), json!({ "name": name }))
        .await?;
    if body["name_taken"] == true {
        return Ok(name_taken(&body, Some(upload_id)));
    }
    match body["done"].as_bool() {
        Some(true) => Ok(UploadOutcome::Done {
            path: body["path"].as_str().unwrap_or_default().to_string(),
        }),
        _ => Err(AppError::Provider(
            "Trakzen Files: the upload is incomplete; try saving again".into(),
        )),
    }
}

/// Cancels an upload left waiting for a new name.
#[tauri::command]
pub async fn trakzen_files_cancel_upload(state: State<'_, AppState>, upload_id: String) -> Result<()> {
    let conn = connection(&state)?;
    let _ = conn
        .request(reqwest::Method::DELETE, &format!("/uploads/{upload_id}"))
        .send()
        .await?;
    Ok(())
}

/// Opens a folder of the server in the browser.
#[tauri::command]
pub async fn trakzen_files_open(app: AppHandle, state: State<'_, AppState>, path: String) -> Result<()> {
    let url = settings::get(&state.db, settings::TRAKZEN_FILES_URL)?
        .ok_or_else(|| AppError::NotConfigured("connect Trakzen Files first".into()))?;
    let segments: Vec<String> = path
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|s| {
            url::form_urlencoded::byte_serialize(s.as_bytes())
                .collect::<String>()
                .replace('+', "%20")
        })
        .collect();
    let target = format!("{url}/files/{}", segments.join("/"));
    app.opener()
        .open_url(target, None::<&str>)
        .map_err(|e| AppError::Other(format!("could not open the browser: {e}")))
}

/// Runs the connect and upload code against a real server:
///
/// ```sh
/// TRAKZEN_FILES_TEST_URL=http://127.0.0.1:54380 TRAKZEN_FILES_TEST_USER=me /// TRAKZEN_FILES_TEST_PASSWORD=secret cargo test live_connect -- --ignored
/// cargo test live_discover -- --ignored --nocapture   # needs a server on the LAN
/// ```
///
/// It uploads a few test files into Shared/conecta-test-<random>; delete that
/// folder afterwards.
#[cfg(test)]
mod live_tests {
    use super::*;

    struct Env {
        url: String,
        cookie: String,
    }

    async fn sign_in() -> Env {
        let url = std::env::var("TRAKZEN_FILES_TEST_URL").expect("TRAKZEN_FILES_TEST_URL");
        let user = std::env::var("TRAKZEN_FILES_TEST_USER").expect("TRAKZEN_FILES_TEST_USER");
        let password = std::env::var("TRAKZEN_FILES_TEST_PASSWORD").expect("TRAKZEN_FILES_TEST_PASSWORD");
        let resp = HTTP
            .post(format!("{url}/api/auth/login"))
            .header("x-trakzen", "1")
            .json(&json!({ "username": user, "password": password }))
            .send()
            .await
            .unwrap();
        let cookie = resp.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_string();
        assert!(resp.status().is_success(), "sign-in failed");
        Env { url, cookie }
    }

    /// What the browser does when the user presses Allow on /connect.
    async fn approve(env: &Env, challenge: &str, redirect_uri: &str) -> String {
        let resp: Value = HTTP
            .post(format!("{}/api/apps/authorize", env.url))
            .header("x-trakzen", "1")
            .header("cookie", &env.cookie)
            .json(&json!({
                "app": APP_NAME, "device": "live-test", "redirect_uri": redirect_uri,
                "code_challenge": challenge, "code_challenge_method": "S256", "state": "st",
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let redirect = url::Url::parse(resp["redirect"].as_str().expect("redirect")).unwrap();
        let params: std::collections::HashMap<_, _> = redirect.query_pairs().into_owned().collect();
        assert_eq!(params["state"], "st");
        params["code"].clone()
    }

    async fn connect(env: &Env) -> Connection {
        let verifier = random_urlsafe(64);
        let redirect = "http://127.0.0.1:49999/trakzen-files";
        let code = approve(env, &pkce_challenge(&verifier), redirect).await;
        // A code can't be exchanged without the matching verifier…
        let wrong = exchange_code(&env.url, &code, "not-the-verifier", redirect).await;
        assert!(wrong.is_err());
        // …and that failed attempt used it up, so approve again.
        let code = approve(env, &pkce_challenge(&verifier), redirect).await;
        let (token, username) = exchange_code(&env.url, &code, &verifier, redirect).await.unwrap();
        assert!(token.starts_with("tzfs_"));
        assert!(!username.is_empty());
        Connection {
            url: env.url.clone(),
            token,
        }
    }

    fn temp_file(bytes: usize, seed: u8) -> PathBuf {
        let path = std::env::temp_dir().join(format!("conecta-live-{}.bin", uuid::Uuid::new_v4()));
        let data: Vec<u8> = (0..bytes)
            .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
            .collect();
        std::fs::write(&path, data).unwrap();
        path
    }

    #[tokio::test]
    #[ignore]
    async fn live_connect_upload_and_disconnect() {
        let env = sign_in().await;
        let conn = connect(&env).await;
        let whoami = conn.get("/auth/state").await.unwrap();
        assert!(!whoami["user"].is_null(), "token should sign in");

        let folder = format!("conecta-test-{}", &uuid::Uuid::new_v4().to_string()[..8]);
        let dir = trakzen_files_mkdir_inner(&conn, "/shared", &folder).await;

        // Three chunks, with progress reported along the way.
        let big = temp_file(18 * 1024 * 1024 + 123, 7);
        let reported = std::sync::Mutex::new(Vec::new());
        let out = upload(&conn, &Payload::File(big.clone()), &dir, "video.bin", "fp-big", |n| {
            reported.lock().unwrap().push(n)
        })
        .await
        .unwrap();
        let UploadOutcome::Done { path } = out else {
            panic!("expected done")
        };
        assert_eq!(path, format!("{dir}/video.bin"));
        let reported = reported.into_inner().unwrap();
        assert_eq!(*reported.last().unwrap(), 18 * 1024 * 1024 + 123);
        assert!(reported.len() >= 3, "progress per chunk: {reported:?}");

        // Same name again: refused before any data is sent, with a suggestion.
        let small = temp_file(1000, 1);
        let out = upload(
            &conn,
            &Payload::File(small.clone()),
            &dir,
            "video.bin",
            "fp-small",
            |_| {},
        )
        .await
        .unwrap();
        let UploadOutcome::NameTaken {
            suggested, upload_id, ..
        } = out
        else {
            panic!("expected name clash")
        };
        assert_eq!(suggested, "video (2).bin");
        assert!(upload_id.is_none());
        let out = upload(
            &conn,
            &Payload::File(small.clone()),
            &dir,
            &suggested,
            "fp-small",
            |_| {},
        )
        .await
        .unwrap();
        assert!(matches!(out, UploadOutcome::Done { .. }));

        // A clash that only appears once the data is in: finish by renaming.
        let created = conn
            .post(
                "/uploads",
                json!({ "dir": dir, "name": "late.bin", "size": 4, "fingerprint": "fp-late" }),
            )
            .await
            .unwrap();
        let id = created["id"].as_str().unwrap().to_string();
        let other = upload(
            &conn,
            &Payload::Bytes(b"other".to_vec()),
            &dir,
            "late.bin",
            "fp-other",
            |_| {},
        )
        .await
        .unwrap();
        assert!(matches!(other, UploadOutcome::Done { .. }));
        let resp = conn
            .request(reqwest::Method::PATCH, &format!("/uploads/{id}"))
            .header("upload-offset", 0)
            .body(b"late".to_vec())
            .send()
            .await
            .unwrap();
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["name_taken"], true);
        let out = rename_upload(&conn, id, "late renamed.bin").await.unwrap();
        let UploadOutcome::Done { path } = out else {
            panic!("expected done after rename")
        };
        assert_eq!(path, format!("{dir}/late renamed.bin"));

        // Disconnecting revokes the token on the server.
        let resp = conn
            .request(reqwest::Method::DELETE, "/apps/self")
            .send()
            .await
            .unwrap();
        assert!(resp.status().is_success());
        let after = conn.get("/auth/state").await.unwrap();
        assert!(after["user"].is_null(), "token should no longer sign in");

        let _ = std::fs::remove_file(big);
        let _ = std::fs::remove_file(small);
        println!("uploaded into {dir}");
    }

    /// Needs a server announcing itself on this network.
    #[tokio::test]
    #[ignore]
    async fn live_discover() {
        let found = trakzen_files_discover().await.unwrap();
        println!("found: {found:?}");
        assert!(!found.is_empty(), "no Trakzen Files server announced on this network");
        assert!(found.iter().all(|f| f.url.starts_with("http://")));
    }

    async fn trakzen_files_mkdir_inner(conn: &Connection, path: &str, name: &str) -> String {
        let body = conn
            .post("/fs/mkdir", json!({ "path": path, "name": name }))
            .await
            .unwrap();
        body["path"].as_str().unwrap().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_server_addresses() {
        assert_eq!(
            normalize_url("192.168.1.24:54380").unwrap(),
            "http://192.168.1.24:54380"
        );
        assert_eq!(
            normalize_url(" http://dp-server.local:54380/ ").unwrap(),
            "http://dp-server.local:54380"
        );
        assert!(normalize_url("").is_err());
        assert!(normalize_url("ftp://x").is_err());
    }
}
