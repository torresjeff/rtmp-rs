//! RTMP handler trait
//!
//! The main extension point for RTMP applications. Implement this trait
//! to handle connection events, authentication, and media data.

use async_trait::async_trait;
use bytes::Bytes;
use std::collections::HashMap;

use crate::amf::AmfValue;
use crate::media::{AacData, FlvTag, H264Data};
use crate::protocol::message::{ConnectParams, PublishParams, PlayParams};
use crate::session::{SessionContext, StreamContext};

/// Result of authentication/authorization checks
#[derive(Debug, Clone)]
pub enum AuthResult {
    /// Accept the request
    Accept,

    /// Reject the request with a reason
    Reject(String),

    /// Redirect to another URL
    Redirect { url: String },
}

impl AuthResult {
    /// Check if the result is Accept
    pub fn is_accept(&self) -> bool {
        matches!(self, AuthResult::Accept)
    }

    /// Check if the result is Reject
    pub fn is_reject(&self) -> bool {
        matches!(self, AuthResult::Reject(_))
    }
}

/// Media delivery mode configuration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MediaDeliveryMode {
    /// Deliver raw FLV tags (minimal parsing)
    RawFlv,

    /// Deliver parsed frames (H.264 NALUs, AAC frames)
    ParsedFrames,

    /// Deliver both raw tags and parsed frames
    #[default]
    Both,
}

/// Handler trait for RTMP applications
///
/// Implement this trait to customize RTMP server behavior. All methods
/// have default implementations that accept/allow everything.
///
/// # Example
///
/// ```ignore
/// use rtmp_rs::{RtmpHandler, AuthResult};
/// use rtmp_rs::session::SessionContext;
/// use rtmp_rs::protocol::message::{ConnectParams, PublishParams};
///
/// struct MyHandler;
///
/// #[async_trait::async_trait]
/// impl RtmpHandler for MyHandler {
///     async fn on_connect(&self, ctx: &SessionContext, params: &ConnectParams) -> AuthResult {
///         // Validate application name
///         if params.app == "live" {
///             AuthResult::Accept
///         } else {
///             AuthResult::Reject("Unknown application".into())
///         }
///     }
///
///     async fn on_publish(&self, ctx: &SessionContext, params: &PublishParams) -> AuthResult {
///         // Validate stream key (e.g., check against database)
///         if params.stream_key.starts_with("valid_") {
///             AuthResult::Accept
///         } else {
///             AuthResult::Reject("Invalid stream key".into())
///         }
///     }
/// }
/// ```
#[async_trait]
pub trait RtmpHandler: Send + Sync + 'static {
    /// Called when a new TCP connection is established
    ///
    /// Return false to immediately close the connection.
    /// Use this for IP-based rate limiting or blocklists.
    async fn on_connection(&self, _ctx: &SessionContext) -> bool {
        true
    }

    /// Called after successful handshake, before connect command
    async fn on_handshake_complete(&self, _ctx: &SessionContext) {}

    /// Called on RTMP 'connect' command
    ///
    /// Validate the application name, auth tokens in tcUrl, etc.
    async fn on_connect(&self, _ctx: &SessionContext, _params: &ConnectParams) -> AuthResult {
        AuthResult::Accept
    }

    /// Called on FCPublish command (OBS/Twitch compatibility)
    ///
    /// This is called before 'publish' and can be used for early stream key validation.
    async fn on_fc_publish(&self, _ctx: &SessionContext, _stream_key: &str) -> AuthResult {
        AuthResult::Accept
    }

    /// Called on 'publish' command
    ///
    /// Validate the stream key. This is the main authentication point for publishers.
    async fn on_publish(&self, _ctx: &SessionContext, _params: &PublishParams) -> AuthResult {
        AuthResult::Accept
    }

    /// Called on 'play' command
    ///
    /// Validate play access. Return Reject to deny playback.
    async fn on_play(&self, _ctx: &SessionContext, _params: &PlayParams) -> AuthResult {
        AuthResult::Accept
    }

    /// Called when stream metadata is received (@setDataFrame/onMetaData)
    async fn on_metadata(&self, _ctx: &StreamContext, _metadata: &HashMap<String, AmfValue>) {}

    /// Called for each raw FLV tag (when MediaDeliveryMode includes RawFlv)
    ///
    /// Return true to continue processing, false to drop the tag.
    async fn on_media_tag(&self, _ctx: &StreamContext, _tag: &FlvTag) -> bool {
        true
    }

    /// Called for each video frame (when MediaDeliveryMode includes ParsedFrames)
    async fn on_video_frame(&self, _ctx: &StreamContext, _frame: &H264Data, _timestamp: u32) {}

    /// Called for each audio frame (when MediaDeliveryMode includes ParsedFrames)
    async fn on_audio_frame(&self, _ctx: &StreamContext, _frame: &AacData, _timestamp: u32) {}

    /// Called when a keyframe is received
    async fn on_keyframe(&self, _ctx: &StreamContext, _timestamp: u32) {}

    /// Called when the publish stream ends
    async fn on_publish_stop(&self, _ctx: &StreamContext) {}

    /// Called when the play stream ends
    async fn on_play_stop(&self, _ctx: &StreamContext) {}

    /// Called when the connection closes
    async fn on_disconnect(&self, _ctx: &SessionContext) {}

    /// Get the media delivery mode for this handler
    fn media_delivery_mode(&self) -> MediaDeliveryMode {
        MediaDeliveryMode::Both
    }

    /// Called periodically with stats update
    async fn on_stats_update(&self, _ctx: &SessionContext) {}
}

