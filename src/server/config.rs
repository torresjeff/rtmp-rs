//! Server configuration

use std::net::SocketAddr;
use std::time::Duration;

use crate::protocol::constants::*;

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
}
