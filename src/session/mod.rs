//! RTMP session state management
//!
//! This module manages the state of RTMP connections, including:
//! - Session lifecycle (handshake, connect, publish/play, disconnect)
//! - Per-stream state (message stream ID, publish/play mode)
//! - Context passed to handlers

pub mod context;
pub mod state;
pub mod stream;

pub use context::{SessionContext, StreamContext};
pub use state::SessionState;
pub use stream::StreamState;
