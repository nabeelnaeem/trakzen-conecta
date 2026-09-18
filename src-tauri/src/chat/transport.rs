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

    /// Server sends a nonce; the client signs it with `chat_peer_id`'s key.
    async fn prove_identity(&self, challenge: &[u8]) -> Result<Vec<u8>>;
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

    async fn prove_identity(&self, _challenge: &[u8]) -> Result<Vec<u8>> {
        Err(AppError::Other("LAN peers already trust the local keypair".into()))
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

    async fn prove_identity(&self, _challenge: &[u8]) -> Result<Vec<u8>> {
        Err(AppError::Other("server relay is not built yet".into()))
    }
}