/// A simple handler that accepts all connections and logs events
pub struct LoggingHandler;

#[async_trait]
impl RtmpHandler for LoggingHandler {
    async fn on_connection(&self, ctx: &SessionContext) -> bool {
        tracing::info!(
            session_id = ctx.session_id,
            peer = %ctx.peer_addr,
            "New connection"
        );
        true
    }

    async fn on_connect(&self, ctx: &SessionContext, params: &ConnectParams) -> AuthResult {
        tracing::info!(
            session_id = ctx.session_id,
            app = %params.app,
            "Connect request"
        );
        AuthResult::Accept
    }

    async fn on_publish(&self, ctx: &SessionContext, params: &PublishParams) -> AuthResult {
        tracing::info!(
            session_id = ctx.session_id,
            stream_key = %params.stream_key,
            "Publish request"
        );
        AuthResult::Accept
    }

    async fn on_metadata(&self, ctx: &StreamContext, metadata: &HashMap<String, AmfValue>) {
        tracing::debug!(
            session_id = ctx.session.session_id,
            stream_key = %ctx.stream_key,
            keys = ?metadata.keys().collect::<Vec<_>>(),
            "Received metadata"
        );
    }

    async fn on_disconnect(&self, ctx: &SessionContext) {
        tracing::info!(
            session_id = ctx.session_id,
            "Connection closed"
        );
    }
}

/// A handler wrapper that chains multiple handlers
pub struct ChainedHandler<H1, H2> {
    first: H1,
    second: H2,
}

impl<H1, H2> ChainedHandler<H1, H2>
where
    H1: RtmpHandler,
    H2: RtmpHandler,
{
    pub fn new(first: H1, second: H2) -> Self {
        Self { first, second }
    }
}

#[async_trait]
impl<H1, H2> RtmpHandler for ChainedHandler<H1, H2>
where
    H1: RtmpHandler,
    H2: RtmpHandler,
{
    async fn on_connection(&self, ctx: &SessionContext) -> bool {
        self.first.on_connection(ctx).await && self.second.on_connection(ctx).await
    }

    async fn on_connect(&self, ctx: &SessionContext, params: &ConnectParams) -> AuthResult {
        let result = self.first.on_connect(ctx, params).await;
        if result.is_accept() {
            self.second.on_connect(ctx, params).await
        } else {
            result
        }
    }

    async fn on_publish(&self, ctx: &SessionContext, params: &PublishParams) -> AuthResult {
        let result = self.first.on_publish(ctx, params).await;
        if result.is_accept() {
            self.second.on_publish(ctx, params).await
        } else {
            result
        }
    }

    async fn on_disconnect(&self, ctx: &SessionContext) {
        self.first.on_disconnect(ctx).await;
        self.second.on_disconnect(ctx).await;
    }
}
