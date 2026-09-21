use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

use crate::db::{now_ms, Db};
use crate::error::{AppError, Result};
use crate::settings;

use super::protocol::{self, ControlMsg, Frame, Purpose, PROTOCOL_VERSION};
use super::store::{ChatStore, NewMessage};
use super::types::*;

pub const EVENT_MESSAGE: &str = "chat://message";
pub const EVENT_PEER: &str = "chat://peer";
pub const EVENT_TRANSFER: &str = "chat://transfer";
pub const EVENT_STATUS: &str = "chat://status";
pub const EVENT_DELETED: &str = "chat://deleted";
pub const EVENT_TYPING: &str = "chat://typing";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const RECONNECT_TICK: Duration = Duration::from_secs(5);
const BACKOFF_MAX: Duration = Duration::from_secs(60);
/// How long a sender waits for `FileAccept` before assuming an older peer.
const ACCEPT_TIMEOUT: Duration = Duration::from_secs(5);

struct Conn {
    generation: u64,
    tx: mpsc::Sender<Frame>,
}

struct Me {
    peer_id: String,
    display_name: String,
    port: u16,
    listening: bool,
}

pub struct ChatEngine {
    app: AppHandle,
    db: Arc<Db>,
    pub store: ChatStore,
    me: RwLock<Me>,
    conns: Mutex<HashMap<i64, Conn>>,
    connecting: Mutex<HashSet<i64>>,
    generation: AtomicU64,
    /// Per-peer reconnect schedule: (failures so far, earliest next attempt).
    backoff: Mutex<HashMap<i64, (u32, std::time::Instant)>>,
    pub discovery: Mutex<Option<Arc<super::discovery::Discovery>>>,
    paused: Mutex<HashSet<String>>,
    /// Transfers in progress, by transfer id.
    transfers: Mutex<HashMap<String, TransferProgress>>,
    /// Peers whose file queue is being drained right now.
    sending_files: Mutex<HashSet<i64>>,
}

/// Why a transfer stopped. `Transient` keeps the file queued (outgoing) or
/// partial on disk (incoming) so the next attempt resumes it.
enum TransferError {
    Transient(AppError),
    Fatal(AppError),
}

