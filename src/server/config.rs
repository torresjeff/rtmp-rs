//! Server configuration

use std::net::SocketAddr;
use std::time::Duration;

use crate::media::fourcc::{AudioFourCc, VideoFourCc};
use crate::protocol::constants::*;
use crate::protocol::enhanced::{CapsEx, EnhancedRtmpMode, FourCcCapability};

/// Server configuration options
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Address to bind to
    pub bind_addr: SocketAddr,

    /// Maximum concurrent connections (0 = unlimited)
    pub max_connections: usize,

    /// Chunk size to negotiate with clients
    pub chunk_size: u32,

    /// Window acknowledgement size
    pub window_ack_size: u32,

    /// Peer bandwidth limit
    pub peer_bandwidth: u32,

    /// Connection timeout (handshake must complete within this time)
    pub connection_timeout: Duration,

    /// Idle timeout (disconnect if no data received)
    pub idle_timeout: Duration,

    /// Enable TCP_NODELAY (disable Nagle's algorithm)
    pub tcp_nodelay: bool,

    /// TCP receive buffer size (0 = OS default)
    pub tcp_recv_buffer: usize,

    /// TCP send buffer size (0 = OS default)
    pub tcp_send_buffer: usize,

    /// Application-level read buffer size
    pub read_buffer_size: usize,

    /// Application-level write buffer size
    pub write_buffer_size: usize,

    /// Enable GOP buffering for late-joiner support
    pub gop_buffer_enabled: bool,

    /// Maximum GOP buffer size in bytes
    pub gop_buffer_max_size: usize,

    /// Stats update interval
    pub stats_interval: Duration,

    /// Enhanced RTMP mode (Auto, LegacyOnly, or EnhancedOnly)
    pub enhanced_rtmp: EnhancedRtmpMode,

    /// Enhanced RTMP server capabilities to advertise
    pub enhanced_capabilities: EnhancedServerCapabilities,
}

/// Server-side Enhanced RTMP capabilities.
///
/// Configure which E-RTMP features and codecs the server supports.
#[derive(Debug, Clone)]
pub struct EnhancedServerCapabilities {
    /// Support for NetConnection.Connect.ReconnectRequest
    pub reconnect: bool,

    /// Support for multitrack audio/video streams
    pub multitrack: bool,

    /// Support for ModEx signal parsing (nanosecond timestamps, etc.)
    pub modex: bool,

    /// Video codecs supported with their capabilities
    pub video_codecs: Vec<(VideoFourCc, FourCcCapability)>,

    /// Audio codecs supported with their capabilities
    pub audio_codecs: Vec<(AudioFourCc, FourCcCapability)>,
}

impl Default for EnhancedServerCapabilities {
    fn default() -> Self {
        Self {
            reconnect: false,
            multitrack: false,
            modex: true, // Parse ModEx but don't require it
            video_codecs: vec![
                (VideoFourCc::Avc, FourCcCapability::forward()),
                (VideoFourCc::Hevc, FourCcCapability::forward()),
                (VideoFourCc::Av1, FourCcCapability::forward()),
                (VideoFourCc::Vp9, FourCcCapability::forward()),
            ],
            audio_codecs: vec![
                (AudioFourCc::Aac, FourCcCapability::forward()),
                (AudioFourCc::Opus, FourCcCapability::forward()),
            ],
        }
    }
}

impl EnhancedServerCapabilities {
    /// Create capabilities with no codec support (minimal E-RTMP).
    pub fn minimal() -> Self {
        Self {
            reconnect: false,
            multitrack: false,
            modex: true,
            video_codecs: vec![],
            audio_codecs: vec![],
        }
    }

    /// Add a video codec with specified capability.
    pub fn with_video_codec(mut self, codec: VideoFourCc, cap: FourCcCapability) -> Self {
        self.video_codecs.push((codec, cap));
        self
    }

    /// Add an audio codec with specified capability.
    pub fn with_audio_codec(mut self, codec: AudioFourCc, cap: FourCcCapability) -> Self {
        self.audio_codecs.push((codec, cap));
        self
    }

    /// Enable reconnect support.
    pub fn with_reconnect(mut self) -> Self {
        self.reconnect = true;
        self
    }

    /// Enable multitrack support.
    pub fn with_multitrack(mut self) -> Self {
        self.multitrack = true;
        self
    }

