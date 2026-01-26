//! RTMP handler trait
//!
//! The main extension point for RTMP applications. Implement this trait
//! to handle connection events, authentication, and media data.

use std::collections::HashMap;

use crate::amf::AmfValue;
use crate::media::{AacData, EnhancedAudioData, EnhancedVideoData, FlvTag, H264Data};
use crate::protocol::message::{ConnectParams, PlayParams, PublishParams};
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
pub trait RtmpHandler: Send + Sync + 'static {
    /// Called when a new TCP connection is established
    ///
    /// Return false to immediately close the connection.
    /// Use this for IP-based rate limiting or blocklists.
    fn on_connection(
        &self,
        _ctx: &SessionContext,
    ) -> impl std::future::Future<Output = bool> + Send {
        async { true }
    }

    /// Called after successful handshake, before connect command
    fn on_handshake_complete(
        &self,
        _ctx: &SessionContext,
    ) -> impl std::future::Future<Output = ()> + Send {
        async {}
    }

    /// Called on RTMP 'connect' command
    ///
    /// Validate the application name, auth tokens in tcUrl, etc.
    fn on_connect(
        &self,
        _ctx: &SessionContext,
        _params: &ConnectParams,
    ) -> impl std::future::Future<Output = AuthResult> + Send {
        async { AuthResult::Accept }
    }

    /// Called on FCPublish command (OBS/Twitch compatibility)
    ///
    /// This is called before 'publish' and can be used for early stream key validation.
    fn on_fc_publish(
        &self,
        _ctx: &SessionContext,
        _stream_key: &str,
    ) -> impl std::future::Future<Output = AuthResult> + Send {
        async { AuthResult::Accept }
    }

    /// Called on 'publish' command
    ///
    /// Validate the stream key. This is the main authentication point for publishers.
    fn on_publish(
        &self,
        _ctx: &SessionContext,
        _params: &PublishParams,
    ) -> impl std::future::Future<Output = AuthResult> + Send {
        async { AuthResult::Accept }
    }

    /// Called on 'play' command
    ///
    /// Validate play access. Return Reject to deny playback.
    fn on_play(
        &self,
        _ctx: &SessionContext,
        _params: &PlayParams,
    ) -> impl std::future::Future<Output = AuthResult> + Send {
        async { AuthResult::Accept }
    }

    /// Called when stream metadata is received (@setDataFrame/onMetaData)
    fn on_metadata(
        &self,
        _ctx: &StreamContext,
        _metadata: &HashMap<String, AmfValue>,
    ) -> impl std::future::Future<Output = ()> + Send {
        async {}
    }

    /// Called for each raw FLV tag (when MediaDeliveryMode includes RawFlv)
    ///
    /// Return true to continue processing, false to drop the tag.
    fn on_media_tag(
        &self,
        _ctx: &StreamContext,
        _tag: &FlvTag,
    ) -> impl std::future::Future<Output = bool> + Send {
        async { true }
    }

    /// Called for each video frame (when MediaDeliveryMode includes ParsedFrames)
    fn on_video_frame(
        &self,
        _ctx: &StreamContext,
        _frame: &H264Data,
        _timestamp: u32,
    ) -> impl std::future::Future<Output = ()> + Send {
        async {}
    }

    /// Called for each audio frame (when MediaDeliveryMode includes ParsedFrames)
    fn on_audio_frame(
        &self,
        _ctx: &StreamContext,
        _frame: &AacData,
        _timestamp: u32,
    ) -> impl std::future::Future<Output = ()> + Send {
        async {}
    }

    /// Called when a keyframe is received
    fn on_keyframe(
        &self,
        _ctx: &StreamContext,
        _timestamp: u32,
    ) -> impl std::future::Future<Output = ()> + Send {
        async {}
    }

    /// Called when the publish stream ends
    #[deprecated(since = "0.3.0", note = "Use on_unpublish instead")]
    fn on_publish_stop(
        &self,
        _ctx: &StreamContext,
    ) -> impl std::future::Future<Output = ()> + Send {
        async {}
    }

    /// Called when the publish stream ends
    fn on_unpublish(&self, _ctx: &StreamContext) -> impl std::future::Future<Output = ()> + Send {
        async {}
    }

    /// Called when the play stream ends
    fn on_play_stop(&self, _ctx: &StreamContext) -> impl std::future::Future<Output = ()> + Send {
        async {}
    }

    /// Called when a subscriber pauses playback
    fn on_pause(&self, _ctx: &StreamContext) -> impl std::future::Future<Output = ()> + Send {
        async {}
    }

    /// Called when a subscriber resumes playback
    fn on_unpause(&self, _ctx: &StreamContext) -> impl std::future::Future<Output = ()> + Send {
        async {}
    }

    /// Called when the connection closes
    fn on_disconnect(&self, _ctx: &SessionContext) -> impl std::future::Future<Output = ()> + Send {
        async {}
    }

    /// Get the media delivery mode for this handler
    fn media_delivery_mode(&self) -> MediaDeliveryMode {
        MediaDeliveryMode::Both
    }

    /// Called periodically with stats update
    fn on_stats_update(
        &self,
        _ctx: &SessionContext,
    ) -> impl std::future::Future<Output = ()> + Send {
        async {}
    }

    // =========================================================================
    // Enhanced RTMP (E-RTMP) callbacks
    // =========================================================================

    /// Called for each enhanced video frame (E-RTMP mode).
    ///
    /// This is called when the client sends video using enhanced RTMP format
    /// (HEVC, AV1, VP9, etc. via FOURCC signaling).
    ///
    /// Default implementation does nothing. Implement this to handle
    /// modern video codecs beyond H.264.
    fn on_enhanced_video_frame(
        &self,
        _ctx: &StreamContext,
        _frame: &EnhancedVideoData,
        _timestamp: u32,
    ) -> impl std::future::Future<Output = ()> + Send {
        async {}
    }

    /// Called for each enhanced audio frame (E-RTMP mode).
    ///
    /// This is called when the client sends audio using enhanced RTMP format
    /// (Opus, FLAC, AC-3, etc. via FOURCC signaling).
    ///
    /// Default implementation does nothing. Implement this to handle
    /// modern audio codecs beyond AAC.
    fn on_enhanced_audio_frame(
        &self,
        _ctx: &StreamContext,
        _frame: &EnhancedAudioData,
        _timestamp: u32,
    ) -> impl std::future::Future<Output = ()> + Send {
        async {}
    }
}

/// A simple handler that accepts all connections and logs events
pub struct LoggingHandler;

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
        tracing::info!(session_id = ctx.session_id, "Connection closed");
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
