//! Statistics and metrics for RTMP sessions

use std::time::{Duration, Instant};

/// Session-level statistics
#[derive(Debug, Clone, Default)]
pub struct SessionStats {
    /// Total bytes received
    pub bytes_received: u64,
    /// Total bytes sent
    pub bytes_sent: u64,
    /// Connection duration
    pub duration: Duration,
    /// Number of video frames received
    pub video_frames: u64,
    /// Number of audio frames received
    pub audio_frames: u64,
    /// Number of keyframes received
    pub keyframes: u64,
    /// Dropped frames count
    pub dropped_frames: u64,
    /// Current bitrate estimate (bits/sec)
    pub bitrate: u64,
}

impl SessionStats {
    /// Create new stats tracker
    pub fn new() -> Self {
        Self::default()
    }

    /// Calculate bitrate from bytes and duration
    pub fn calculate_bitrate(&mut self) {
        let secs = self.duration.as_secs();
        if secs > 0 {
            self.bitrate = (self.bytes_received * 8) / secs;
        }
    }
}

/// Stream-level statistics
#[derive(Debug, Clone)]
pub struct StreamStats {
    /// Stream key
    pub stream_key: String,
    /// Start time
    pub started_at: Instant,
    /// Total bytes received
    pub bytes_received: u64,
    /// Video frames received
    pub video_frames: u64,
    /// Audio frames received
    pub audio_frames: u64,
    /// Keyframes received
    pub keyframes: u64,
    /// Last video timestamp
    pub last_video_ts: u32,
    /// Last audio timestamp
    pub last_audio_ts: u32,
    /// Video codec info
    pub video_codec: Option<String>,
    /// Audio codec info
    pub audio_codec: Option<String>,
    /// Video width
    pub width: Option<u32>,
    /// Video height
    pub height: Option<u32>,
    /// Video framerate
    pub framerate: Option<f64>,
    /// Audio sample rate
    pub audio_sample_rate: Option<u32>,
    /// Audio channels
    pub audio_channels: Option<u8>,
}

impl StreamStats {
    pub fn new(stream_key: String) -> Self {
        Self {
            stream_key,
            started_at: Instant::now(),
            bytes_received: 0,
            video_frames: 0,
            audio_frames: 0,
            keyframes: 0,
            last_video_ts: 0,
            last_audio_ts: 0,
            video_codec: None,
            audio_codec: None,
            width: None,
            height: None,
            framerate: None,
            audio_sample_rate: None,
            audio_channels: None,
        }
    }

    /// Get duration since stream started
    pub fn duration(&self) -> Duration {
        self.started_at.elapsed()
    }

    /// Calculate bitrate in bits per second
    pub fn bitrate(&self) -> u64 {
        let secs = self.duration().as_secs();
        if secs > 0 {
            (self.bytes_received * 8) / secs
        } else {
            0
        }
    }

    /// Calculate video framerate
    pub fn calculated_framerate(&self) -> f64 {
        let secs = self.duration().as_secs_f64();
        if secs > 0.0 {
            self.video_frames as f64 / secs
        } else {
            0.0
        }
    }
}

/// Server-wide statistics
#[derive(Debug, Clone, Default)]
pub struct ServerStats {
    /// Total connections ever
    pub total_connections: u64,
    /// Current active connections
    pub active_connections: u64,
    /// Total bytes received
    pub total_bytes_received: u64,
    /// Total bytes sent
    pub total_bytes_sent: u64,
    /// Active streams
    pub active_streams: u64,
    /// Uptime
    pub uptime: Duration,
}

impl ServerStats {
    pub fn new() -> Self {
        Self::default()
    }
}
