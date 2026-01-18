//! RTMP server implementation
//!
//! This module provides the server-side RTMP implementation:
//! - TCP listener for accepting connections
//! - Per-connection handler
//! - Handler trait for application callbacks

pub mod config;
pub mod connection;
pub mod handler;
pub mod listener;

pub use config::ServerConfig;
pub use handler::{AuthResult, RtmpHandler};
pub use listener::RtmpServer;