fn fatal(e: std::io::Error) -> TransferError {
    TransferError::Fatal(e.into())
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletedEvent {
    pub peer_id: i64,
    /// Empty means the whole conversation was cleared.
    pub msg_ids: Vec<String>,
}

struct RemoteHello {
    peer_id: String,
    display_name: String,
    port: u16,
    purpose: Purpose,
}

impl ChatEngine {
    pub fn new(app: AppHandle, db: Arc<Db>) -> Result<Arc<Self>> {
        let peer_id = settings::get_or_init(&db, settings::CHAT_PEER_ID, || {
            uuid::Uuid::new_v4().to_string()
        })?;
        let display_name = settings::get_or_init(&db, settings::CHAT_DISPLAY_NAME, || {
            std::env::var("COMPUTERNAME")
                .or_else(|_| std::env::var("HOSTNAME"))
                .or_else(|_| std::env::var("USERNAME"))
                .or_else(|_| std::env::var("USER"))
                .unwrap_or_else(|_| "Me".into())
        })?;
        let port = std::env::var("TRAKZEN_CONECTA_CHAT_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .or(settings::get(&db, settings::CHAT_PORT)?.and_then(|p| p.parse().ok()))
            .unwrap_or(settings::DEFAULT_CHAT_PORT);

        Ok(Arc::new(Self {
            app,
            store: ChatStore::new(db.clone()),
            db,
            me: RwLock::new(Me {
                peer_id,
                display_name,
                port,
                listening: false,
            }),
            conns: Mutex::new(HashMap::new()),
            connecting: Mutex::new(HashSet::new()),
            generation: AtomicU64::new(1),
            backoff: Mutex::new(HashMap::new()),
            discovery: Mutex::new(None),
            paused: Mutex::new(HashSet::new()),
            transfers: Mutex::new(HashMap::new()),
            sending_files: Mutex::new(HashSet::new()),
        }))
    }

    pub fn start(self: &Arc<Self>) {
        if let Err(e) = self.store.requeue_unfinished() {
            tracing::warn!(%e, "could not requeue unfinished transfers");
        }
        let engine = self.clone();
        tauri::async_runtime::spawn(async move {
            let port = engine.me.read().unwrap().port;
            let listener = match TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], port))).await {
                Ok(l) => l,
                Err(e) => {
                    tracing::error!(port, %e, "chat listener failed to bind");
                    let _ = engine.app.emit(
                        EVENT_STATUS,
                        serde_json::json!({ "listening": false, "error": e.to_string() }),
                    );
                    return;
                }
            };
            engine.me.write().unwrap().listening = true;
            tracing::info!(port, "chat listener up");
            {
                let (id, name) = {
                    let me = engine.me.read().unwrap();
                    (me.peer_id.clone(), me.display_name.clone())
                };
                *engine.discovery.lock().unwrap() =
                    super::discovery::Discovery::start(engine.app.clone(), id, name, port);
            }
            let _ = engine
                .app
                .emit(EVENT_STATUS, serde_json::json!({ "listening": true }));

            let reconnect = engine.clone();
            tauri::async_runtime::spawn(async move { reconnect.reconnect_loop().await });

            loop {
                match listener.accept().await {
                    Ok((stream, addr)) => {
                        let engine = engine.clone();
                        tauri::async_runtime::spawn(async move {
                            if let Err(e) = engine.handle_inbound(stream, addr).await {
                                tracing::debug!(%addr, %e, "inbound connection ended with error");
                            }
                        });
                    }
                    Err(e) => {
                        tracing::warn!(%e, "accept failed");
                        tokio::time::sleep(Duration::from_millis(200)).await;
                    }
                }
            }
        });
    }

    // ---- identity ---------------------------------------------------------

    pub fn identity(&self) -> Identity {
        let me = self.me.read().unwrap();
        Identity {
            peer_id: me.peer_id.clone(),
            display_name: me.display_name.clone(),
            port: me.port,
            addresses: local_addresses(),
            listening: me.listening,
        }
    }

    pub fn set_display_name(&self, name: &str) -> Result<Identity> {
        let name = name.trim();
        if name.is_empty() {
            return Err(AppError::Other("display name cannot be empty".into()));
        }
        settings::set(&self.db, settings::CHAT_DISPLAY_NAME, name)?;
        self.me.write().unwrap().display_name = name.to_string();
        let (id, port) = {
            let me = self.me.read().unwrap();
            (me.peer_id.clone(), me.port)
        };
        if let Some(d) = self.discovery.lock().unwrap().as_ref() {
            d.rename(&id, name, port);
        }
        let live: Vec<mpsc::Sender<Frame>> =
            self.conns.lock().unwrap().values().map(|c| c.tx.clone()).collect();
        let name = name.to_string();
        tauri::async_runtime::spawn(async move {
            for tx in live {
                let _ = tx
                    .send(Frame::Control(ControlMsg::Rename {
                        display_name: name.clone(),
                    }))
                    .await;
            }
        });
        Ok(self.identity())
    }

    fn hello(&self, purpose: Purpose) -> ControlMsg {
        let me = self.me.read().unwrap();
        ControlMsg::Hello {
            version: PROTOCOL_VERSION,
            peer_id: me.peer_id.clone(),
            display_name: me.display_name.clone(),
            port: me.port,
            purpose,
        }
    }

    // ---- peers ------------------------------------------------------------

    pub fn nearby(&self) -> Vec<super::discovery::Nearby> {
        self.discovery.lock().unwrap().as_ref().map(|d| d.list()).unwrap_or_default()
    }

    pub fn is_online(&self, row_id: i64) -> bool {
        self.conns.lock().unwrap().contains_key(&row_id)
    }

    pub fn list_peers(&self) -> Result<Vec<Peer>> {
        let mut peers = self.store.list_peers()?;
        let conns = self.conns.lock().unwrap();
        for p in &mut peers {
            p.online = conns.contains_key(&p.id);
        }
        Ok(peers)
    }

    fn emit_peer(&self, row_id: i64) {
        if let Ok(mut p) = self.store.get_peer(row_id) {
            p.online = self.is_online(row_id);
            let _ = self.app.emit(EVENT_PEER, p);
        }
    }

    fn emit_message(&self, m: &ChatMessage) {
        let _ = self.app.emit(EVENT_MESSAGE, m);
    }

    /// Dials offline peers on an exponential schedule (5 s doubling to 60 s)
    /// so a machine that is off for the day is not hammered every few seconds.
    async fn reconnect_loop(self: Arc<Self>) {
        loop {
            if let Ok(peers) = self.store.list_peers() {
                let now = std::time::Instant::now();
                for p in peers {
                    if self.is_online(p.id) {
                        continue;
                    }
                    let due = self
                        .backoff
                        .lock()
                        .unwrap()
                        .get(&p.id)
                        .map_or(true, |(_, next)| *next <= now);
                    if !due {
                        continue;
                    }
                    let engine = self.clone();
                    tauri::async_runtime::spawn(async move {
                        if engine.connect_peer(p.id).await.is_err() {
                            let mut b = engine.backoff.lock().unwrap();
                            let (n, _) = b.get(&p.id).copied().unwrap_or((0, std::time::Instant::now()));
                            let wait = (RECONNECT_TICK * 2u32.pow(n.min(4))).min(BACKOFF_MAX);
                            b.insert(p.id, (n + 1, std::time::Instant::now() + wait));
                        }
                    });
                }
            }
            // A transfer that broke while the chat link stayed up is retried
            // here, since nothing else would kick its queue again.
            if let Ok(ids) = self.store.peers_with_queued_files() {
                for id in ids.into_iter().filter(|id| self.is_online(*id)) {
                    let engine = self.clone();
                    tauri::async_runtime::spawn(async move { engine.flush_files(id).await });
                }
            }
            tokio::time::sleep(RECONNECT_TICK).await;
        }
    }

    /// Every address worth trying for a peer: the stored one first, then
    /// whatever mDNS currently reports for its peer id (handles machines
    /// that changed IP or sit on a different interface).
    fn candidate_addresses(&self, peer: &Peer) -> Vec<(String, u16)> {
        let mut out = vec![(peer.host.clone(), peer.port)];
        if let (Some(id), Some(d)) = (&peer.peer_id, self.discovery.lock().unwrap().as_ref()) {
            for a in d.addresses_for(id) {
                if !out.contains(&a) {
                    out.push(a);
                }
            }
        }
        out
    }

    /// Dials the peer's chat port and runs the connection until it drops.
    /// Returns the sender for the live connection.
    pub async fn connect_peer(self: &Arc<Self>, row_id: i64) -> Result<mpsc::Sender<Frame>> {
        if let Some(c) = self.conns.lock().unwrap().get(&row_id) {
            return Ok(c.tx.clone());
        }
        if !self.connecting.lock().unwrap().insert(row_id) {
            return Err(AppError::Other("already connecting".into()));
        }
        let result = self.connect_peer_inner(row_id).await;
        self.connecting.lock().unwrap().remove(&row_id);
        result
    }

    async fn connect_peer_inner(self: &Arc<Self>, row_id: i64) -> Result<mpsc::Sender<Frame>> {
        let peer = self.store.get_peer(row_id)?;
        let mut last_err = AppError::Other("no address".into());
        let mut dialed: Option<(TcpStream, String)> = None;
        for (host, port) in self.candidate_addresses(&peer) {
            match dial(&host, port).await {
                Ok(s) => {
                    dialed = Some((s, host));
                    break;
                }
                Err(e) => last_err = e,
            }
        }
        let Some((mut stream, host)) = dialed else { return Err(last_err) };
        let hello = self.handshake(&mut stream, Purpose::Chat).await?;
        let bound = self.store.bind_peer(
            Some(row_id),
            &hello.peer_id,
            &hello.display_name,
            &host,
            hello.port,
        )?;
        let tx = self.clone().run_chat_connection(stream, bound);
        if bound != row_id {
            // Merged into an existing row; tell the UI so it refreshes.
            let _ = self.app.emit(EVENT_PEER, self.store.get_peer(bound)?);
        }
        Ok(tx)
    }

    async fn handshake(&self, stream: &mut TcpStream, purpose: Purpose) -> Result<RemoteHello> {
        protocol::write_frame(stream, &Frame::Control(self.hello(purpose))).await?;
        let frame = tokio::time::timeout(HANDSHAKE_TIMEOUT, protocol::read_frame(stream))
            .await
            .map_err(|_| AppError::Other("handshake timed out".into()))??;
        match frame {
            Some(Frame::Control(ControlMsg::Hello {
                version,
                peer_id,
                display_name,
                port,
                purpose,
            })) => {
                if version != PROTOCOL_VERSION {
                    return Err(AppError::Other(format!(
                        "peer speaks protocol v{version}, this build speaks v{PROTOCOL_VERSION}"
                    )));
                }
                let my_id = self.me.read().unwrap().peer_id.clone();
                if peer_id == my_id {
                    return Err(AppError::Other("that address is this machine".into()));
                }
                Ok(RemoteHello {
                    peer_id,
                    display_name,
                    port,
                    purpose,
                })
            }
            _ => Err(AppError::Other("peer did not send a hello".into())),
        }
    }

    async fn handle_inbound(self: Arc<Self>, mut stream: TcpStream, addr: SocketAddr) -> Result<()> {
        let hello = self.handshake(&mut stream, Purpose::Chat).await?;
        let host = addr.ip().to_string();
        let row_id = self.store.bind_peer(
            None,
            &hello.peer_id,
            &hello.display_name,
            &host,
            hello.port,
        )?;
        match hello.purpose {
            Purpose::Chat => {
                self.run_chat_connection(stream, row_id);
                Ok(())
            }
            Purpose::Transfer => self.receive_file(stream, row_id).await,
        }
    }

    /// Registers the connection and spawns its reader/writer tasks.
    fn run_chat_connection(self: Arc<Self>, stream: TcpStream, row_id: i64) -> mpsc::Sender<Frame> {
        let generation = self.generation.fetch_add(1, Ordering::Relaxed);
        let (tx, mut rx) = mpsc::channel::<Frame>(64);
        self.conns.lock().unwrap().insert(
            row_id,
            Conn {
                generation,
                tx: tx.clone(),
            },
        );
        let _ = self.store.touch_peer(row_id);
        self.backoff.lock().unwrap().remove(&row_id);
        self.emit_peer(row_id);
        let flush = self.clone();
        let flush_tx = tx.clone();
        tauri::async_runtime::spawn(async move {
            flush.flush_queue(row_id, &flush_tx).await;
            flush.flush_files(row_id).await;
        });

        let (mut rd, mut wr) = stream.into_split();
        tauri::async_runtime::spawn(async move {
            while let Some(frame) = rx.recv().await {
                if protocol::write_frame(&mut wr, &frame).await.is_err() {
                    break;
                }
            }
            let _ = wr.shutdown().await;
        });

        let engine = self.clone();
        let out = tx.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                match protocol::read_frame(&mut rd).await {
                    Ok(Some(Frame::Control(msg))) => {
                        if let Err(e) = engine.on_control(row_id, msg, &out).await {
                            tracing::warn!(row_id, %e, "failed handling message");
                        }
                    }
                    Ok(Some(Frame::Chunk { .. })) => {
                        tracing::debug!(row_id, "ignoring chunk on chat connection");
                    }
                    Ok(None) => break,
                    Err(e) => {
                        tracing::debug!(row_id, %e, "connection closed");
                        break;
                    }
                }
            }
            let mut conns = engine.conns.lock().unwrap();
            if conns.get(&row_id).map_or(false, |c| c.generation == generation) {
                conns.remove(&row_id);
            }
            drop(conns);
            engine.emit_peer(row_id);
        });

        tx
    }

    async fn on_control(&self, row_id: i64, msg: ControlMsg, out: &mpsc::Sender<Frame>) -> Result<()> {
        match msg {
            ControlMsg::Text { msg_id, body, reply_to, group_id, .. } => {
                let dest = if let Some(g) = group_id.as_deref() {
                    self.store.bind_peer(None, g, "Group", "group", 0)?
                } else {
                    row_id
                };
                let m = self.store.insert_message(&NewMessage {
                    msg_id: &msg_id,
                    peer_id: dest,
                    direction: Direction::In,
                    kind: MessageKind::Text,
                    body: &body,
                    file_name: None,
                    file_path: None,
                    file_size: None,
                    status: "unread",
                    created_at: now_ms(),
                    reply_to: reply_to.as_deref(),
                })?;
                self.store.touch_peer(dest)?;
                self.emit_message(&m);
                self.emit_peer(dest);
                let _ = out.send(Frame::Control(ControlMsg::Ack { msg_id })).await;
                self.spawn_preview(m.msg_id.clone(), body);
            }
            ControlMsg::Ack { msg_id } => {
                // Don't regress a message the peer already reported as read.
                let current = self.store.get_message(&msg_id)?;
                if current.map_or(true, |m| m.status != "read") {
                    if let Some(m) = self.store.set_status(&msg_id, "delivered")? {
                        self.emit_message(&m);
                    }
                }
            }
            ControlMsg::Typing => {
                let _ = self.app.emit(EVENT_TYPING, row_id);
            }
            ControlMsg::Rename { display_name } => {
                let display_name = display_name.trim();
                if !display_name.is_empty() {
                    self.store.set_peer_name(row_id, display_name)?;
                    self.emit_peer(row_id);
                }
            }
            ControlMsg::Read { msg_ids } => {
                for m in self.store.set_status_many(&msg_ids, "read")? {
                    self.emit_message(&m);
                }
            }
            ControlMsg::React { msg_id, emoji, add } => {
                if let Some(m) = self.store.get_message(&msg_id)? {
                    if m.peer_id == row_id {
                        if let Some(m) = self.store.toggle_reaction(&msg_id, &emoji, "peer", Some(add))? {
                            self.emit_message(&m);
                        }
                    }
                }
            }
            ControlMsg::Edit { msg_id, body } => {
                // Only the author may edit: the message must be one the peer sent us.
                if let Some(m) = self.store.get_message(&msg_id)? {
                    if m.peer_id == row_id && m.direction == Direction::In {
                        if let Some(m) = self.store.edit_message(&msg_id, &body)? {
                            self.emit_message(&m);
                        }
                    }
                }
            }
            ControlMsg::Delete { msg_id } => {
                if let Some(m) = self.store.delete_message(&msg_id)? {
                    if m.peer_id == row_id {
                        self.remove_file_if_ours(m.file_path.as_deref()).await;
                        let _ = self.app.emit(EVENT_DELETED, DeletedEvent { peer_id: row_id, msg_ids: vec![msg_id] });
                        self.emit_peer(row_id);
                    } else {
                        // Never let one peer delete another peer's messages.
                        let _ = self.store.insert_message(&NewMessage {
                            msg_id: &m.msg_id,
                            peer_id: m.peer_id,
                            direction: m.direction,
                            kind: m.kind,
                            body: &m.body,
                            file_name: m.file_name.as_deref(),
                            file_path: m.file_path.as_deref(),
                            file_size: m.file_size,
                            status: &m.status,
                            created_at: m.created_at,
                            reply_to: m.reply_to.as_deref(),
                        });
                    }
                }
            }
            ControlMsg::ClearChat => {
                // Older builds could ask us to wipe a conversation; history
                // on this machine is only ever cleared by its owner.
                tracing::debug!(row_id, "ignoring clear-chat request from peer");
            }
            ControlMsg::Ping => {
                let _ = out.send(Frame::Control(ControlMsg::Pong)).await;
            }
            ControlMsg::Pong | ControlMsg::Hello { .. } | ControlMsg::Unknown => {}
            ControlMsg::FilePause { transfer_id } => {
                self.paused.lock().unwrap().insert(transfer_id);
            }
            ControlMsg::FileResume { transfer_id } => {
                self.paused.lock().unwrap().remove(&transfer_id);
            }
            ControlMsg::Pin { msg_id, pinned } => {
                if let Some(m) = self.store.set_pinned(&msg_id, pinned)? {
                    self.emit_message(&m);
                }
            }
            ControlMsg::GroupInvite { group_id, name, .. } => {
                let id = self.store.bind_peer(None, &group_id, &name, "group", 0)?;
                let _ = self.store.mark_group(id);
            }
            ControlMsg::FileOffer { .. }
            | ControlMsg::FileAccept { .. }
            | ControlMsg::FileDone { .. }
            | ControlMsg::FileError { .. } => {
                tracing::debug!("file control frame on chat connection ignored");
            }
        }
        Ok(())
    }

    // ---- deleting ---------------------------------------------------------

    /// Files the app itself wrote (received files, pasted blobs) are removed
    /// with their message; files the user attached from elsewhere are not.
    async fn remove_file_if_ours(&self, path: Option<&str>) {
        let Some(path) = path else { return };
        let p = PathBuf::from(path);
        let ours = [self.download_dir().ok(), self.outgoing_dir().ok()];
        if ours.iter().flatten().any(|dir| p.starts_with(dir)) {
            let _ = tokio::fs::remove_file(&p).await;
        }
    }

    pub fn outgoing_dir(&self) -> Result<PathBuf> {
        Ok(self
            .app
            .path()
            .app_cache_dir()
            .map_err(|e| AppError::Other(format!("no cache dir: {e}")))?
            .join("outgoing"))
    }

    pub async fn delete_message(self: &Arc<Self>, msg_id: &str, for_everyone: bool) -> Result<()> {
        let Some(m) = self.store.delete_message(msg_id)? else { return Ok(()) };
        self.remove_file_if_ours(m.file_path.as_deref()).await;
        if for_everyone && m.direction == Direction::Out {
            if let Ok(tx) = self.connect_peer(m.peer_id).await {
                let _ = tx
                    .send(Frame::Control(ControlMsg::Delete {
                        msg_id: msg_id.to_string(),
                    }))
                    .await;
            }
        }
        let _ = self.app.emit(EVENT_DELETED, DeletedEvent { peer_id: m.peer_id, msg_ids: vec![msg_id.to_string()] });
        self.emit_peer(m.peer_id);
        Ok(())
    }

    pub async fn clear_chat(self: &Arc<Self>, row_id: i64) -> Result<()> {
        let paths = self.store.clear_messages(row_id)?;
        for p in paths {
            self.remove_file_if_ours(Some(&p)).await;
        }
        let _ = self.app.emit(EVENT_DELETED, DeletedEvent { peer_id: row_id, msg_ids: vec![] });
        self.emit_peer(row_id);
        Ok(())
    }

    // ---- sending ----------------------------------------------------------

    pub async fn send_text(self: &Arc<Self>, row_id: i64, body: &str, reply_to: Option<&str>) -> Result<ChatMessage> {
        let body = body.trim();
        if body.is_empty() {
            return Err(AppError::Other("message is empty".into()));
        }
        let msg_id = uuid::Uuid::new_v4().to_string();
        let now = now_ms();
        let msg = self.store.insert_message(&NewMessage {
            msg_id: &msg_id,
            peer_id: row_id,
            direction: Direction::Out,
            kind: MessageKind::Text,
            body,
            file_name: None,
            file_path: None,
            file_size: None,
            status: "sending",
            created_at: now,
            reply_to,
        })?;

        let gid = if self.store.is_group(row_id).unwrap_or(false) {
            self.store.get_peer(row_id).ok().and_then(|p| p.peer_id)
        } else {
            None
        };
        let targets: Vec<i64> = if gid.is_some() {
            self.store.group_members(row_id).unwrap_or_default()
        } else {
            vec![row_id]
        };
        let mut any = false;
        for id in targets {
            let frame = Frame::Control(ControlMsg::Text {
                msg_id: msg_id.clone(),
                body: body.to_string(),
                sent_at: now,
                reply_to: reply_to.map(str::to_string),
                group_id: gid.clone(),
            });
            let sent = match self.connect_peer(id).await {
                Ok(tx) => tx.send(frame).await.is_ok(),
                Err(_) => false,
            };
            any |= sent;
        }
        self.spawn_preview(msg_id.clone(), body.to_string());
        if any {
            Ok(msg)
        } else {
            let queued = self.store.set_status(&msg_id, "queued")?.unwrap_or(msg);
            self.emit_message(&queued);
            Ok(queued)
        }
    }

    /// Sends messages that were written while the peer was offline.
    async fn flush_queue(&self, row_id: i64, tx: &mpsc::Sender<Frame>) {
        let Ok(queued) = self.store.queued_messages(row_id) else { return };
        for m in queued {
            let frame = Frame::Control(ControlMsg::Text {
                msg_id: m.msg_id.clone(),
                body: m.body.clone(),
                sent_at: m.created_at,
                reply_to: m.reply_to.clone(),
                group_id: None,
            });
            if tx.send(frame).await.is_err() {
                break;
            }
            if let Ok(Some(m)) = self.store.set_status(&m.msg_id, "sending") {
                self.emit_message(&m);
            }
        }
    }

    /// Toggles my reaction and tells the peer.
    pub async fn react(self: &Arc<Self>, msg_id: &str, emoji: &str) -> Result<Option<ChatMessage>> {
        let Some(m) = self.store.toggle_reaction(msg_id, emoji, "me", None)? else { return Ok(None) };
        let add = m.reactions.get(emoji).map_or(false, |v| v.iter().any(|w| w == "me"));
        if let Ok(tx) = self.connect_peer(m.peer_id).await {
            let _ = tx
                .send(Frame::Control(ControlMsg::React {
                    msg_id: msg_id.to_string(),
                    emoji: emoji.to_string(),
                    add,
                }))
                .await;
        }
        self.emit_message(&m);
        Ok(Some(m))
    }

    /// Edits one of my own text messages and tells the peer.
    pub async fn edit(self: &Arc<Self>, msg_id: &str, body: &str) -> Result<Option<ChatMessage>> {
        let Some(existing) = self.store.get_message(msg_id)? else { return Ok(None) };
        if existing.direction != Direction::Out || existing.kind != MessageKind::Text {
            return Err(AppError::Other("only your own text messages can be edited".into()));
        }
        let body = body.trim();
        if body.is_empty() {
            return Err(AppError::Other("message is empty".into()));
        }
        let Some(m) = self.store.edit_message(msg_id, body)? else { return Ok(None) };
        if let Ok(tx) = self.connect_peer(m.peer_id).await {
            let _ = tx
                .send(Frame::Control(ControlMsg::Edit {
                    msg_id: msg_id.to_string(),
                    body: body.to_string(),
                }))
                .await;
        }
        self.emit_message(&m);
        Ok(Some(m))
    }

    pub async fn send_typing(self: &Arc<Self>, row_id: i64) {
        let tx = self.conns.lock().unwrap().get(&row_id).map(|c| c.tx.clone());
        if let Some(tx) = tx {
            let _ = tx.try_send(Frame::Control(ControlMsg::Typing));
        }
    }

    /// Marks a peer's messages read locally and tells the peer.
    pub async fn mark_read(self: &Arc<Self>, row_id: i64) -> Result<()> {
        let ids = self.store.mark_read(row_id)?;
        if !ids.is_empty() {
            if let Some(tx) = self.conns.lock().unwrap().get(&row_id).map(|c| c.tx.clone()) {
                let _ = tx.try_send(Frame::Control(ControlMsg::Read { msg_ids: ids }));
            }
        }
        Ok(())
    }

    pub async fn send_file(self: &Arc<Self>, row_id: i64, path: String) -> Result<ChatMessage> {
        let path = PathBuf::from(path);
        let meta = tokio::fs::metadata(&path).await?;
        if !meta.is_file() {
            return Err(AppError::Other("not a file".into()));
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".into());
        let msg_id = uuid::Uuid::new_v4().to_string();
        let msg = self.store.insert_message(&NewMessage {
            msg_id: &msg_id,
            peer_id: row_id,
            direction: Direction::Out,
            kind: MessageKind::File,
            body: "",
            file_name: Some(&name),
            file_path: Some(&path.to_string_lossy()),
            file_size: Some(meta.len() as i64),
            status: "queued",
            created_at: now_ms(),
            reply_to: None,
        })?;
        let engine = self.clone();
        tauri::async_runtime::spawn(async move { engine.flush_files(row_id).await });
        Ok(msg)
    }

    /// Sends the peer's queued files one at a time. Stops at the first
    /// failure; a transient one leaves the file queued for the next attempt.
    async fn flush_files(self: Arc<Self>, row_id: i64) {
        if !self.sending_files.lock().unwrap().insert(row_id) {
            return;
        }
        loop {
            let Ok(queued) = self.store.queued_files(row_id) else { break };
            let Some(m) = queued.into_iter().next() else { break };
            let (Some(name), Some(path), Some(size)) = (m.file_name.clone(), m.file_path.clone(), m.file_size) else {
                let _ = self.store.set_status(&m.msg_id, "failed");
                continue;
            };
            if let Ok(Some(m)) = self.store.set_status(&m.msg_id, "sending") {
                self.emit_message(&m);
            }
            let transfer_id = uuid::Uuid::new_v4().to_string();
            let outcome = self
                .push_file(row_id, &transfer_id, &m.msg_id, &name, size as u64, &PathBuf::from(&path))
                .await;
            let (status, state) = match &outcome {
                Ok(()) => ("delivered", "done"),
                Err(TransferError::Transient(e)) => {
                    tracing::debug!(row_id, %e, "file send interrupted, will retry");
                    ("queued", "failed")
                }
                Err(TransferError::Fatal(e)) => {
                    tracing::warn!(row_id, %e, "file send failed");
                    ("failed", "failed")
                }
            };
            if let Ok(Some(m)) = self.store.set_status(&m.msg_id, status) {
                self.emit_message(&m);
            }
            self.emit_progress(TransferProgress {
                transfer_id,
                msg_id: m.msg_id.clone(),
                peer_id: row_id,
                direction: Direction::Out,
                file_name: name,
                bytes_done: size as u64,
                bytes_total: size as u64,
                state: state.into(),
            });
            if outcome.is_err() {
                break;
            }
        }
        self.sending_files.lock().unwrap().remove(&row_id);
    }

    async fn push_file(
        &self,
        row_id: i64,
        transfer_id: &str,
        msg_id: &str,
        name: &str,
        size: u64,
        path: &PathBuf,
    ) -> std::result::Result<(), TransferError> {
        use tokio::io::{AsyncReadExt, AsyncSeekExt};

        let mut file = tokio::fs::File::open(path).await.map_err(|e| TransferError::Fatal(e.into()))?;

        let peer = self.store.get_peer(row_id).map_err(TransferError::Fatal)?;
        let mut stream = None;
        let mut last_err = AppError::Other("no address".into());
        for (host, port) in self.candidate_addresses(&peer) {
            match dial(&host, port).await {
                Ok(s) => {
                    stream = Some(s);
                    break;
                }
                Err(e) => last_err = e,
            }
        }
        let Some(mut stream) = stream else { return Err(TransferError::Transient(last_err)) };
        self.handshake(&mut stream, Purpose::Transfer).await.map_err(TransferError::Transient)?;

        protocol::write_frame(
            &mut stream,
            &Frame::Control(ControlMsg::FileOffer {
                transfer_id: transfer_id.to_string(),
                msg_id: msg_id.to_string(),
                name: name.to_string(),
                size,
                resumable: true,
            }),
        )
        .await
        .map_err(TransferError::Transient)?;

        // A receiver from before resume support never answers the offer and
        // just waits for chunks, so a silent peer means "start from zero".
        let offset = match tokio::time::timeout(ACCEPT_TIMEOUT, protocol::read_frame(&mut stream)).await {
            Ok(Ok(Some(Frame::Control(ControlMsg::FileAccept { offset, .. })))) => offset,
            Ok(Ok(Some(Frame::Control(ControlMsg::FileError { reason, .. })))) => {
                return Err(TransferError::Fatal(AppError::Other(format!("receiver rejected file: {reason}"))));
            }
            Ok(Ok(_)) => return Err(TransferError::Transient(AppError::Other("unexpected reply to file offer".into()))),
            Ok(Err(e)) => return Err(TransferError::Transient(e)),
            Err(_) => 0,
        };
        if offset > size {
            return Err(TransferError::Fatal(AppError::Other("receiver claims more bytes than the file has".into())));
        }
        file.seek(std::io::SeekFrom::Start(offset)).await.map_err(|e| TransferError::Fatal(e.into()))?;

        let progress = |done: u64, state: &str| TransferProgress {
            transfer_id: transfer_id.to_string(),
            msg_id: msg_id.to_string(),
            peer_id: row_id,
            direction: Direction::Out,
            file_name: name.to_string(),
            bytes_done: done,
            bytes_total: size,
            state: state.into(),
        };
        self.emit_progress(progress(offset, "active"));

        let tid = protocol::transfer_id_bytes(transfer_id);
        let mut buf = vec![0u8; protocol::CHUNK_SIZE];
        let mut done = offset;
        let mut last_emit = offset;
        loop {
            let n = file.read(&mut buf).await.map_err(|e| TransferError::Fatal(e.into()))?;
            if n == 0 {
                break;
            }
            protocol::write_frame(
                &mut stream,
                &Frame::Chunk {
                    transfer_id: tid,
                    data: buf[..n].to_vec(),
                },
            )
            .await
            .map_err(TransferError::Transient)?;
            self.wait_unpaused(transfer_id).await;
            done += n as u64;
            if done - last_emit >= 1024 * 1024 {
                last_emit = done;
                self.emit_progress(progress(done, "active"));
            }
        }
        if done != size {
            return Err(TransferError::Fatal(AppError::Other(format!(
                "file changed while sending: expected {size} bytes, read {done}"
            ))));
        }
        protocol::write_frame(
            &mut stream,
            &Frame::Control(ControlMsg::FileDone {
                transfer_id: transfer_id.to_string(),
            }),
        )
        .await
        .map_err(TransferError::Transient)?;

        // Wait for the receiver to confirm it wrote everything out.
        let ack = tokio::time::timeout(Duration::from_secs(60), protocol::read_frame(&mut stream))
            .await
            .map_err(|_| TransferError::Transient(AppError::Other("receiver did not acknowledge".into())))?
            .map_err(TransferError::Transient)?;
        match ack {
            Some(Frame::Control(ControlMsg::Ack { msg_id: id })) if id == msg_id => Ok(()),
            Some(Frame::Control(ControlMsg::FileError { reason, .. })) => {
                Err(TransferError::Fatal(AppError::Other(format!("receiver rejected file: {reason}"))))
            }
            _ => Err(TransferError::Transient(AppError::Other("unexpected reply after file".into()))),
        }
    }

    async fn receive_file(&self, mut stream: TcpStream, row_id: i64) -> Result<()> {
        let offer = tokio::time::timeout(HANDSHAKE_TIMEOUT, protocol::read_frame(&mut stream))
            .await
            .map_err(|_| AppError::Other("no file offer".into()))??;
        let (transfer_id, msg_id, name, size, resumable) = match offer {
            Some(Frame::Control(ControlMsg::FileOffer {
                transfer_id,
                msg_id,
                name,
                size,
                resumable,
            })) => (transfer_id, msg_id, name, size, resumable),
            _ => return Err(AppError::Other("expected a file offer".into())),
        };

        let dir = self.download_dir()?;
        tokio::fs::create_dir_all(&dir).await?;

        // A re-offer of a message we already started keeps its partial file;
        // anything else starts fresh under a name nobody else is using.
        let existing = self.store.get_message(&msg_id)?.filter(|m| m.peer_id == row_id);
        let (final_path, part_path, mut offset) = match existing.as_ref().and_then(|m| m.file_path.clone()) {
            Some(p) if p.ends_with(".part") => {
                let part = PathBuf::from(&p);
                let have = tokio::fs::metadata(&part).await.map(|m| m.len()).unwrap_or(0);
                let final_path = PathBuf::from(&p[..p.len() - 5]);
                (final_path, part, have.min(size))
            }
            Some(p) if existing.as_ref().map_or(false, |m| m.status != "failed") => {
                let final_path = PathBuf::from(&p);
                let have = tokio::fs::metadata(&final_path).await.map(|m| m.len()).unwrap_or(0);
                if have == size {
                    (final_path, PathBuf::new(), size)
                } else {
                    let final_path = crate::util::unique_download_path(&dir, &safe_file_name(&name));
                    let part = crate::util::part_path(&final_path);
                    (final_path, part, 0)
                }
            }
            _ => {
                let final_path = crate::util::unique_download_path(&dir, &safe_file_name(&name));
                let part = crate::util::part_path(&final_path);
                (final_path, part, 0)
            }
        };
        if !resumable {
            offset = 0;
        }

        let already_complete = offset == size && part_path.as_os_str().is_empty();
        if !already_complete {
            let msg = self.store.insert_message(&NewMessage {
                msg_id: &msg_id,
                peer_id: row_id,
                direction: Direction::In,
                kind: MessageKind::File,
                body: "",
                file_name: Some(&name),
                file_path: Some(&part_path.to_string_lossy()),
                file_size: Some(size as i64),
                status: "receiving",
                created_at: now_ms(),
                reply_to: None,
            })?;
            let msg = self
                .store
                .set_file_result(&msg.msg_id, "receiving", Some(&part_path.to_string_lossy()))?
                .unwrap_or(msg);
            self.emit_message(&msg);
            self.emit_peer(row_id);
        }

        if resumable {
            protocol::write_frame(
                &mut stream,
                &Frame::Control(ControlMsg::FileAccept {
                    transfer_id: transfer_id.clone(),
                    offset,
                }),
            )
            .await?;
        }

        let progress = |done: u64, state: &str| TransferProgress {
            transfer_id: transfer_id.clone(),
            msg_id: msg_id.clone(),
            peer_id: row_id,
            direction: Direction::In,
            file_name: name.clone(),
            bytes_done: done,
            bytes_total: size,
            state: state.into(),
        };

        let outcome = if already_complete {
            drain_to_done(&mut stream).await.map_err(TransferError::Transient)
        } else {
            self.emit_progress(progress(offset, "active"));
            self.pull_file(&mut stream, &transfer_id, offset, size, &part_path, &progress).await
        };

        match outcome {
            Ok(()) => {
                if !already_complete {
                    tokio::fs::rename(&part_path, &final_path).await?;
                    if let Some(m) =
                        self.store
                            .set_file_result(&msg_id, "unread", Some(&final_path.to_string_lossy()))?
                    {
                        self.emit_message(&m);
                    }
                    self.emit_peer(row_id);
                }
                self.emit_progress(progress(size, "done"));
                protocol::write_frame(&mut stream, &Frame::Control(ControlMsg::Ack { msg_id }))
                    .await?;
                let _ = stream.shutdown().await;
                Ok(())
            }
            Err(e) => {
                // The partial file stays so the sender can resume; only a
                // corrupt stream throws it away.
                let (status, e) = match e {
                    TransferError::Transient(e) => ("interrupted", e),
                    TransferError::Fatal(e) => {
                        let _ = tokio::fs::remove_file(&part_path).await;
                        ("failed", e)
                    }
                };
                if !already_complete {
                    if let Some(m) = self.store.set_status(&msg_id, status)? {
                        self.emit_message(&m);
                    }
                }
                self.emit_progress(progress(0, "failed"));
                let _ = protocol::write_frame(
                    &mut stream,
                    &Frame::Control(ControlMsg::FileError {
                        transfer_id,
                        reason: e.to_string(),
                    }),
                )
                .await;
                Err(e)
            }
        }
    }

    async fn pull_file(
        &self,
        stream: &mut TcpStream,
        transfer_id: &str,
        offset: u64,
        size: u64,
        path: &PathBuf,
        progress: &(dyn Fn(u64, &str) -> TransferProgress + Send + Sync),
    ) -> std::result::Result<(), TransferError> {
        use tokio::io::AsyncSeekExt;

        let expected = protocol::transfer_id_bytes(transfer_id);
        let mut file = if offset > 0 {
            let mut f = tokio::fs::OpenOptions::new().write(true).open(path).await.map_err(fatal)?;
            f.set_len(offset).await.map_err(fatal)?;
            f.seek(std::io::SeekFrom::Start(offset)).await.map_err(fatal)?;
            f
        } else {
            tokio::fs::File::create(path).await.map_err(fatal)?
        };
        let mut done = offset;
        let mut last_emit = offset;
        loop {
            self.wait_unpaused(transfer_id).await;
            let frame = tokio::time::timeout(Duration::from_secs(60), protocol::read_frame(stream))
                .await
                .map_err(|_| TransferError::Transient(AppError::Other("transfer stalled".into())))?
                .map_err(TransferError::Transient)?;
            match frame {
                Some(Frame::Chunk { transfer_id, data }) => {
                    if transfer_id != expected {
                        return Err(TransferError::Fatal(AppError::Other("chunk for unknown transfer".into())));
                    }
                    file.write_all(&data).await.map_err(fatal)?;
                    done += data.len() as u64;
                    if done > size {
                        return Err(TransferError::Fatal(AppError::Other("received more data than offered".into())));
                    }
                    if done - last_emit >= 1024 * 1024 {
                        last_emit = done;
                        self.emit_progress(progress(done, "active"));
                    }
                }
                Some(Frame::Control(ControlMsg::FileDone { .. })) => break,
                Some(Frame::Control(ControlMsg::FileError { reason, .. })) => {
                    return Err(TransferError::Transient(AppError::Other(format!("sender aborted: {reason}"))));
                }
                Some(_) => continue,
                None => {
                    return Err(TransferError::Transient(AppError::Other("connection dropped mid-transfer".into())));
                }
            }
        }
        file.flush().await.map_err(fatal)?;
        if done != size {
            return Err(TransferError::Transient(AppError::Other(format!(
                "size mismatch: expected {size} bytes, got {done}"
            ))));
        }
        Ok(())
    }

    async fn wait_unpaused(&self, transfer_id: &str) {
        loop {
            if !self.paused.lock().unwrap().contains(transfer_id) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    /// Records live transfers so pause/resume can find their peer, and tells
    /// the UI. Finished transfers drop out of the table.
    fn emit_progress(&self, p: TransferProgress) {
        let mut live = self.transfers.lock().unwrap();
        if p.state == "active" || p.state == "paused" {
            live.insert(p.transfer_id.clone(), p.clone());
        } else {
            live.remove(&p.transfer_id);
        }
        drop(live);
        let _ = self.app.emit(EVENT_TRANSFER, p);
    }

    /// Pauses on both ends: the peer's loop must stop too, or its stall
    /// timeout would give up on us after a minute.
    pub fn pause_transfer(&self, transfer_id: &str, pause: bool) {
        {
            let mut g = self.paused.lock().unwrap();
            if pause {
                g.insert(transfer_id.to_string());
            } else {
                g.remove(transfer_id);
            }
        }
        let current = self.transfers.lock().unwrap().get(transfer_id).cloned();
        let Some(mut p) = current else { return };
        p.state = if pause { "paused" } else { "active" }.into();
        let tx = self.conns.lock().unwrap().get(&p.peer_id).map(|c| c.tx.clone());
        if let Some(tx) = tx {
            let msg = if pause {
                ControlMsg::FilePause { transfer_id: transfer_id.to_string() }
            } else {
                ControlMsg::FileResume { transfer_id: transfer_id.to_string() }
            };
            let _ = tx.try_send(Frame::Control(msg));
        }
        self.emit_progress(p);
    }

    pub async fn pin_message(self: &Arc<Self>, msg_id: &str, pinned: bool) -> Result<Option<ChatMessage>> {
        let Some(m) = self.store.set_pinned(msg_id, pinned)? else { return Ok(None) };
        if let Ok(tx) = self.connect_peer(m.peer_id).await {
            let _ = tx
                .send(Frame::Control(ControlMsg::Pin {
                    msg_id: msg_id.to_string(),
                    pinned,
                }))
                .await;
        }
        self.emit_message(&m);
        Ok(Some(m))
    }

    pub async fn create_group(self: &Arc<Self>, name: &str, member_ids: Vec<i64>) -> Result<Peer> {
        let peer = self.store.create_group(name.trim(), &member_ids)?;
        let gid = peer.peer_id.clone().unwrap_or_default();
        let members: Vec<String> = member_ids
            .iter()
            .filter_map(|id| self.store.get_peer(*id).ok()?.peer_id)
            .collect();
        for id in member_ids {
            if let Ok(tx) = self.connect_peer(id).await {
                let _ = tx
                    .send(Frame::Control(ControlMsg::GroupInvite {
                        group_id: gid.clone(),
                        name: name.to_string(),
                        members: members.clone(),
                    }))
                    .await;
            }
        }
        Ok(peer)
    }

    fn spawn_preview(&self, msg_id: String, body: String) {
        let store = self.store.clone();
        let app = self.app.clone();
        tauri::async_runtime::spawn(async move {
            let Some(url) = first_http_url(&body) else { return };
            if let Some(preview) = fetch_preview(&url).await {
                if let Ok(Some(m)) = store.set_preview(&msg_id, &preview) {
                    let _ = app.emit(EVENT_MESSAGE, m);
                }
            }
        });
    }

    pub fn download_dir(&self) -> Result<PathBuf> {
        if let Some(dir) = settings::get(&self.db, settings::CHAT_DOWNLOAD_DIR)? {
            if !dir.trim().is_empty() {
                return Ok(PathBuf::from(dir));
            }
        }
        Ok(self
            .app
            .path()
            .download_dir()
            .map_err(|e| AppError::Other(format!("no downloads dir: {e}")))?
            .join("Trakzen Conecta"))
    }
}

fn safe_file_name(name: &str) -> String {
    let safe = sanitize_filename::sanitize(name);
    if safe.is_empty() { "file".to_string() } else { safe }
}

/// We already have the whole file: let the sender run through its offer
/// (it sends nothing but `FileDone` when told the offset equals the size).
async fn drain_to_done(stream: &mut TcpStream) -> Result<()> {
    loop {
        match tokio::time::timeout(Duration::from_secs(60), protocol::read_frame(stream))
            .await
            .map_err(|_| AppError::Other("transfer stalled".into()))??
        {
            Some(Frame::Control(ControlMsg::FileDone { .. })) => return Ok(()),
            Some(Frame::Control(ControlMsg::FileError { reason, .. })) => {
                return Err(AppError::Other(format!("sender aborted: {reason}")));
            }
            Some(_) => continue,
            None => return Err(AppError::Other("connection dropped".into())),
        }
    }
}

async fn dial(host: &str, port: u16) -> Result<TcpStream> {
    let addr = format!("{host}:{port}");
    let stream = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(&addr))
        .await
        .map_err(|_| AppError::Other(format!("timed out connecting to {addr}")))?
        .map_err(|e| AppError::Other(format!("could not connect to {addr}: {e}")))?;
    stream.set_nodelay(true)?;
    Ok(stream)
}

fn local_addresses() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Ok(ip) = local_ip_address::local_ip() {
        out.push(ip.to_string());
    }
    if let Ok(ifaces) = local_ip_address::list_afinet_netifas() {
        for (_, ip) in ifaces {
            if let IpAddr::V4(v4) = ip {
                if v4.is_loopback() || v4.is_link_local() {
                    continue;
                }
                let s = v4.to_string();
                if !out.contains(&s) {
                    out.push(s);
                }
            }
        }
    }
    out
}

fn first_http_url(body: &str) -> Option<String> {
    body.split_whitespace().find_map(|w| {
        let w = w.trim_end_matches(|c: char| matches!(c, '.' | ',' | ')' | ']' | '>'));
        if w.starts_with("http://") || w.starts_with("https://") {
            Some(w.to_string())
        } else {
            None
        }
    })
}

async fn fetch_preview(url: &str) -> Option<LinkPreview> {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(6)).build().ok()?;
    let html = client.get(url).send().await.ok()?.text().await.ok()?;
    let pick = |name: &str| -> Option<String> {
        let pat = format!("property=\"og:{name}\"");
        let idx = html.find(&pat).or_else(|| html.find(&format!("name=\"og:{name}\"")))?;
        let slice = &html[idx..idx.saturating_add(400).min(html.len())];
        let start = slice.find("content=\"")? + 9;
        let end = slice[start..].find('"')?;
        Some(slice[start..start + end].to_string())
    };
    Some(LinkPreview {
        url: url.to_string(),
        title: pick("title"),
        description: pick("description"),
        image: pick("image"),
    })
}
