//! Wire format for peer-to-peer chat over TCP.
//!
//! ```text
//! frame := kind:u8 | len:u32 (big-endian) | payload[len]
//! kind 1 = control message, payload is JSON (`ControlMsg`)
//! kind 2 = file chunk, payload is transfer id (16 bytes) followed by data
//! ```
//!
//! A connection starts with both sides sending `Hello`. Chat connections stay
//! open and carry text; each file transfer opens its own connection so a large
//! file never delays messages.
//!
//! Transfer connection: `FileOffer` → `FileAccept { offset }` → chunks →
//! `FileDone` → `Ack`. The receiver keeps partial files, so a transfer that
//! drops is re-offered under the same `msg_id` and picks up at `offset`.

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::error::{AppError, Result};

pub const PROTOCOL_VERSION: u32 = 1;
pub const CHUNK_SIZE: usize = 256 * 1024;
const MAX_FRAME: u32 = 4 * 1024 * 1024;

const KIND_CONTROL: u8 = 1;
const KIND_CHUNK: u8 = 2;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Purpose {
    Chat,
    Transfer,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlMsg {
    Hello {
        version: u32,
        peer_id: String,
        display_name: String,
        /// Port this peer listens on, so the receiver can dial back later.
        port: u16,
        purpose: Purpose,
    },
    Text {
        msg_id: String,
        body: String,
        sent_at: i64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply_to: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        group_id: Option<String>,
    },
    /// The peer is composing; UI shows "typing…" briefly.
    Typing,
    /// Sender changed their display name; peers otherwise only learn it from
    /// `Hello`, which chat connections send once and then stay open for hours.
    Rename {
        display_name: String,
    },
    /// Receiver has displayed these messages.
    Read {
        msg_ids: Vec<String>,
    },
    /// Toggle an emoji reaction on a message.
    React {
        msg_id: String,
        emoji: String,
        add: bool,
    },
    /// Sender changed the text of one of their own messages.
    Edit {
        msg_id: String,
        body: String,
    },
    Ack {
        msg_id: String,
    },
    FileOffer {
        transfer_id: String,
        msg_id: String,
        name: String,
        size: u64,
        /// Sender will wait for `FileAccept` and start at the offset it
        /// names. Absent from older builds, which always stream from zero.
        #[serde(default)]
        resumable: bool,
    },
    /// Receiver already holds `offset` bytes of this message's file.
    FileAccept {
        transfer_id: String,
        offset: u64,
    },
    FileDone {
        transfer_id: String,
    },
    FileError {
        transfer_id: String,
        reason: String,
    },
    FilePause {
        transfer_id: String,
    },
    FileResume {
        transfer_id: String,
    },
    Pin {
        msg_id: String,
        pinned: bool,
    },
    GroupInvite {
        group_id: String,
        name: String,
        members: Vec<String>,
    },
    /// Sender retracted a message ("delete for everyone").
    Delete {
        msg_id: String,
    },
    /// Retired: builds up to 0.3.0 sent this to wipe the other side too.
    /// Kept so their frames still decode; it is ignored on receipt.
    ClearChat,
    Ping,
    Pong,
    /// Any `type` this build does not know. Newer peers may send frames we
    /// have not learned yet; dropping the connection over one would be worse
    /// than skipping it.
    #[serde(other)]
    Unknown,
}

#[derive(Debug)]
pub enum Frame {
    Control(ControlMsg),
    Chunk { transfer_id: [u8; 16], data: Vec<u8> },
}

pub async fn write_frame<W: AsyncWrite + Unpin>(w: &mut W, frame: &Frame) -> Result<()> {
    match frame {
        Frame::Control(msg) => {
            let payload = serde_json::to_vec(msg)?;
            w.write_u8(KIND_CONTROL).await?;
            w.write_u32(payload.len() as u32).await?;
            w.write_all(&payload).await?;
        }
        Frame::Chunk { transfer_id, data } => {
            w.write_u8(KIND_CHUNK).await?;
            w.write_u32((16 + data.len()) as u32).await?;
            w.write_all(transfer_id).await?;
            w.write_all(data).await?;
        }
    }
    w.flush().await?;
    Ok(())
}

/// Returns `Ok(None)` on a clean EOF at a frame boundary.
pub async fn read_frame<R: AsyncRead + Unpin>(r: &mut R) -> Result<Option<Frame>> {
    let kind = match r.read_u8().await {
        Ok(k) => k,
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let len = r.read_u32().await?;
    if len > MAX_FRAME {
        return Err(AppError::Other(format!("frame too large: {len} bytes")));
    }
    let mut payload = vec![0u8; len as usize];
    r.read_exact(&mut payload).await?;
    match kind {
        KIND_CONTROL => Ok(Some(Frame::Control(serde_json::from_slice(&payload)?))),
        KIND_CHUNK => {
            if payload.len() < 16 {
                return Err(AppError::Other("short chunk frame".into()));
            }
            let mut transfer_id = [0u8; 16];
            transfer_id.copy_from_slice(&payload[..16]);
            payload.drain(..16);
            Ok(Some(Frame::Chunk {
                transfer_id,
                data: payload,
            }))
        }
        other => Err(AppError::Other(format!("unknown frame kind {other}"))),
    }
}

pub fn transfer_id_bytes(id: &str) -> [u8; 16] {
    uuid::Uuid::parse_str(id)
        .map(|u| *u.as_bytes())
        .unwrap_or([0u8; 16])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn control_and_chunk_frames_round_trip() {
        let id = uuid::Uuid::new_v4().to_string();
        let frames = vec![
            Frame::Control(ControlMsg::Text {
                msg_id: id.clone(),
                body: "héllo".into(),
                sent_at: 42,
                reply_to: None,
                group_id: None,
            }),
            Frame::Chunk {
                transfer_id: transfer_id_bytes(&id),
                data: vec![1, 2, 3, 4, 5],
            },
            Frame::Control(ControlMsg::Ping),
        ];

        let mut buf = Vec::new();
        for f in &frames {
            write_frame(&mut buf, f).await.unwrap();
        }

        let mut cursor = std::io::Cursor::new(buf);
        match read_frame(&mut cursor).await.unwrap().unwrap() {
            Frame::Control(ControlMsg::Text { msg_id, body, sent_at, .. }) => {
                assert_eq!(msg_id, id);
                assert_eq!(body, "héllo");
                assert_eq!(sent_at, 42);
            }
            other => panic!("unexpected {other:?}"),
        }
        match read_frame(&mut cursor).await.unwrap().unwrap() {
            Frame::Chunk { transfer_id, data } => {
                assert_eq!(transfer_id, transfer_id_bytes(&id));
                assert_eq!(data, vec![1, 2, 3, 4, 5]);
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(matches!(
            read_frame(&mut cursor).await.unwrap(),
            Some(Frame::Control(ControlMsg::Ping))
        ));
        assert!(read_frame(&mut cursor).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn unknown_control_type_is_tolerated() {
        let payload = br#"{"type":"from_the_future","x":1}"#;
        let mut buf = vec![KIND_CONTROL];
        buf.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        buf.extend_from_slice(payload);
        write_frame(
            &mut buf,
            &Frame::Control(ControlMsg::Rename { display_name: "Benji".into() }),
        )
        .await
        .unwrap();

        let mut cursor = std::io::Cursor::new(buf);
        assert!(matches!(
            read_frame(&mut cursor).await.unwrap(),
            Some(Frame::Control(ControlMsg::Unknown))
        ));
        match read_frame(&mut cursor).await.unwrap().unwrap() {
            Frame::Control(ControlMsg::Rename { display_name }) => assert_eq!(display_name, "Benji"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn oversized_frame_is_rejected() {
        let mut buf = vec![KIND_CONTROL];
        buf.extend_from_slice(&(MAX_FRAME + 1).to_be_bytes());
        let mut cursor = std::io::Cursor::new(buf);
        assert!(read_frame(&mut cursor).await.is_err());
    }
}
