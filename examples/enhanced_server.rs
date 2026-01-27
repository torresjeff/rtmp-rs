//! Enhanced RTMP (E-RTMP) server example
//!
//! Demonstrates E-RTMP capability negotiation with modern codec support.
//! You can read more about E-RTMP here: https://github.com/veovera/enhanced-rtmp
//!
//! Run with: cargo run --example enhanced_server
//!
//! ## E-RTMP Features
//!
//! - Extended codec support: HEVC, AV1, VP9, Opus, FLAC
//! - Capability negotiation: Server and client agree on supported codecs
//! - Backwards compatible: Falls back to legacy RTMP for older clients
//!
//! ## Testing with E-RTMP clients
//!
//! Currently, OBS 30+ and some other modern clients support E-RTMP.
//! Legacy clients (older ffmpeg, VLC) will use standard RTMP.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};

use rtmp_rs::amf::AmfValue;
use rtmp_rs::media::{AacData, EnhancedAudioData, EnhancedVideoData, FlvTag, H264Data};
use rtmp_rs::protocol::enhanced::CapsEx;
use rtmp_rs::protocol::message::{ConnectParams, PublishParams};
use rtmp_rs::server::handler::{AuthResult, MediaDeliveryMode, RtmpHandler};
use rtmp_rs::session::{SessionContext, StreamContext};
use rtmp_rs::{RtmpServer, ServerConfig};

/// Handler that demonstrates E-RTMP capability awareness
struct EnhancedHandler {
    video_frames: AtomicU64,
    audio_frames: AtomicU64,
    keyframes: AtomicU64,
}

impl EnhancedHandler {
    fn new() -> Self {
        Self {
            video_frames: AtomicU64::new(0),
            audio_frames: AtomicU64::new(0),
            keyframes: AtomicU64::new(0),
        }
    }
}

impl RtmpHandler for EnhancedHandler {
    async fn on_connection(&self, ctx: &SessionContext) -> bool {
        println!("[{}] New connection from {}", ctx.session_id, ctx.peer_addr);
        true
    }

    async fn on_connect(&self, ctx: &SessionContext, params: &ConnectParams) -> AuthResult {
        println!(
            "[{}] Connect: app={}, tcUrl={:?}",
            ctx.session_id, params.app, params.tc_url
        );

        // Check if client supports E-RTMP
        if params.has_enhanced_rtmp() {
            println!("[{}] Client supports Enhanced RTMP!", ctx.session_id);

            // Log client's codec capabilities
            let client_caps = params.to_enhanced_capabilities();

            if !client_caps.video_codecs.is_empty() {
                let codecs: Vec<_> = client_caps
                    .video_codecs
                    .keys()
                    .map(|c| c.as_fourcc_str())
                    .collect();
                println!("[{}]   Video codecs: {:?}", ctx.session_id, codecs);
            }

            if !client_caps.audio_codecs.is_empty() {
                let codecs: Vec<_> = client_caps
                    .audio_codecs
                    .keys()
                    .map(|c| c.as_fourcc_str())
                    .collect();
                println!("[{}]   Audio codecs: {:?}", ctx.session_id, codecs);
            }

            println!(
                "[{}]   CapsEx flags: 0x{:02X}",
                ctx.session_id,
                client_caps.caps_ex.bits()
            );
        } else {
            println!("[{}] Client using legacy RTMP", ctx.session_id);
        }

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

        // Check if E-RTMP was successfully negotiated for this session
        if ctx.is_enhanced_rtmp() {
            println!("[{}] Publishing with E-RTMP enabled", ctx.session_id);

            if let Some(caps) = &ctx.enhanced_capabilities {
                // Log negotiated capabilities
                if caps.caps_ex.contains(CapsEx::MULTITRACK) {
                    println!("[{}]   Multitrack: enabled", ctx.session_id);
                }
                if caps.caps_ex.contains(CapsEx::RECONNECT) {
                    println!("[{}]   Reconnect: enabled", ctx.session_id);
                }
            }
        }

        AuthResult::Accept
    }

    async fn on_metadata(&self, ctx: &StreamContext, metadata: &HashMap<String, AmfValue>) {
        println!("[{}] Metadata received:", ctx.session.session_id);

        if let Some(width) = metadata.get("width").and_then(|v| v.as_number()) {
            if let Some(height) = metadata.get("height").and_then(|v| v.as_number()) {
                println!("  Resolution: {}x{}", width as u32, height as u32);
            }
        }

        // Check for E-RTMP specific metadata
        if let Some(codec) = metadata.get("videocodec").and_then(|v| v.as_str()) {
            println!("  Video codec (E-RTMP): {}", codec);
        }
        if let Some(codec) = metadata.get("audiocodec").and_then(|v| v.as_str()) {
            println!("  Audio codec (E-RTMP): {}", codec);
        }
    }

    async fn on_media_tag(&self, _ctx: &StreamContext, _tag: &FlvTag) -> bool {
        true
    }

    // ==========================================================================
    // Legacy callbacks (FLV format)
    //
    // Called when the client sends video/audio using the legacy FLV tag format:
    // - Video: CodecID in lower 4 bits (7 = H.264/AVC)
    // - Audio: SoundFormat in upper 4 bits (10 = AAC)
    //
    // This is the traditional RTMP format used by older clients and when
    // streaming H.264/AAC content, even from modern clients like OBS.
    // ==========================================================================

