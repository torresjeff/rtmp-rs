//! Handler context
//!
//! Context passed to handler callbacks containing session information
//! and methods to interact with the connection.

use std::net::SocketAddr;
use std::sync::Arc;

use crate::protocol::enhanced::EnhancedCapabilities;
use crate::protocol::message::ConnectParams;
use crate::protocol::quirks::EncoderType;
use crate::stats::SessionStats;

/// Context passed to RtmpHandler callbacks
///
/// Provides read-only access to session information. For operations
/// that modify state, use the return values from handler methods.
#[derive(Debug, Clone)]
pub struct SessionContext {
    /// Unique session ID
    pub session_id: u64,

    /// Remote peer address
    pub peer_addr: SocketAddr,

    /// Application name (from connect)
    pub app: String,

    /// Detected encoder type
    pub encoder_type: EncoderType,

    /// Connect parameters (if available)
    pub connect_params: Option<Arc<ConnectParams>>,

    /// Negotiated E-RTMP capabilities (if E-RTMP is active)
    pub enhanced_capabilities: Option<EnhancedCapabilities>,

    /// Current session statistics
    pub stats: SessionStats,
}

impl SessionContext {
    /// Create a new context
    pub fn new(session_id: u64, peer_addr: SocketAddr) -> Self {
        Self {
            session_id,
            peer_addr,
            app: String::new(),
            encoder_type: EncoderType::Unknown,
            connect_params: None,
            enhanced_capabilities: None,
            stats: SessionStats::default(),
        }
    }

    /// Update with connect parameters
    pub fn with_connect(&mut self, params: ConnectParams, encoder_type: EncoderType) {
        self.app = params.app.clone();
        self.encoder_type = encoder_type;
        self.connect_params = Some(Arc::new(params));
    }

    /// Set negotiated E-RTMP capabilities
    pub fn with_enhanced_capabilities(&mut self, caps: EnhancedCapabilities) {
        if caps.enabled {
            self.enhanced_capabilities = Some(caps);
        }
    }

    /// Check if E-RTMP is active for this session
    pub fn is_enhanced_rtmp(&self) -> bool {
        self.enhanced_capabilities
            .as_ref()
            .map(|c| c.enabled)
            .unwrap_or(false)
    }

    /// Get the TC URL if available
    pub fn tc_url(&self) -> Option<&str> {
        self.connect_params
            .as_ref()
            .and_then(|p| p.tc_url.as_deref())
    }

    /// Get the page URL if available
    pub fn page_url(&self) -> Option<&str> {
        self.connect_params
            .as_ref()
            .and_then(|p| p.page_url.as_deref())
    }

    /// Get the flash version string if available
    pub fn flash_ver(&self) -> Option<&str> {
        self.connect_params
            .as_ref()
            .and_then(|p| p.flash_ver.as_deref())
    }
}

/// Stream context passed to media callbacks
#[derive(Debug, Clone)]
pub struct StreamContext {
    /// Parent session context
    pub session: SessionContext,

    /// Message stream ID
    pub stream_id: u32,

    /// Stream key
    pub stream_key: String,

    /// Whether this is a publishing or playing stream
    pub is_publishing: bool,
}

