//! Peer-to-peer chat and file sharing over the local network. No server:
//! every instance listens on a TCP port and peers are added by IP address.

pub mod commands;
pub mod engine;
pub mod protocol;
pub mod store;
pub mod types;

pub use engine::ChatEngine;
