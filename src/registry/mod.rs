//! Stream registry for pub/sub routing
//!
//! The registry manages active streams and routes media from publishers to subscribers.
//! It uses `tokio::sync::broadcast` for efficient zero-copy fan-out to multiple subscribers.
//!
//! # Architecture
//!
//! ```text
//!                          Arc<StreamRegistry>
//!                     ┌─────────────────────────┐
//!                     │ streams: HashMap<Key,   │
//!                     │   StreamEntry {         │
//!                     │     gop_buffer,         │
//!                     │     tx: broadcast::Tx,  │
//!                     │   }                     │
//!                     │ >                       │
//!                     └───────────┬─────────────┘
//!                                 │
//!         ┌───────────────────────┼───────────────────────┐
//!         │                       │                       │
//!         ▼                       ▼                       ▼
//!    [Publisher]            [Subscriber]            [Subscriber]
//!    handle_video()         frame_rx.recv()         frame_rx.recv()
//!         │                       │                       │
//!         └──► registry.broadcast()──► send_video() ──► TCP
//! ```
//!
//! # Zero-Copy Design
//!
//! `bytes::Bytes` uses reference counting, so all subscribers share the same
//! memory allocation. The broadcast channel clones the `BroadcastFrame`, but
//! the inner `Bytes` data is only reference-counted, not copied.

pub mod config;

pub use config::RegistryConfig;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Instant;

use bytes::Bytes;
use tokio::sync::{broadcast, RwLock};

use crate::media::flv::FlvTag;
use crate::media::gop::GopBuffer;

/// Unique identifier for a stream (app + stream name)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StreamKey {
    /// Application name (e.g., "live")
    pub app: String,
    /// Stream name/key (e.g., "stream_key_123")
    pub name: String,
}

impl StreamKey {
    /// Create a new stream key
    pub fn new(app: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            app: app.into(),
            name: name.into(),
        }
    }
}

impl std::fmt::Display for StreamKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.app, self.name)
    }
}

/// Type of broadcast frame
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameType {
    /// Video frame
    Video,
    /// Audio frame
    Audio,
    /// Metadata (onMetaData)
    Metadata,
}

/// A frame to be broadcast to subscribers
///
/// This is designed to be cheap to clone due to `Bytes` reference counting.
#[derive(Debug, Clone)]
pub struct BroadcastFrame {
    /// Type of frame
    pub frame_type: FrameType,
    /// Timestamp in milliseconds
    pub timestamp: u32,
    /// Frame data (zero-copy via reference counting)
    pub data: Bytes,
    /// Whether this is a keyframe (video only)
    pub is_keyframe: bool,
    /// Whether this is a sequence header
    pub is_header: bool,
}

impl BroadcastFrame {
    /// Create a video frame
    pub fn video(timestamp: u32, data: Bytes, is_keyframe: bool, is_header: bool) -> Self {
        Self {
            frame_type: FrameType::Video,
            timestamp,
            data,
            is_keyframe,
            is_header,
        }
    }

    /// Create an audio frame
    pub fn audio(timestamp: u32, data: Bytes, is_header: bool) -> Self {
        Self {
            frame_type: FrameType::Audio,
            timestamp,
            data,
            is_keyframe: false,
            is_header,
        }
    }

    /// Create a metadata frame
    pub fn metadata(data: Bytes) -> Self {
        Self {
            frame_type: FrameType::Metadata,
            timestamp: 0,
            data,
            is_keyframe: false,
            is_header: false,
        }
    }

    /// Convert from FLV tag
    pub fn from_flv_tag(tag: &FlvTag) -> Self {
        match tag.tag_type {
            crate::media::flv::FlvTagType::Video => {
                let is_keyframe = tag.is_keyframe();
                let is_header = tag.is_avc_sequence_header();
                Self::video(tag.timestamp, tag.data.clone(), is_keyframe, is_header)
            }
            crate::media::flv::FlvTagType::Audio => {
                let is_header = tag.is_aac_sequence_header();
                Self::audio(tag.timestamp, tag.data.clone(), is_header)
            }
            crate::media::flv::FlvTagType::Script => {
                Self::metadata(tag.data.clone())
            }
        }
    }
}

/// State of a stream entry
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamState {
    /// Stream has an active publisher
    Active,
    /// Publisher disconnected, within grace period
    GracePeriod,
    /// No publisher, waiting for cleanup
    Idle,
}

