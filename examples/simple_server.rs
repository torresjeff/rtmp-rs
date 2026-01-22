//! Simple RTMP server example with pub/sub support
//!
//! Run with: cargo run --example simple_server
//!
//! ## Publishing (send stream)
//!
//! With OBS:
//!   Server: rtmp://localhost/live
//!   Stream Key: test_key
//!
//! With ffmpeg:
//!   ffmpeg -re -i input.mp4 -c copy -f flv rtmp://localhost/live/test_key
//!
//! ## Playing (receive stream)
//!
//! With VLC:
//!   vlc rtmp://localhost/live/test_key
//!
//! With ffplay:
//!   ffplay rtmp://localhost/live/test_key
//!
//! ## Features
//!
//! - Late-joiner support: Players joining after stream starts receive sequence headers + GOP
//! - Publisher reconnect: If publisher disconnects, stream stays alive for 10s grace period
//! - Backpressure: Slow subscribers skip to next keyframe instead of buffering indefinitely

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use rtmp_rs::amf::AmfValue;
use rtmp_rs::media::{AacData, FlvTag, H264Data};
use rtmp_rs::protocol::message::{ConnectParams, PublishParams};
use rtmp_rs::server::handler::{AuthResult, MediaDeliveryMode, RtmpHandler};
use rtmp_rs::session::{SessionContext, StreamContext};
use rtmp_rs::{RtmpServer, ServerConfig};

/// Simple handler that logs events and collects stats
struct MyHandler {
    video_frames: AtomicU64,
    audio_frames: AtomicU64,
    keyframes: AtomicU64,
    bytes_received: AtomicU64,
}

impl MyHandler {
    fn new() -> Self {
        Self {
            video_frames: AtomicU64::new(0),
            audio_frames: AtomicU64::new(0),
            keyframes: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
        }
    }

    fn print_stats(&self) {
        println!(
            "Stats: video={} audio={} keyframes={} bytes={}",
            self.video_frames.load(Ordering::Relaxed),
            self.audio_frames.load(Ordering::Relaxed),
            self.keyframes.load(Ordering::Relaxed),
            self.bytes_received.load(Ordering::Relaxed),
        );
    }
}

#[async_trait]
impl RtmpHandler for MyHandler {
    async fn on_connection(&self, ctx: &SessionContext) -> bool {
        println!("[{}] New connection from {}", ctx.session_id, ctx.peer_addr);
        true
    }

    async fn on_connect(&self, ctx: &SessionContext, params: &ConnectParams) -> AuthResult {
        println!(
            "[{}] Connect: app={}, tcUrl={:?}",
            ctx.session_id, params.app, params.tc_url
        );

        // Accept any app name
        AuthResult::Accept
    }

    async fn on_fc_publish(&self, ctx: &SessionContext, stream_key: &str) -> AuthResult {
        println!("[{}] FCPublish: {}", ctx.session_id, stream_key);
        AuthResult::Accept
    }

    async fn on_publish(&self, ctx: &SessionContext, params: &PublishParams) -> AuthResult {
        println!(
            "[{}] Publish: key={}, type={}",
            ctx.session_id, params.stream_key, params.publish_type
        );

        // Example: validate stream key
        // if !params.stream_key.starts_with("valid_") {
        //     return AuthResult::Reject("Invalid stream key".into());
        // }

        AuthResult::Accept
    }

    async fn on_metadata(&self, ctx: &StreamContext, metadata: &HashMap<String, AmfValue>) {
        println!("[{}] Metadata received:", ctx.session.session_id);

        if let Some(width) = metadata.get("width").and_then(|v| v.as_number()) {
            if let Some(height) = metadata.get("height").and_then(|v| v.as_number()) {
                println!("  Resolution: {}x{}", width as u32, height as u32);
            }
        }

        if let Some(fps) = metadata.get("framerate").and_then(|v| v.as_number()) {
            println!("  Framerate: {:.2} fps", fps);
        }

        if let Some(bitrate) = metadata.get("videodatarate").and_then(|v| v.as_number()) {
            println!("  Video bitrate: {:.0} kbps", bitrate);
        }

        if let Some(codec) = metadata.get("videocodecid").and_then(|v| v.as_number()) {
            println!("  Video codec ID: {}", codec as u32);
        }

        if let Some(codec) = metadata.get("audiocodecid").and_then(|v| v.as_number()) {
            println!("  Audio codec ID: {}", codec as u32);
        }
    }

