//! Client configuration

use std::time::Duration;

use crate::media::fourcc::{AudioFourCc, VideoFourCc};
use crate::protocol::enhanced::{CapsEx, EnhancedRtmpMode, FourCcCapability};

/// Client configuration
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// RTMP URL to connect to (rtmp://host[:port]/app/stream)
    pub url: String,

    /// Connection timeout
    pub connect_timeout: Duration,

    /// Read timeout
    pub read_timeout: Duration,

    /// Enable TCP_NODELAY
    pub tcp_nodelay: bool,

    /// Flash version string to send
    pub flash_ver: String,

    /// SWF URL to send
    pub swf_url: Option<String>,

    /// Page URL to send
    pub page_url: Option<String>,

    /// Receive buffer size in milliseconds
    pub buffer_length: u32,

    /// Enhanced RTMP mode (Auto, LegacyOnly, or EnhancedOnly)
    pub enhanced_rtmp: EnhancedRtmpMode,

    /// Enhanced RTMP client capabilities to advertise
    pub enhanced_capabilities: EnhancedClientCapabilities,
}

/// Client-side Enhanced RTMP capabilities.
///
/// Configure which E-RTMP features and codecs the client supports.
#[derive(Debug, Clone)]
pub struct EnhancedClientCapabilities {
    /// Support for NetConnection.Connect.ReconnectRequest
    pub reconnect: bool,

    /// Support for multitrack audio/video streams
    pub multitrack: bool,

    /// Support for ModEx signal parsing
    pub modex: bool,

    /// Video codecs supported with their capabilities
    pub video_codecs: Vec<(VideoFourCc, FourCcCapability)>,

    /// Audio codecs supported with their capabilities
    pub audio_codecs: Vec<(AudioFourCc, FourCcCapability)>,
}

impl Default for EnhancedClientCapabilities {
    fn default() -> Self {
        Self {
            reconnect: false,
            multitrack: false,
            modex: true,
            video_codecs: vec![
                (VideoFourCc::Avc, FourCcCapability::decode()),
                (VideoFourCc::Hevc, FourCcCapability::decode()),
                (VideoFourCc::Av1, FourCcCapability::decode()),
            ],
            audio_codecs: vec![
                (AudioFourCc::Aac, FourCcCapability::decode()),
                (AudioFourCc::Opus, FourCcCapability::decode()),
            ],
        }
    }
}

impl EnhancedClientCapabilities {
    /// Create capabilities with no codec support.
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

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            connect_timeout: Duration::from_secs(10),
            read_timeout: Duration::from_secs(30),
            tcp_nodelay: true,
            flash_ver: "LNX 9,0,124,2".to_string(),
            swf_url: None,
            page_url: None,
            buffer_length: 1000,
            enhanced_rtmp: EnhancedRtmpMode::Auto,
            enhanced_capabilities: EnhancedClientCapabilities::default(),
        }
    }
}

impl ClientConfig {
    /// Create a new config with the given URL
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            ..Default::default()
        }
    }

    /// Set Enhanced RTMP mode.
    ///
    /// - `Auto`: Negotiate E-RTMP if server supports it (default)
    /// - `LegacyOnly`: Use legacy RTMP only
    /// - `EnhancedOnly`: Require E-RTMP, fail if server doesn't support it
    pub fn enhanced_rtmp(mut self, mode: EnhancedRtmpMode) -> Self {
        self.enhanced_rtmp = mode;
        self
    }

    /// Set Enhanced RTMP client capabilities.
    pub fn enhanced_capabilities(mut self, caps: EnhancedClientCapabilities) -> Self {
        self.enhanced_capabilities = caps;
        self
    }

    /// Parse URL into components
    pub fn parse_url(&self) -> Option<ParsedUrl> {
        // rtmp://host[:port]/app[/stream]
        let url = self.url.strip_prefix("rtmp://")?;

        let (host_port, path) = url.split_once('/')?;
        let (host, port) = if let Some((h, p)) = host_port.split_once(':') {
            (h.to_string(), p.parse().ok()?)
        } else {
            (host_port.to_string(), 1935)
        };

        let (app, stream_key) = if let Some((a, s)) = path.split_once('/') {
            (a.to_string(), Some(s.to_string()))
        } else {
            (path.to_string(), None)
        };

        Some(ParsedUrl {
            host,
            port,
            app,
            stream_key,
        })
    }
}