/// Entry for a single stream in the registry
pub struct StreamEntry {
    /// GOP buffer for late-joiner support
    pub gop_buffer: GopBuffer,

    /// Cached video sequence header for fast subscriber catchup
    pub video_header: Option<BroadcastFrame>,

    /// Cached audio sequence header for fast subscriber catchup
    pub audio_header: Option<BroadcastFrame>,

    /// Cached metadata
    pub metadata: Option<BroadcastFrame>,

    /// Current publisher's session ID (None if no publisher)
    pub publisher_id: Option<u64>,

    /// Broadcast sender for fan-out to subscribers
    tx: broadcast::Sender<BroadcastFrame>,

    /// Number of active subscribers
    pub subscriber_count: AtomicU32,

    /// When the publisher disconnected (for grace period tracking)
    pub publisher_disconnected_at: Option<Instant>,

    /// When the stream was created
    pub created_at: Instant,

    /// Current stream state
    pub state: StreamState,
}

impl StreamEntry {
    /// Create a new stream entry
    fn new(config: &RegistryConfig) -> Self {
        let (tx, _) = broadcast::channel(config.broadcast_capacity);

        Self {
            gop_buffer: GopBuffer::with_max_size(config.max_gop_size),
            video_header: None,
            audio_header: None,
            metadata: None,
            publisher_id: None,
            tx,
            subscriber_count: AtomicU32::new(0),
            publisher_disconnected_at: None,
            created_at: Instant::now(),
            state: StreamState::Idle,
        }
    }

    /// Get the number of subscribers
    pub fn subscriber_count(&self) -> u32 {
        self.subscriber_count.load(Ordering::Relaxed)
    }

    /// Check if the stream has an active publisher
    pub fn has_publisher(&self) -> bool {
        self.publisher_id.is_some()
    }

    /// Get catchup frames for a new subscriber
    ///
    /// Returns sequence headers followed by GOP buffer contents.
    pub fn get_catchup_frames(&self) -> Vec<BroadcastFrame> {
        let mut frames = Vec::new();

        // Add metadata first
        if let Some(ref meta) = self.metadata {
            frames.push(meta.clone());
        }

        // Add sequence headers
        if let Some(ref video) = self.video_header {
            frames.push(video.clone());
        }
        if let Some(ref audio) = self.audio_header {
            frames.push(audio.clone());
        }

        // Add GOP buffer contents
        for tag in self.gop_buffer.get_catchup_data() {
            frames.push(BroadcastFrame::from_flv_tag(&tag));
        }

        frames
    }
}

/// Error type for registry operations
#[derive(Debug, Clone)]
pub enum RegistryError {
    /// Stream not found
    StreamNotFound(StreamKey),
    /// Stream already has a publisher
    StreamAlreadyPublishing(StreamKey),
    /// Publisher ID mismatch
    PublisherMismatch,
    /// Stream is not active (e.g., in grace period without publisher)
    StreamNotActive(StreamKey),
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegistryError::StreamNotFound(key) => write!(f, "Stream not found: {}", key),
            RegistryError::StreamAlreadyPublishing(key) => {
                write!(f, "Stream already has a publisher: {}", key)
            }
            RegistryError::PublisherMismatch => write!(f, "Publisher ID mismatch"),
            RegistryError::StreamNotActive(key) => write!(f, "Stream not active: {}", key),
        }
    }
}

impl std::error::Error for RegistryError {}

/// Central registry for all active streams
///
/// Thread-safe via `RwLock`. Read-heavy workloads (subscriber count checks,
/// broadcasting) benefit from the concurrent read access.
pub struct StreamRegistry {
    /// Map of stream key to stream entry
    streams: RwLock<HashMap<StreamKey, Arc<RwLock<StreamEntry>>>>,

    /// Configuration
    config: RegistryConfig,
}

impl StreamRegistry {
    /// Create a new stream registry with default configuration
    pub fn new() -> Self {
        Self::with_config(RegistryConfig::default())
    }

    /// Create a new stream registry with custom configuration
    pub fn with_config(config: RegistryConfig) -> Self {
        Self {
            streams: RwLock::new(HashMap::new()),
            config,
        }
    }

    /// Get the registry configuration
    pub fn config(&self) -> &RegistryConfig {
        &self.config
    }