    /// Convert to CapsEx bitmask for protocol encoding.
    pub fn to_caps_ex(&self) -> CapsEx {
        let mut caps = CapsEx::empty();
        if self.reconnect {
            caps.insert(CapsEx::RECONNECT);
        }
        if self.multitrack {
            caps.insert(CapsEx::MULTITRACK);
        }
        if self.modex {
            caps.insert(CapsEx::MODEX);
        }
        caps
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: "0.0.0.0:1935".parse().unwrap(),
            max_connections: 0, // Unlimited
            chunk_size: RECOMMENDED_CHUNK_SIZE,
            window_ack_size: DEFAULT_WINDOW_ACK_SIZE,
            peer_bandwidth: DEFAULT_PEER_BANDWIDTH,
            connection_timeout: Duration::from_secs(10),
            idle_timeout: Duration::from_secs(60),
            tcp_nodelay: true, // Important for low latency
            tcp_recv_buffer: 0,
            tcp_send_buffer: 0,
            read_buffer_size: 64 * 1024, // 64KB
            write_buffer_size: 64 * 1024,
            gop_buffer_enabled: true,
            gop_buffer_max_size: 4 * 1024 * 1024, // 4MB
            stats_interval: Duration::from_secs(5),
            enhanced_rtmp: EnhancedRtmpMode::Auto,
            enhanced_capabilities: EnhancedServerCapabilities::default(),
        }
    }
}

impl ServerConfig {
    /// Create a new config with custom bind address
    pub fn with_addr(addr: SocketAddr) -> Self {
        Self {
            bind_addr: addr,
            ..Default::default()
        }
    }

    /// Set the bind address
    pub fn bind(mut self, addr: SocketAddr) -> Self {
        self.bind_addr = addr;
        self
    }

    /// Set maximum connections
    pub fn max_connections(mut self, max: usize) -> Self {
        self.max_connections = max;
        self
    }

    /// Set chunk size
    pub fn chunk_size(mut self, size: u32) -> Self {
        self.chunk_size = size.min(MAX_CHUNK_SIZE);
        self
    }

    /// Disable GOP buffering
    pub fn disable_gop_buffer(mut self) -> Self {
        self.gop_buffer_enabled = false;
        self
    }

    /// Set connection timeout
    pub fn connection_timeout(mut self, timeout: Duration) -> Self {
        self.connection_timeout = timeout;
        self
    }

    /// Set idle timeout
    pub fn idle_timeout(mut self, timeout: Duration) -> Self {
        self.idle_timeout = timeout;
        self
    }

    /// Set Enhanced RTMP mode.
    ///
    /// - `Auto`: Negotiate E-RTMP if client supports it (default)
    /// - `LegacyOnly`: Use legacy RTMP only
    /// - `EnhancedOnly`: Require E-RTMP, reject legacy clients
    pub fn enhanced_rtmp(mut self, mode: EnhancedRtmpMode) -> Self {
        self.enhanced_rtmp = mode;
        self
    }

    /// Set Enhanced RTMP server capabilities.
    pub fn enhanced_capabilities(mut self, caps: EnhancedServerCapabilities) -> Self {
        self.enhanced_capabilities = caps;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ServerConfig::default();

        assert_eq!(config.bind_addr.port(), 1935);
        assert_eq!(config.max_connections, 0);
        assert_eq!(config.chunk_size, RECOMMENDED_CHUNK_SIZE);
        assert_eq!(config.window_ack_size, DEFAULT_WINDOW_ACK_SIZE);
        assert_eq!(config.peer_bandwidth, DEFAULT_PEER_BANDWIDTH);
        assert!(config.tcp_nodelay);
        assert!(config.gop_buffer_enabled);
        assert_eq!(config.enhanced_rtmp, EnhancedRtmpMode::Auto);
    }

    #[test]
    fn test_with_addr() {
        let addr: SocketAddr = "127.0.0.1:1936".parse().unwrap();
        let config = ServerConfig::with_addr(addr);

        assert_eq!(config.bind_addr.port(), 1936);
    }

    #[test]
    fn test_builder_bind() {
        let addr: SocketAddr = "0.0.0.0:8080".parse().unwrap();
        let config = ServerConfig::default().bind(addr);

        assert_eq!(config.bind_addr, addr);
    }

    #[test]
    fn test_builder_max_connections() {
        let config = ServerConfig::default().max_connections(100);

        assert_eq!(config.max_connections, 100);
    }

    #[test]
    fn test_builder_chunk_size() {
        let config = ServerConfig::default().chunk_size(8192);

        assert_eq!(config.chunk_size, 8192);
    }

    #[test]
    fn test_builder_chunk_size_capped() {
        // Chunk size should be capped at MAX_CHUNK_SIZE
        let config = ServerConfig::default().chunk_size(u32::MAX);

        assert_eq!(config.chunk_size, MAX_CHUNK_SIZE);
    }

    #[test]
    fn test_builder_disable_gop_buffer() {
        let config = ServerConfig::default().disable_gop_buffer();

        assert!(!config.gop_buffer_enabled);
    }

    #[test]
    fn test_builder_connection_timeout() {
        let config = ServerConfig::default().connection_timeout(Duration::from_secs(30));

        assert_eq!(config.connection_timeout, Duration::from_secs(30));
    }

