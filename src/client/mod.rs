//! RTMP client implementation
//!
//! Provides client-side RTMP for:
//! - Pulling streams from remote RTMP servers
//! - Connecting to any RTMP server for transcoding, relaying, etc.

pub mod config;
pub mod connector;
pub mod puller;

pub use config::ClientConfig;
pub use connector::RtmpConnector;
pub use puller::{ClientEvent, RtmpPuller};