    /// Register a publisher for a stream
    ///
    /// If the stream doesn't exist, it will be created.
    /// If the stream exists and is in grace period, the publisher reclaims it.
    /// Returns an error if the stream already has an active publisher.
    pub async fn register_publisher(
        &self,
        key: &StreamKey,
        session_id: u64,
    ) -> Result<(), RegistryError> {
        let mut streams = self.streams.write().await;

        if let Some(entry_arc) = streams.get(key) {
            let mut entry = entry_arc.write().await;

            // Check if stream is available for publishing
            match entry.state {
                StreamState::Active if entry.publisher_id.is_some() => {
                    return Err(RegistryError::StreamAlreadyPublishing(key.clone()));
                }
                StreamState::GracePeriod | StreamState::Idle | StreamState::Active => {
                    // Reclaim or take over the stream
                    entry.publisher_id = Some(session_id);
                    entry.publisher_disconnected_at = None;
                    entry.state = StreamState::Active;

                    tracing::info!(
                        stream = %key,
                        session_id = session_id,
                        subscribers = entry.subscriber_count(),
                        "Publisher registered (existing stream)"
                    );
                }
            }
        } else {
            // Create new stream entry
            let mut entry = StreamEntry::new(&self.config);
            entry.publisher_id = Some(session_id);
            entry.state = StreamState::Active;

            streams.insert(key.clone(), Arc::new(RwLock::new(entry)));

            tracing::info!(
                stream = %key,
                session_id = session_id,
                "Publisher registered (new stream)"
            );
        }

        Ok(())
    }

    /// Unregister a publisher from a stream
    ///
    /// The stream enters grace period if there are active subscribers,
    /// allowing the publisher to reconnect.
    pub async fn unregister_publisher(&self, key: &StreamKey, session_id: u64) {
        let streams = self.streams.read().await;

        if let Some(entry_arc) = streams.get(key) {
            let mut entry = entry_arc.write().await;

            // Verify this is the actual publisher
            if entry.publisher_id != Some(session_id) {
                tracing::warn!(
                    stream = %key,
                    expected = ?entry.publisher_id,
                    actual = session_id,
                    "Publisher unregister mismatch"
                );
                return;
            }

            entry.publisher_id = None;
            entry.publisher_disconnected_at = Some(Instant::now());

            // If there are subscribers, enter grace period; otherwise go idle
            if entry.subscriber_count() > 0 {
                entry.state = StreamState::GracePeriod;
                tracing::info!(
                    stream = %key,
                    session_id = session_id,
                    subscribers = entry.subscriber_count(),
                    grace_period_secs = self.config.publisher_grace_period.as_secs(),
                    "Publisher disconnected, entering grace period"
                );
            } else {
                entry.state = StreamState::Idle;
                tracing::info!(
                    stream = %key,
                    session_id = session_id,
                    "Publisher disconnected, no subscribers"
                );
            }
        }
    }

    /// Subscribe to a stream
    ///
    /// Returns a broadcast receiver and catchup frames for the subscriber.
    /// The catchup frames contain sequence headers and recent GOP data.
    pub async fn subscribe(
        &self,
        key: &StreamKey,
    ) -> Result<(broadcast::Receiver<BroadcastFrame>, Vec<BroadcastFrame>), RegistryError> {
        let streams = self.streams.read().await;

        let entry_arc = streams
            .get(key)
            .ok_or_else(|| RegistryError::StreamNotFound(key.clone()))?;

        let entry = entry_arc.read().await;

        // Allow subscription even during grace period (publisher might reconnect)
        if entry.state == StreamState::Idle && entry.publisher_id.is_none() {
            return Err(RegistryError::StreamNotActive(key.clone()));
        }

        // Get receiver and catchup frames
        let rx = entry.tx.subscribe();
        let catchup = entry.get_catchup_frames();

        // Increment subscriber count
        entry.subscriber_count.fetch_add(1, Ordering::Relaxed);

        tracing::info!(
            stream = %key,
            subscribers = entry.subscriber_count(),
            catchup_frames = catchup.len(),
            "Subscriber added"
        );

        Ok((rx, catchup))
    }

    /// Unsubscribe from a stream
    pub async fn unsubscribe(&self, key: &StreamKey) {
        let streams = self.streams.read().await;

        if let Some(entry_arc) = streams.get(key) {
            let entry = entry_arc.read().await;
            let prev = entry.subscriber_count.fetch_sub(1, Ordering::Relaxed);

            tracing::debug!(
                stream = %key,
                subscribers = prev.saturating_sub(1),
                "Subscriber removed"
            );
        }
    }