    #[test]
    fn test_builder_idle_timeout() {
        let config = ServerConfig::default().idle_timeout(Duration::from_secs(120));

        assert_eq!(config.idle_timeout, Duration::from_secs(120));
    }

    #[test]
    fn test_builder_chaining() {
        let addr: SocketAddr = "127.0.0.1:1935".parse().unwrap();
        let config = ServerConfig::default()
            .bind(addr)
            .max_connections(50)
            .chunk_size(4096)
            .connection_timeout(Duration::from_secs(5))
            .idle_timeout(Duration::from_secs(30))
            .disable_gop_buffer();

        assert_eq!(config.bind_addr, addr);
        assert_eq!(config.max_connections, 50);
        assert_eq!(config.chunk_size, 4096);
        assert_eq!(config.connection_timeout, Duration::from_secs(5));
        assert_eq!(config.idle_timeout, Duration::from_secs(30));
        assert!(!config.gop_buffer_enabled);
    }

    #[test]
    fn test_enhanced_rtmp_mode() {
        let config = ServerConfig::default().enhanced_rtmp(EnhancedRtmpMode::EnhancedOnly);
        assert_eq!(config.enhanced_rtmp, EnhancedRtmpMode::EnhancedOnly);

        let config = ServerConfig::default().enhanced_rtmp(EnhancedRtmpMode::LegacyOnly);
        assert_eq!(config.enhanced_rtmp, EnhancedRtmpMode::LegacyOnly);
    }

    #[test]
    fn test_enhanced_server_capabilities_default() {
        let caps = EnhancedServerCapabilities::default();

        assert!(!caps.reconnect);
        assert!(!caps.multitrack);
        assert!(caps.modex);
        assert!(!caps.video_codecs.is_empty());
        assert!(!caps.audio_codecs.is_empty());

        // Check default video codecs
        assert!(caps
            .video_codecs
            .iter()
            .any(|(c, _)| *c == VideoFourCc::Avc));
        assert!(caps
            .video_codecs
            .iter()
            .any(|(c, _)| *c == VideoFourCc::Hevc));
        assert!(caps
            .video_codecs
            .iter()
            .any(|(c, _)| *c == VideoFourCc::Av1));

        // Check default audio codecs
        assert!(caps
            .audio_codecs
            .iter()
            .any(|(c, _)| *c == AudioFourCc::Aac));
        assert!(caps
            .audio_codecs
            .iter()
            .any(|(c, _)| *c == AudioFourCc::Opus));
    }

    #[test]
    fn test_enhanced_server_capabilities_minimal() {
        let caps = EnhancedServerCapabilities::minimal();

        assert!(!caps.reconnect);
        assert!(!caps.multitrack);
        assert!(caps.modex);
        assert!(caps.video_codecs.is_empty());
        assert!(caps.audio_codecs.is_empty());
    }

    #[test]
    fn test_enhanced_server_capabilities_builder() {
        let caps = EnhancedServerCapabilities::minimal()
            .with_video_codec(VideoFourCc::Hevc, FourCcCapability::full())
            .with_audio_codec(AudioFourCc::Opus, FourCcCapability::decode())
            .with_reconnect()
            .with_multitrack();

        assert!(caps.reconnect);
        assert!(caps.multitrack);
        assert_eq!(caps.video_codecs.len(), 1);
        assert_eq!(caps.audio_codecs.len(), 1);

        let (codec, cap) = &caps.video_codecs[0];
        assert_eq!(*codec, VideoFourCc::Hevc);
        assert!(cap.can_decode());
        assert!(cap.can_encode());
        assert!(cap.can_forward());
    }

    #[test]
    fn test_enhanced_server_capabilities_to_caps_ex() {
        let caps = EnhancedServerCapabilities::default();
        let caps_ex = caps.to_caps_ex();

        assert!(!caps_ex.supports_reconnect());
        assert!(!caps_ex.supports_multitrack());
        assert!(caps_ex.supports_modex());

        let caps_full = EnhancedServerCapabilities::default()
            .with_reconnect()
            .with_multitrack();
        let caps_ex = caps_full.to_caps_ex();

        assert!(caps_ex.supports_reconnect());
        assert!(caps_ex.supports_multitrack());
        assert!(caps_ex.supports_modex());
    }

    #[test]
    fn test_config_with_enhanced_capabilities() {
        let caps = EnhancedServerCapabilities::minimal()
            .with_video_codec(VideoFourCc::Av1, FourCcCapability::forward());

        let config = ServerConfig::default()
            .enhanced_rtmp(EnhancedRtmpMode::Auto)
            .enhanced_capabilities(caps);

        assert_eq!(config.enhanced_rtmp, EnhancedRtmpMode::Auto);
        assert_eq!(config.enhanced_capabilities.video_codecs.len(), 1);
    }
}
