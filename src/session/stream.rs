//! Per-stream state management
//!
//! Each RTMP message stream (identified by stream ID) has its own state,
//! including publish/play mode, stream key, and media state.

use std::time::Instant;

use crate::media::gop::GopBuffer;

/// Stream mode (publishing or playing)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamMode {
    /// Stream is idle (created but not publishing/playing)
    Idle,
    /// Stream is publishing (receiving media)
    Publishing,
    /// Stream is playing (sending media)
    Playing,
}

/// Per-stream state
#[derive(Debug)]
pub struct StreamState {
    /// Message stream ID
    pub id: u32,

    /// Current mode
    pub mode: StreamMode,

    /// Stream key/name (for publish/play)
    pub stream_key: Option<String>,

    /// Publish type ("live", "record", "append")
    pub publish_type: Option<String>,

    /// Time when stream became active
    pub started_at: Option<Instant>,

    /// Whether we've received video sequence header
    pub has_video_header: bool,

    /// Whether we've received audio sequence header
    pub has_audio_header: bool,

    /// Whether we've received metadata
    pub has_metadata: bool,

    /// Last video timestamp
    pub last_video_ts: u32,

    /// Last audio timestamp
    pub last_audio_ts: u32,

    /// Video frames received
    pub video_frames: u64,

    /// Audio frames received
    pub audio_frames: u64,

    /// Keyframes received
    pub keyframes: u64,

    /// Total bytes received on this stream
    pub bytes_received: u64,

    /// GOP buffer for late-joiner support
    pub gop_buffer: GopBuffer,

    /// NALU length prefix size from the last AVC sequence header (4 until one is seen)
    pub video_nalu_length_size: u8,
}

impl StreamState {
    /// Create a new stream state
    pub fn new(id: u32) -> Self {
        Self {
            id,
            mode: StreamMode::Idle,
            stream_key: None,
            publish_type: None,
            started_at: None,
            has_video_header: false,
            has_audio_header: false,
            has_metadata: false,
            last_video_ts: 0,
            last_audio_ts: 0,
            video_frames: 0,
            audio_frames: 0,
            keyframes: 0,
            bytes_received: 0,
            gop_buffer: GopBuffer::new(),
            video_nalu_length_size: 4,
        }
    }

    /// Start publishing on this stream
    pub fn start_publish(&mut self, stream_key: String, publish_type: String) {
        self.mode = StreamMode::Publishing;
        self.stream_key = Some(stream_key);
        self.publish_type = Some(publish_type);
        self.started_at = Some(Instant::now());
    }

    /// Start playing on this stream
    pub fn start_play(&mut self, stream_name: String) {
        self.mode = StreamMode::Playing;
        self.stream_key = Some(stream_name);
        self.started_at = Some(Instant::now());
    }

    /// Stop the stream
    pub fn stop(&mut self) {
        self.mode = StreamMode::Idle;
    }

    /// Check if stream is publishing
    pub fn is_publishing(&self) -> bool {
        self.mode == StreamMode::Publishing
    }

    /// Check if stream is playing
    pub fn is_playing(&self) -> bool {
        self.mode == StreamMode::Playing
    }

    /// Check if stream is ready (has required headers)
    pub fn is_ready(&self) -> bool {
        // A stream is ready if it has at least video or audio header
        self.has_video_header || self.has_audio_header
    }

    /// Get stream duration
    pub fn duration(&self) -> Option<std::time::Duration> {
        self.started_at.map(|t| t.elapsed())
    }

    /// Update video state
    pub fn on_video(&mut self, timestamp: u32, is_keyframe: bool, is_header: bool, size: usize) {
        self.last_video_ts = timestamp;
        self.video_frames += 1;
        self.bytes_received += size as u64;

        if is_header {
            self.has_video_header = true;
        }
        if is_keyframe {
            self.keyframes += 1;
        }
    }

    /// Update audio state
    pub fn on_audio(&mut self, timestamp: u32, is_header: bool, size: usize) {
        self.last_audio_ts = timestamp;
        self.audio_frames += 1;
        self.bytes_received += size as u64;

        if is_header {
            self.has_audio_header = true;
        }
    }

    /// Mark metadata received
    pub fn on_metadata(&mut self) {
        self.has_metadata = true;
    }

    /// Get bitrate estimate (bits per second)
    pub fn bitrate(&self) -> Option<u64> {
        let duration = self.duration()?.as_secs();
        if duration > 0 {
            Some((self.bytes_received * 8) / duration)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_publish() {
        let mut stream = StreamState::new(1);

        assert_eq!(stream.mode, StreamMode::Idle);
        assert!(!stream.is_publishing());

        stream.start_publish("test_key".into(), "live".into());

        assert_eq!(stream.mode, StreamMode::Publishing);
        assert!(stream.is_publishing());
        assert_eq!(stream.stream_key, Some("test_key".into()));
        assert!(stream.started_at.is_some());
    }

    #[test]
    fn test_stream_ready() {
        let mut stream = StreamState::new(1);

        assert!(!stream.is_ready());

        stream.on_video(0, false, true, 100);
        assert!(stream.is_ready());
        assert!(stream.has_video_header);
    }
}
