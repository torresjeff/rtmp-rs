//! Client configuration

use std::time::Duration;

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
}
