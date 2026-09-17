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

const CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const RECONNECT_INTERVAL: Duration = Duration::from_secs(20);

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
        }))
    }

    pub fn start(self: &Arc<Self>) {
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

    async fn reconnect_loop(self: Arc<Self>) {
        loop {
            if let Ok(peers) = self.store.list_peers() {
                for p in peers {
                    if !self.is_online(p.id) {
                        let engine = self.clone();
                        tauri::async_runtime::spawn(async move {
                            let _ = engine.connect_peer(p.id).await;
                        });
                    }
                }
            }
            tokio::time::sleep(RECONNECT_INTERVAL).await;
        }
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
        let mut stream = dial(&peer.host, peer.port).await?;
        let hello = self.handshake(&mut stream, Purpose::Chat).await?;
        let bound = self.store.bind_peer(
            Some(row_id),
            &hello.peer_id,
            &hello.display_name,
            &peer.host,
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
        self.emit_peer(row_id);

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
            ControlMsg::Text { msg_id, body, .. } => {
                let m = self.store.insert_message(&NewMessage {
                    msg_id: &msg_id,
                    peer_id: row_id,
                    direction: Direction::In,
                    kind: MessageKind::Text,
                    body: &body,
                    file_name: None,
                    file_path: None,
                    file_size: None,
                    status: "unread",
                    created_at: now_ms(),
                })?;
                self.store.touch_peer(row_id)?;
                self.emit_message(&m);
                self.emit_peer(row_id);
                let _ = out.send(Frame::Control(ControlMsg::Ack { msg_id })).await;
            }
            ControlMsg::Ack { msg_id } => {
                if let Some(m) = self.store.set_status(&msg_id, "delivered")? {
                    self.emit_message(&m);
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
                        });
                    }
                }
            }
            ControlMsg::ClearChat => {
                let paths = self.store.clear_messages(row_id)?;
                for p in paths {
                    self.remove_file_if_ours(Some(&p)).await;
                }
                let _ = self.app.emit(EVENT_DELETED, DeletedEvent { peer_id: row_id, msg_ids: vec![] });
                self.emit_peer(row_id);
            }
            ControlMsg::Ping => {
                let _ = out.send(Frame::Control(ControlMsg::Pong)).await;
            }
            ControlMsg::Pong | ControlMsg::Hello { .. } => {}
            ControlMsg::FileOffer { .. }
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

    pub async fn clear_chat(self: &Arc<Self>, row_id: i64, for_everyone: bool) -> Result<()> {
        let paths = self.store.clear_messages(row_id)?;
        for p in paths {
            self.remove_file_if_ours(Some(&p)).await;
        }
        if for_everyone {
            if let Ok(tx) = self.connect_peer(row_id).await {
                let _ = tx.send(Frame::Control(ControlMsg::ClearChat)).await;
            }
        }
        let _ = self.app.emit(EVENT_DELETED, DeletedEvent { peer_id: row_id, msg_ids: vec![] });
        self.emit_peer(row_id);
        Ok(())
    }

    // ---- sending ----------------------------------------------------------

    pub async fn send_text(self: &Arc<Self>, row_id: i64, body: &str) -> Result<ChatMessage> {
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
        })?;

        let frame = Frame::Control(ControlMsg::Text {
            msg_id: msg_id.clone(),
            body: body.to_string(),
            sent_at: now,
        });
        let sent = match self.connect_peer(row_id).await {
            Ok(tx) => tx.send(frame).await.is_ok(),
            Err(e) => {
                tracing::debug!(row_id, %e, "peer unreachable");
                false
            }
        };
        if sent {
            Ok(msg)
        } else {
            let failed = self
                .store
                .set_status(&msg_id, "failed")?
                .unwrap_or(msg);
            self.emit_message(&failed);
            Err(AppError::Other("peer is not reachable".into()))
        }
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
        let size = meta.len();
        let msg_id = uuid::Uuid::new_v4().to_string();
        let msg = self.store.insert_message(&NewMessage {
            msg_id: &msg_id,
            peer_id: row_id,
            direction: Direction::Out,
            kind: MessageKind::File,
            body: "",
            file_name: Some(&name),
            file_path: Some(&path.to_string_lossy()),
            file_size: Some(size as i64),
            status: "sending",
            created_at: now_ms(),
        })?;

        let engine = self.clone();
        let result_msg = msg.clone();
        tauri::async_runtime::spawn(async move {
            let transfer_id = uuid::Uuid::new_v4().to_string();
            let outcome = engine
                .push_file(row_id, &transfer_id, &result_msg.msg_id, &name, size, &path)
                .await;
            let (status, state) = match &outcome {
                Ok(()) => ("delivered", "done"),
                Err(e) => {
                    tracing::warn!(row_id, %e, "file send failed");
                    ("failed", "failed")
                }
            };
            if let Ok(Some(m)) = engine.store.set_status(&result_msg.msg_id, status) {
                engine.emit_message(&m);
            }
            let _ = engine.app.emit(
                EVENT_TRANSFER,
                TransferProgress {
                    transfer_id,
                    msg_id: result_msg.msg_id.clone(),
                    peer_id: row_id,
                    direction: Direction::Out,
                    file_name: name,
                    bytes_done: size,
                    bytes_total: size,
                    state: state.into(),
                },
            );
        });
        Ok(msg)
    }

    async fn push_file(
        &self,
        row_id: i64,
        transfer_id: &str,
        msg_id: &str,
        name: &str,
        size: u64,
        path: &PathBuf,
    ) -> Result<()> {
        use tokio::io::AsyncReadExt;

        let peer = self.store.get_peer(row_id)?;
        let mut stream = dial(&peer.host, peer.port).await?;
        self.handshake(&mut stream, Purpose::Transfer).await?;

        protocol::write_frame(
            &mut stream,
            &Frame::Control(ControlMsg::FileOffer {
                transfer_id: transfer_id.to_string(),
                msg_id: msg_id.to_string(),
                name: name.to_string(),
                size,
            }),
        )
        .await?;

        let tid = protocol::transfer_id_bytes(transfer_id);
        let mut file = tokio::fs::File::open(path).await?;
        let mut buf = vec![0u8; protocol::CHUNK_SIZE];
        let mut done = 0u64;
        let mut last_emit = 0u64;
        loop {
            let n = file.read(&mut buf).await?;
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
            .await?;
            done += n as u64;
            if done - last_emit >= 1024 * 1024 {
                last_emit = done;
                let _ = self.app.emit(
                    EVENT_TRANSFER,
                    TransferProgress {
                        transfer_id: transfer_id.to_string(),
                        msg_id: msg_id.to_string(),
                        peer_id: row_id,
                        direction: Direction::Out,
                        file_name: name.to_string(),
                        bytes_done: done,
                        bytes_total: size,
                        state: "active".into(),
                    },
                );
            }
        }
        protocol::write_frame(
            &mut stream,
            &Frame::Control(ControlMsg::FileDone {
                transfer_id: transfer_id.to_string(),
            }),
        )
        .await?;

        // Wait for the receiver to confirm it wrote everything out.
        let ack = tokio::time::timeout(Duration::from_secs(60), protocol::read_frame(&mut stream))
            .await
            .map_err(|_| AppError::Other("receiver did not acknowledge".into()))??;
        match ack {
            Some(Frame::Control(ControlMsg::Ack { msg_id: id })) if id == msg_id => Ok(()),
            Some(Frame::Control(ControlMsg::FileError { reason, .. })) => {
                Err(AppError::Other(format!("receiver rejected file: {reason}")))
            }
            _ => Err(AppError::Other("unexpected reply after file".into())),
        }
    }

    async fn receive_file(&self, mut stream: TcpStream, row_id: i64) -> Result<()> {
        let offer = tokio::time::timeout(HANDSHAKE_TIMEOUT, protocol::read_frame(&mut stream))
            .await
            .map_err(|_| AppError::Other("no file offer".into()))??;
        let (transfer_id, msg_id, name, size) = match offer {
            Some(Frame::Control(ControlMsg::FileOffer {
                transfer_id,
                msg_id,
                name,
                size,
            })) => (transfer_id, msg_id, name, size),
            _ => return Err(AppError::Other("expected a file offer".into())),
        };

        let dir = self.download_dir()?;
        tokio::fs::create_dir_all(&dir).await?;
        let safe_name = sanitize_filename::sanitize(&name);
        let safe_name = if safe_name.is_empty() { "file".to_string() } else { safe_name };
        let path = crate::util::unique_path(&dir, &safe_name);

        let msg = self.store.insert_message(&NewMessage {
            msg_id: &msg_id,
            peer_id: row_id,
            direction: Direction::In,
            kind: MessageKind::File,
            body: "",
            file_name: Some(&name),
            file_path: Some(&path.to_string_lossy()),
            file_size: Some(size as i64),
            status: "receiving",
            created_at: now_ms(),
        })?;
        self.emit_message(&msg);
        self.emit_peer(row_id);

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

        let outcome = self
            .pull_file(&mut stream, &transfer_id, size, &path, &progress)
            .await;

        match outcome {
            Ok(()) => {
                if let Some(m) =
                    self.store
                        .set_file_result(&msg_id, "unread", Some(&path.to_string_lossy()))?
                {
                    self.emit_message(&m);
                }
                self.emit_peer(row_id);
                let _ = self.app.emit(EVENT_TRANSFER, progress(size, "done"));
                protocol::write_frame(&mut stream, &Frame::Control(ControlMsg::Ack { msg_id }))
                    .await?;
                let _ = stream.shutdown().await;
                Ok(())
            }
            Err(e) => {
                let _ = tokio::fs::remove_file(&path).await;
                if let Some(m) = self.store.set_status(&msg_id, "failed")? {
                    self.emit_message(&m);
                }
                let _ = self.app.emit(EVENT_TRANSFER, progress(0, "failed"));
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
        size: u64,
        path: &PathBuf,
        progress: &(dyn Fn(u64, &str) -> TransferProgress + Send + Sync),
    ) -> Result<()> {
        let expected = protocol::transfer_id_bytes(transfer_id);
        let mut file = tokio::fs::File::create(path).await?;
        let mut done = 0u64;
        let mut last_emit = 0u64;
        loop {
            let frame = tokio::time::timeout(Duration::from_secs(60), protocol::read_frame(stream))
                .await
                .map_err(|_| AppError::Other("transfer stalled".into()))??;
            match frame {
                Some(Frame::Chunk { transfer_id, data }) => {
                    if transfer_id != expected {
                        return Err(AppError::Other("chunk for unknown transfer".into()));
                    }
                    file.write_all(&data).await?;
                    done += data.len() as u64;
                    if done > size {
                        return Err(AppError::Other("received more data than offered".into()));
                    }
                    if done - last_emit >= 1024 * 1024 {
                        last_emit = done;
                        let _ = self.app.emit(EVENT_TRANSFER, progress(done, "active"));
                    }
                }
                Some(Frame::Control(ControlMsg::FileDone { .. })) => break,
                Some(Frame::Control(ControlMsg::FileError { reason, .. })) => {
                    return Err(AppError::Other(format!("sender aborted: {reason}")));
                }
                Some(_) => continue,
                None => return Err(AppError::Other("connection dropped mid-transfer".into())),
            }
        }
        file.flush().await?;
        if done != size {
            return Err(AppError::Other(format!(
                "size mismatch: expected {size} bytes, got {done}"
            )));
        }
        Ok(())
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
