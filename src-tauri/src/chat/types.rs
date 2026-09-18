use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub peer_id: String,
    pub display_name: String,
    pub port: u16,
    /// Local addresses other machines can dial.
    pub addresses: Vec<String>,
    pub listening: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Peer {
    pub id: i64,
    pub peer_id: Option<String>,
    pub display_name: String,
    pub host: String,
    pub port: u16,
    pub last_seen: Option<i64>,
    pub online: bool,
    pub unread: i64,
    pub last_message: Option<String>,
    pub last_message_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    In,
    Out,
}

impl Direction {
    pub fn as_str(self) -> &'static str {
        match self {
            Direction::In => "in",
            Direction::Out => "out",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageKind {
    Text,
    File,
}

impl MessageKind {
    pub fn as_str(self) -> &'static str {
        match self {
            MessageKind::Text => "text",
            MessageKind::File => "file",
        }
    }
}

/// `sending` → `delivered` | `failed` for outgoing; `receiving` → `received`
/// | `failed` for incoming files; incoming text is `received` immediately.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: i64,
    pub msg_id: String,
    pub peer_id: i64,
    pub direction: Direction,
    pub kind: MessageKind,
    pub body: String,
    pub file_name: Option<String>,
    pub file_path: Option<String>,
    pub file_size: Option<i64>,
    pub status: String,
    pub created_at: i64,
    pub reply_to: Option<String>,
    /// emoji → who reacted, as seen from this machine ("me" / "peer").
    pub reactions: std::collections::HashMap<String, Vec<String>>,
    pub edited_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferProgress {
    pub transfer_id: String,
    pub msg_id: String,
    pub peer_id: i64,
    pub direction: Direction,
    pub file_name: String,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub state: String,
}
