//! Registry configuration
//!
//! Configuration for the stream registry including broadcast capacity,
//! grace periods, and buffer sizes.

use std::time::Duration;

/// Configuration for the stream registry
#[derive(Debug, Clone)]
pub struct RegistryConfig {
    /// Capacity of the broadcast channel per stream
    ///
    /// At 30fps, 128 frames is about 4 seconds of video buffer.
    /// If subscribers fall behind more than this, they'll receive a Lagged error.
    pub broadcast_capacity: usize,

    /// Grace period to keep stream alive after publisher disconnects
    ///
    /// If the publisher reconnects within this period, subscribers continue
    /// without interruption. This handles brief network hiccups.
    pub publisher_grace_period: Duration,

    /// Timeout for idle streams with no publisher and no subscribers
    pub idle_stream_timeout: Duration,

    /// Maximum GOP buffer size in bytes per stream
    pub max_gop_size: usize,

    /// Interval for running cleanup tasks
    pub cleanup_interval: Duration,

    /// Number of lag events before disconnecting a slow subscriber
    pub max_consecutive_lag_events: u32,

    /// Lag threshold (frames) below which we continue normally
    pub lag_threshold_low: u64,
}

impl Default for RegistryConfig {
    fn default() -> Self {
        Self {
            broadcast_capacity: 128,         // ~4 seconds @ 30fps
            publisher_grace_period: Duration::from_secs(10),
            idle_stream_timeout: Duration::from_secs(30),
            max_gop_size: 4 * 1024 * 1024,   // 4MB
            cleanup_interval: Duration::from_secs(5),
            max_consecutive_lag_events: 10,
            lag_threshold_low: 30,           // ~1 second @ 30fps
        }
    }
}

impl RegistryConfig {
    /// Create a new registry config with default values
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the broadcast channel capacity
    pub fn broadcast_capacity(mut self, capacity: usize) -> Self {
        self.broadcast_capacity = capacity;
        self
    }

    /// Set the publisher grace period
    pub fn publisher_grace_period(mut self, duration: Duration) -> Self {
        self.publisher_grace_period = duration;
        self
    }

    /// Set the idle stream timeout
    pub fn idle_stream_timeout(mut self, duration: Duration) -> Self {
        self.idle_stream_timeout = duration;
        self
    }

    /// Set the maximum GOP buffer size
    pub fn max_gop_size(mut self, size: usize) -> Self {
        self.max_gop_size = size;
        self
    }
}
