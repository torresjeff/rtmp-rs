//! RTMP wire protocol implementation
//!
//! This module handles the low-level protocol details:
//! - Handshake (C0C1C2/S0S1S2 exchange)
//! - Chunk stream multiplexing and demultiplexing
//! - Message framing and parsing

pub mod chunk;
pub mod constants;
pub mod handshake;
pub mod message;
pub mod quirks;

pub use chunk::{ChunkDecoder, ChunkEncoder};
pub use handshake::{Handshake, HandshakeRole};
pub use message::RtmpMessage;