    /// Broadcast a frame to all subscribers of a stream
    ///
    /// Also updates the GOP buffer and sequence headers as needed.
    pub async fn broadcast(&self, key: &StreamKey, frame: BroadcastFrame) {
        let streams = self.streams.read().await;

        if let Some(entry_arc) = streams.get(key) {
            let mut entry = entry_arc.write().await;

            // Update cached headers
            match frame.frame_type {
                FrameType::Video if frame.is_header => {
                    entry.video_header = Some(frame.clone());
                }
                FrameType::Audio if frame.is_header => {
                    entry.audio_header = Some(frame.clone());
                }
                FrameType::Metadata => {
                    entry.metadata = Some(frame.clone());
                }
                _ => {}
            }

            // Update GOP buffer for video frames (non-headers)
            if frame.frame_type == FrameType::Video && !frame.is_header {
                let tag = FlvTag::video(frame.timestamp, frame.data.clone());
                entry.gop_buffer.push(tag);
            }

            // Broadcast to subscribers
            // Note: send() returns Ok(n) where n is number of receivers, or Err if no receivers
            let _ = entry.tx.send(frame);
        }
    }

    /// Check if a stream exists and has an active publisher
    pub async fn has_active_stream(&self, key: &StreamKey) -> bool {
        let streams = self.streams.read().await;

        if let Some(entry_arc) = streams.get(key) {
            let entry = entry_arc.read().await;
            entry.state == StreamState::Active && entry.publisher_id.is_some()
        } else {
            false
        }
    }

    /// Check if a stream exists (active or in grace period)
    pub async fn stream_exists(&self, key: &StreamKey) -> bool {
        let streams = self.streams.read().await;

        if let Some(entry_arc) = streams.get(key) {
            let entry = entry_arc.read().await;
            matches!(entry.state, StreamState::Active | StreamState::GracePeriod)
        } else {
            false
        }
    }

    /// Get stream statistics
    pub async fn get_stream_stats(&self, key: &StreamKey) -> Option<StreamStats> {
        let streams = self.streams.read().await;

        if let Some(entry_arc) = streams.get(key) {
            let entry = entry_arc.read().await;
            Some(StreamStats {
                subscriber_count: entry.subscriber_count(),
                has_publisher: entry.publisher_id.is_some(),
                state: entry.state,
                gop_frame_count: entry.gop_buffer.frame_count(),
                gop_size_bytes: entry.gop_buffer.size(),
            })
        } else {
            None
        }
    }

    /// Get total number of streams
    pub async fn stream_count(&self) -> usize {
        self.streams.read().await.len()
    }

    /// Run cleanup task once
    ///
    /// Removes streams that have:
    /// - Been in grace period longer than `publisher_grace_period`
    /// - Been idle longer than `idle_stream_timeout`
    pub async fn cleanup(&self) {
        let mut streams = self.streams.write().await;
        let now = Instant::now();

        let keys_to_remove: Vec<StreamKey> = streams
            .iter()
            .filter_map(|(key, entry_arc)| {
                // Try to get read lock without blocking
                if let Ok(entry) = entry_arc.try_read() {
                    let should_remove = match entry.state {
                        StreamState::GracePeriod => {
                            if let Some(disconnected_at) = entry.publisher_disconnected_at {
                                now.duration_since(disconnected_at) > self.config.publisher_grace_period
                            } else {
                                false
                            }
                        }
                        StreamState::Idle => {
                            if let Some(disconnected_at) = entry.publisher_disconnected_at {
                                now.duration_since(disconnected_at) > self.config.idle_stream_timeout
                            } else {
                                now.duration_since(entry.created_at) > self.config.idle_stream_timeout
                            }
                        }
                        StreamState::Active => false,
                    };

                    if should_remove {
                        Some(key.clone())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();

        for key in keys_to_remove {
            streams.remove(&key);
            tracing::info!(stream = %key, "Stream removed by cleanup");
        }
    }

    /// Spawn background cleanup task
    ///
    /// Returns a handle that can be used to abort the task.
    pub fn spawn_cleanup_task(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let registry = Arc::clone(self);
        let interval = registry.config.cleanup_interval;

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                ticker.tick().await;
                registry.cleanup().await;
            }
        })
    }
}

impl Default for StreamRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics for a stream
#[derive(Debug, Clone)]
pub struct StreamStats {
    /// Number of active subscribers
    pub subscriber_count: u32,
    /// Whether the stream has an active publisher
    pub has_publisher: bool,
    /// Current stream state
    pub state: StreamState,
    /// Number of frames in GOP buffer
    pub gop_frame_count: usize,
    /// Size of GOP buffer in bytes
    pub gop_size_bytes: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_register_publisher() {
        let registry = StreamRegistry::new();
        let key = StreamKey::new("live", "test_stream");

        // Register publisher
        registry.register_publisher(&key, 1).await.unwrap();
        assert!(registry.has_active_stream(&key).await);

        // Can't register another publisher
        let result = registry.register_publisher(&key, 2).await;
        assert!(matches!(result, Err(RegistryError::StreamAlreadyPublishing(_))));
    }

