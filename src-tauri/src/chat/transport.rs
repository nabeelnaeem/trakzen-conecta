//! Chat transport: LAN today, a server relay later.
//!
//! Both implementations speak the same [`crate::chat::protocol`] frames.
//! Identity is the per-user keypair already stored as `chat_peer_id`; a
//! server would authenticate by signing a challenge, then store-and-forward
//! for offline peers (Noise or libsodium, not a custom construction).
//! Evaluate Matrix or XMPP before committing to a custom relay.

#![allow(dead_code)]

use async_trait::async_trait;

use crate::error::{AppError, Result};
use crate::chat::protocol::Frame;
use crate::chat::types::Peer;

#[async_trait]
pub trait Transport: Send + Sync {
    async fn deliver(&self, peer: &Peer, frame: Frame) -> Result<()>;
}

/// Direct TCP on the LAN (what [`super::engine::ChatEngine`] uses today).
pub struct LanTransport;

#[async_trait]
impl Transport for LanTransport {
    async fn deliver(&self, _peer: &Peer, _frame: Frame) -> Result<()> {
        Err(AppError::Other(
            "LAN delivery is handled by ChatEngine, not this adapter".into(),
        ))
    }
}

/// Placeholder for a future WebSocket relay (e.g. behind Caddy on a VPS).
pub struct RelayTransport {
    pub endpoint: Option<String>,
}

#[async_trait]
impl Transport for RelayTransport {
    async fn deliver(&self, _peer: &Peer, _frame: Frame) -> Result<()> {
        match &self.endpoint {
            Some(_) => Err(AppError::Other("server relay is not built yet".into())),
            None => Err(AppError::Other("no chat server configured".into())),
        }
    }
}
