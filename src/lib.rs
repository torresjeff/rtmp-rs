//! rtmp-rs: Production RTMP client/server library
//!
//! This library provides a complete RTMP implementation supporting:
//! - Server mode for receiving streams from OBS, ffmpeg, etc.
//! - Client mode for pulling streams from remote RTMP servers
//! - H.264 video and AAC audio codec support
//! - GOP buffering for late-joiner support
//! - Lenient parsing for encoder compatibility (OBS quirks)
//!
//! # Example: Simple Server
//!
//! ```no_run
//! use rtmp_rs::{RtmpServer, ServerConfig, RtmpHandler, AuthResult};
//! use rtmp_rs::session::SessionContext;
//! use rtmp_rs::protocol::message::{ConnectParams, PublishParams};
//!
//! struct MyHandler;
//!
//! #[async_trait::async_trait]
//! impl RtmpHandler for MyHandler {
//!     async fn on_connect(&self, _ctx: &SessionContext, _params: &ConnectParams) -> AuthResult {
//!         AuthResult::Accept
//!     }
//!
//!     async fn on_publish(&self, _ctx: &SessionContext, params: &PublishParams) -> AuthResult {
//!         println!("Stream published: {}", params.stream_key);
//!         AuthResult::Accept
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let server = RtmpServer::new(ServerConfig::default(), MyHandler);
//!     server.run().await?;
//!     Ok(())
//! }
//! ```

pub mod amf;
pub mod client;
pub mod error;
pub mod media;
pub mod protocol;
pub mod server;
pub mod session;
pub mod stats;

// Re-export main types for convenience
pub use error::{Error, Result};
pub use server::handler::{AuthResult, RtmpHandler};
pub use server::listener::RtmpServer;
pub use server::config::ServerConfig;
pub use client::connector::RtmpConnector;
pub use client::puller::{RtmpPuller, ClientEvent};
pub use client::config::ClientConfig;