    async fn on_video_frame(&self, ctx: &StreamContext, frame: &H264Data, _timestamp: u32) {
        self.video_frames.fetch_add(1, Ordering::Relaxed);

        if let H264Data::SequenceHeader(config) = frame {
            println!(
                "[{}] Video (legacy): H.264 {} profile, level {}",
                ctx.session.session_id,
                config.profile_name(),
                config.level_string()
            );
        }
    }

    async fn on_audio_frame(&self, ctx: &StreamContext, frame: &AacData, _timestamp: u32) {
        self.audio_frames.fetch_add(1, Ordering::Relaxed);

        if let AacData::SequenceHeader(config) = frame {
            println!(
                "[{}] Audio (legacy): {:?}, {} Hz, {} channels",
                ctx.session.session_id,
                config.profile(),
                config.sampling_frequency,
                config.channels()
            );
        }
    }

    // ==========================================================================
    // Enhanced RTMP callbacks (ExVideoTagHeader / ExAudioTagHeader format)
    //
    // Called when the client sends video/audio using the E-RTMP format:
    // - Video: Bit 7 set (0x80), FourCC codec identifier (hvc1, av01, vp09)
    // - Audio: SoundFormat=9, FourCC codec identifier (Opus, fLaC, ac-3)
    //
    // This format is used for modern codecs (HEVC, AV1, VP9, Opus, FLAC, etc.)
    // that aren't supported by the legacy FLV format. OBS 30+ uses this when
    // streaming HEVC or AV1, even without explicit E-RTMP negotiation in the
    // connect command - the format is self-describing at the packet level.
    // ==========================================================================

    async fn on_enhanced_video_frame(
        &self,
        ctx: &StreamContext,
        frame: &EnhancedVideoData,
        _timestamp: u32,
    ) {
        self.video_frames.fetch_add(1, Ordering::Relaxed);

        match frame {
            EnhancedVideoData::SequenceHeader { codec, .. } => {
                println!(
                    "[{}] Video (E-RTMP): {} sequence header",
                    ctx.session.session_id,
                    codec.as_fourcc_str()
                );
            }
            EnhancedVideoData::Frame {
                codec, frame_type, ..
            } if frame_type.is_keyframe() => {
                // Only log first keyframe to avoid spam
                if self.video_frames.load(Ordering::Relaxed) == 1 {
                    println!(
                        "[{}] Video (E-RTMP): {} keyframe",
                        ctx.session.session_id,
                        codec.as_fourcc_str()
                    );
                }
            }
            _ => {}
        }
    }

    async fn on_enhanced_audio_frame(
        &self,
        ctx: &StreamContext,
        frame: &EnhancedAudioData,
        _timestamp: u32,
    ) {
        self.audio_frames.fetch_add(1, Ordering::Relaxed);

        match frame {
            EnhancedAudioData::SequenceHeader { codec, .. } => {
                println!(
                    "[{}] Audio (E-RTMP): {} sequence header",
                    ctx.session.session_id,
                    codec.as_fourcc_str()
                );
            }
            _ => {}
        }
    }

    async fn on_keyframe(&self, _ctx: &StreamContext, _timestamp: u32) {
        self.keyframes.fetch_add(1, Ordering::Relaxed);
    }

    async fn on_unpublish(&self, ctx: &StreamContext) {
        println!(
            "[{}] Unpublish: {} (video={}, audio={}, keyframes={})",
            ctx.session.session_id,
            ctx.stream_key,
            self.video_frames.load(Ordering::Relaxed),
            self.audio_frames.load(Ordering::Relaxed),
            self.keyframes.load(Ordering::Relaxed)
        );
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
                .add_directive("enhanced_server=debug".parse()?),
        )
        .init();

    let bind_addr: SocketAddr = "0.0.0.0:1935".parse()?;

    // Default ServerConfig includes E-RTMP in Auto mode with:
    // - Video: AVC, HEVC, AV1, VP9 (forward capability)
    // - Audio: AAC, Opus (forward capability)
    // - ModEx enabled for extended packet parsing
    let config = ServerConfig::default().bind(bind_addr);

    println!("Starting Enhanced RTMP server on {}", bind_addr);
    println!();
    println!("=== Server Capabilities ===");
    println!("Mode: Auto (E-RTMP if supported, legacy fallback)");
    println!("Video: AVC (H.264), HEVC (H.265), AV1, VP9");
    println!("Audio: AAC, Opus");
    println!("Features: ModEx enabled");
    println!();
    println!("=== Publish a stream ===");
    println!("OBS 30+: Server: rtmp://localhost/live  Stream Key: test");
    println!("ffmpeg:  ffmpeg -re -i input.mp4 -c copy -f flv rtmp://localhost/live/test");
    println!();
    println!("=== Play a stream ===");
    println!("ffplay:  ffplay rtmp://localhost/live/test");
    println!();

    let handler = EnhancedHandler::new();
    let server = RtmpServer::new(config, handler);

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