    #[tokio::test]
    async fn test_subscribe_unsubscribe() {
        let registry = StreamRegistry::new();
        let key = StreamKey::new("live", "test_stream");

        // Need a publisher first
        registry.register_publisher(&key, 1).await.unwrap();

        // Subscribe
        let (mut rx, catchup) = registry.subscribe(&key).await.unwrap();
        assert!(catchup.is_empty()); // No data yet

        // Broadcast a frame
        let frame = BroadcastFrame::video(0, Bytes::from_static(&[0x17, 0x01]), true, false);
        registry.broadcast(&key, frame.clone()).await;

        // Receive the frame
        let received = rx.recv().await.unwrap();
        assert_eq!(received.timestamp, 0);
        assert!(received.is_keyframe);

        // Unsubscribe
        registry.unsubscribe(&key).await;

        let stats = registry.get_stream_stats(&key).await.unwrap();
        assert_eq!(stats.subscriber_count, 0);
    }

    #[tokio::test]
    async fn test_grace_period() {
        let config = RegistryConfig::default()
            .publisher_grace_period(std::time::Duration::from_millis(100));
        let registry = StreamRegistry::with_config(config);
        let key = StreamKey::new("live", "test_stream");

        // Register publisher and subscriber
        registry.register_publisher(&key, 1).await.unwrap();
        let (_rx, _) = registry.subscribe(&key).await.unwrap();

        // Publisher disconnects
        registry.unregister_publisher(&key, 1).await;

        // Stream should be in grace period
        let stats = registry.get_stream_stats(&key).await.unwrap();
        assert_eq!(stats.state, StreamState::GracePeriod);

        // Stream still exists
        assert!(registry.stream_exists(&key).await);

        // New subscriber can still join
        let result = registry.subscribe(&key).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_publisher_reconnect() {
        let registry = StreamRegistry::new();
        let key = StreamKey::new("live", "test_stream");

        // Register publisher
        registry.register_publisher(&key, 1).await.unwrap();

        // Add subscriber
        let (_rx, _) = registry.subscribe(&key).await.unwrap();

        // Publisher disconnects
        registry.unregister_publisher(&key, 1).await;

        // New publisher takes over
        registry.register_publisher(&key, 2).await.unwrap();

        let stats = registry.get_stream_stats(&key).await.unwrap();
        assert!(stats.has_publisher);
        assert_eq!(stats.state, StreamState::Active);
        assert_eq!(stats.subscriber_count, 1); // Subscriber still there
    }

    #[tokio::test]
    async fn test_catchup_frames() {
        let registry = StreamRegistry::new();
        let key = StreamKey::new("live", "test_stream");

        registry.register_publisher(&key, 1).await.unwrap();

        // Broadcast sequence headers
        let video_header = BroadcastFrame::video(0, Bytes::from_static(&[0x17, 0x00]), true, true);
        let audio_header = BroadcastFrame::audio(0, Bytes::from_static(&[0xAF, 0x00]), true);
        registry.broadcast(&key, video_header).await;
        registry.broadcast(&key, audio_header).await;

        // Broadcast a keyframe
        let keyframe = BroadcastFrame::video(33, Bytes::from_static(&[0x17, 0x01]), true, false);
        registry.broadcast(&key, keyframe).await;

        // Late joiner subscribes
        let (_rx, catchup) = registry.subscribe(&key).await.unwrap();

        // Should receive headers + keyframe
        assert_eq!(catchup.len(), 3);
        assert!(catchup[0].is_header); // video header
        assert!(catchup[1].is_header); // audio header
        assert!(catchup[2].is_keyframe); // keyframe
    }
}