/// Parsed RTMP URL components
#[derive(Debug, Clone)]
pub struct ParsedUrl {
    pub host: String,
    pub port: u16,
    pub app: String,
    pub stream_key: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_parsing() {
        let config = ClientConfig::new("rtmp://localhost/live/test");
        let parsed = config.parse_url().unwrap();
        assert_eq!(parsed.host, "localhost");
        assert_eq!(parsed.port, 1935);
        assert_eq!(parsed.app, "live");
        assert_eq!(parsed.stream_key, Some("test".into()));

        let config = ClientConfig::new("rtmp://example.com:1936/app");
        let parsed = config.parse_url().unwrap();
        assert_eq!(parsed.host, "example.com");
        assert_eq!(parsed.port, 1936);
        assert_eq!(parsed.app, "app");
        assert_eq!(parsed.stream_key, None);
    }

    #[test]
    fn test_default_config_enhanced_rtmp() {
        let config = ClientConfig::default();
        assert_eq!(config.enhanced_rtmp, EnhancedRtmpMode::Auto);
    }

    #[test]
    fn test_enhanced_rtmp_mode() {
        let config = ClientConfig::new("rtmp://localhost/live/test")
            .enhanced_rtmp(EnhancedRtmpMode::EnhancedOnly);
        assert_eq!(config.enhanced_rtmp, EnhancedRtmpMode::EnhancedOnly);

        let config = ClientConfig::new("rtmp://localhost/live/test")
            .enhanced_rtmp(EnhancedRtmpMode::LegacyOnly);
        assert_eq!(config.enhanced_rtmp, EnhancedRtmpMode::LegacyOnly);
    }

    #[test]
    fn test_enhanced_client_capabilities_default() {
        let caps = EnhancedClientCapabilities::default();

        assert!(!caps.reconnect);
        assert!(!caps.multitrack);
        assert!(caps.modex);
        assert!(!caps.video_codecs.is_empty());
        assert!(!caps.audio_codecs.is_empty());

        // Check default video codecs - client defaults to decode capability
        assert!(caps
            .video_codecs
            .iter()
            .any(|(c, _)| *c == VideoFourCc::Avc));
        assert!(caps
            .video_codecs
            .iter()
            .any(|(c, _)| *c == VideoFourCc::Hevc));
    }

    #[test]
    fn test_enhanced_client_capabilities_minimal() {
        let caps = EnhancedClientCapabilities::minimal();

        assert!(caps.video_codecs.is_empty());
        assert!(caps.audio_codecs.is_empty());
    }

    #[test]
    fn test_enhanced_client_capabilities_builder() {
        let caps = EnhancedClientCapabilities::minimal()
            .with_video_codec(VideoFourCc::Av1, FourCcCapability::decode())
            .with_audio_codec(AudioFourCc::Opus, FourCcCapability::decode())
            .with_reconnect()
            .with_multitrack();

        assert!(caps.reconnect);
        assert!(caps.multitrack);
        assert_eq!(caps.video_codecs.len(), 1);
        assert_eq!(caps.audio_codecs.len(), 1);
    }

    #[test]
    fn test_enhanced_client_capabilities_to_caps_ex() {
        let caps = EnhancedClientCapabilities::default();
        let caps_ex = caps.to_caps_ex();

        assert!(!caps_ex.supports_reconnect());
        assert!(!caps_ex.supports_multitrack());
        assert!(caps_ex.supports_modex());

        let caps_full = EnhancedClientCapabilities::default()
            .with_reconnect()
            .with_multitrack();
        let caps_ex = caps_full.to_caps_ex();

        assert!(caps_ex.supports_reconnect());
        assert!(caps_ex.supports_multitrack());
    }

    #[test]
    fn test_config_with_enhanced_capabilities() {
        let caps = EnhancedClientCapabilities::minimal()
            .with_video_codec(VideoFourCc::Hevc, FourCcCapability::decode());

        let config = ClientConfig::new("rtmp://localhost/live/test")
            .enhanced_rtmp(EnhancedRtmpMode::Auto)
            .enhanced_capabilities(caps);

        assert_eq!(config.enhanced_rtmp, EnhancedRtmpMode::Auto);
        assert_eq!(config.enhanced_capabilities.video_codecs.len(), 1);
    }
}