impl StreamContext {
    /// Create a new stream context
    pub fn new(
        session: SessionContext,
        stream_id: u32,
        stream_key: String,
        is_publishing: bool,
    ) -> Self {
        Self {
            session,
            stream_id,
            stream_key,
            is_publishing,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    fn make_test_addr() -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)), 54321)
    }

    #[test]
    fn test_session_context_new() {
        let addr = make_test_addr();
        let ctx = SessionContext::new(42, addr);

        assert_eq!(ctx.session_id, 42);
        assert_eq!(ctx.peer_addr, addr);
        assert_eq!(ctx.app, "");
        assert_eq!(ctx.encoder_type, EncoderType::Unknown);
        assert!(ctx.connect_params.is_none());
    }

    #[test]
    fn test_session_context_with_connect() {
        let addr = make_test_addr();
        let mut ctx = SessionContext::new(1, addr);

        let mut params = ConnectParams::default();
        params.app = "live".to_string();
        params.tc_url = Some("rtmp://localhost/live".to_string());
        params.flash_ver = Some("FMLE/3.0".to_string());
        params.page_url = Some("http://example.com".to_string());

        ctx.with_connect(params, EncoderType::Obs);

        assert_eq!(ctx.app, "live");
        assert_eq!(ctx.encoder_type, EncoderType::Obs);
        assert!(ctx.connect_params.is_some());
    }

    #[test]
    fn test_session_context_tc_url() {
        let addr = make_test_addr();
        let mut ctx = SessionContext::new(1, addr);

        // Before connect, tc_url should be None
        assert!(ctx.tc_url().is_none());

        // After connect with tc_url
        let mut params = ConnectParams::default();
        params.tc_url = Some("rtmp://server/app".to_string());
        ctx.with_connect(params, EncoderType::Unknown);

        assert_eq!(ctx.tc_url(), Some("rtmp://server/app"));
    }

    #[test]
    fn test_session_context_page_url() {
        let addr = make_test_addr();
        let mut ctx = SessionContext::new(1, addr);

        // Before connect
        assert!(ctx.page_url().is_none());

        // After connect with page_url
        let mut params = ConnectParams::default();
        params.page_url = Some("http://twitch.tv".to_string());
        ctx.with_connect(params, EncoderType::Unknown);

        assert_eq!(ctx.page_url(), Some("http://twitch.tv"));
    }

    #[test]
    fn test_session_context_flash_ver() {
        let addr = make_test_addr();
        let mut ctx = SessionContext::new(1, addr);

        // Before connect
        assert!(ctx.flash_ver().is_none());

        // After connect with flash_ver
        let mut params = ConnectParams::default();
        params.flash_ver = Some("OBS-Studio/29.1.3".to_string());
        ctx.with_connect(params, EncoderType::Obs);

        assert_eq!(ctx.flash_ver(), Some("OBS-Studio/29.1.3"));
    }

    #[test]
    fn test_session_context_no_optional_params() {
        let addr = make_test_addr();
        let mut ctx = SessionContext::new(1, addr);

        // Connect with minimal params (no tc_url, page_url, flash_ver)
        let params = ConnectParams::default();
        ctx.with_connect(params, EncoderType::Ffmpeg);

        assert!(ctx.tc_url().is_none());
        assert!(ctx.page_url().is_none());
        assert!(ctx.flash_ver().is_none());
    }

    #[test]
    fn test_stream_context_new() {
        let addr = make_test_addr();
        let session_ctx = SessionContext::new(1, addr);

        let stream_ctx =
            StreamContext::new(session_ctx.clone(), 5, "my_stream_key".to_string(), true);

        assert_eq!(stream_ctx.stream_id, 5);
        assert_eq!(stream_ctx.stream_key, "my_stream_key");
        assert!(stream_ctx.is_publishing);
        assert_eq!(stream_ctx.session.session_id, 1);
    }

    #[test]
    fn test_stream_context_playing() {
        let addr = make_test_addr();
        let session_ctx = SessionContext::new(2, addr);

        let stream_ctx = StreamContext::new(session_ctx, 10, "viewer_stream".to_string(), false);

        assert_eq!(stream_ctx.stream_id, 10);
        assert_eq!(stream_ctx.stream_key, "viewer_stream");
        assert!(!stream_ctx.is_publishing);
    }

    #[test]
    fn test_session_context_clone() {
        let addr = make_test_addr();
        let mut ctx = SessionContext::new(1, addr);

        let mut params = ConnectParams::default();
        params.app = "test".to_string();
        ctx.with_connect(params, EncoderType::Wirecast);

        let cloned = ctx.clone();

        assert_eq!(cloned.session_id, ctx.session_id);
        assert_eq!(cloned.app, ctx.app);
        assert_eq!(cloned.encoder_type, ctx.encoder_type);
    }
}