    async fn on_media_tag(&self, ctx: &StreamContext, tag: &FlvTag) -> bool {
        self.bytes_received
            .fetch_add(tag.size() as u64, Ordering::Relaxed);
        tracing::trace!(
            stream_id = ctx.stream_id,
            is_publishing = ctx.is_publishing,
            tag_type = ?tag.tag_type,
            timestamp = tag.timestamp,
            "Server received flv tag",
        );
        true
    }

    async fn on_video_frame(&self, ctx: &StreamContext, frame: &H264Data, timestamp: u32) {
        self.video_frames.fetch_add(1, Ordering::Relaxed);

        match frame {
            H264Data::SequenceHeader(config) => {
                tracing::debug!(
                    profile_name = config.profile_name(),
                    level = config.level_string(),
                    sps = config.sps.len(),
                    pps = config.pps.len(),
                    stream_id = ctx.stream_id,
                    is_publishing = ctx.is_publishing,
                    "  Video sequence header",
                );
            }
            H264Data::Frame { keyframe, .. } if *keyframe => {
                // Only log keyframes to avoid spam
                tracing::trace!(timestamp = timestamp, "Server received video key frame");
            }
            _ => {}
        }
    }

    async fn on_audio_frame(&self, ctx: &StreamContext, frame: &AacData, _timestamp: u32) {
        self.audio_frames.fetch_add(1, Ordering::Relaxed);

        if let AacData::SequenceHeader(config) = frame {
            tracing::debug!(
                profile = ?config.profile(),
                sampling_frequency = config.sampling_frequency,
                channels = config.channels(),
                stream_id = ctx.stream_id,
                is_publishing = ctx.is_publishing,
                "  Audio sequence header"
            );
        }
    }

    async fn on_keyframe(&self, ctx: &StreamContext, _timestamp: u32) {
        self.keyframes.fetch_add(1, Ordering::Relaxed);

        // Print stats every keyframe (usually every 2 seconds)
        let total_keyframes = self.keyframes.load(Ordering::Relaxed);
        if total_keyframes % 5 == 0 {
            tracing::debug!(
                "[{}] Stream '{}' progress: {} keyframes, {} video, {} audio frames",
                ctx.session.session_id,
                ctx.stream_key,
                total_keyframes,
                self.video_frames.load(Ordering::Relaxed),
                self.audio_frames.load(Ordering::Relaxed)
            );
        }
    }

    async fn on_publish_stop(&self, ctx: &StreamContext) {
        println!(
            "[{}] Publish stopped: {}",
            ctx.session.session_id, ctx.stream_key
        );
        self.print_stats();
    }

    async fn on_disconnect(&self, ctx: &SessionContext) {
        println!("[{}] Disconnected", ctx.session_id);
    }

    fn media_delivery_mode(&self) -> MediaDeliveryMode {
        MediaDeliveryMode::ParsedFrames
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("rtmp_rs=debug".parse()?)
                .add_directive("simple_server=debug".parse()?),
        )
        .init();

    // Create server config
    let config = ServerConfig::default();

    println!("Starting RTMP server on {}", config.bind_addr);
    println!();
    println!("=== Publish a stream ===");
    println!("OBS:    Server: rtmp://localhost/live  Stream Key: test");
    println!("ffmpeg: ffmpeg -re -i input.mp4 -c copy -f flv rtmp://localhost/live/test");
    println!();
    println!("=== Play a stream ===");
    println!("VLC:    vlc rtmp://localhost/live/test");
    println!("ffplay: ffplay rtmp://localhost/live/test");
    println!();

    // Create and run server
    let handler = MyHandler::new();
    let server = RtmpServer::new(config, handler);

    // Run with Ctrl+C handling
    let server = Arc::new(server);
    let _server_clone = Arc::clone(&server);

    tokio::select! {
        result = server.run() => {
            if let Err(e) = result {
                eprintln!("Server error: {}", e);
            }
        }
        _ = tokio::signal::ctrl_c() => {
            println!("\nShutting down...");
        }
    }

    Ok(())
}
