//! RTMP stream puller example
//!
//! Run with: cargo run --example puller -- rtmp://localhost/live/test_key
//!
//! This connects to an RTMP server and pulls a stream, printing frame info.

use rtmp_rs::client::{ClientConfig, ClientEvent, RtmpPuller};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("rtmp_rs=debug".parse()?)
                .add_directive("puller=info".parse()?),
        )
        .init();

    // Get URL from command line
    let url = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("Usage: puller <rtmp_url>");
        eprintln!("Example: puller rtmp://localhost/live/test_key");
        std::process::exit(1);
    });

    println!("Connecting to {}", url);

    let config = ClientConfig::new(&url);
    let (puller, mut events) = RtmpPuller::new(config);

    // Spawn event handler
    let event_handle = tokio::spawn(async move {
        let mut video_frames = 0u64;
        let mut audio_frames = 0u64;
        let mut keyframes = 0u64;

        while let Some(event) = events.recv().await {
            match event {
                ClientEvent::Connected => {
                    println!("Connected!");
                }
                ClientEvent::Metadata(metadata) => {
                    println!("Metadata received:");
                    if let Some(width) = metadata.get("width") {
                        println!("  Width: {:?}", width);
                    }
                    if let Some(height) = metadata.get("height") {
                        println!("  Height: {:?}", height);
                    }
                }
                ClientEvent::VideoFrame { timestamp, data } => {
                    video_frames += 1;
                    if data.is_keyframe() {
                        keyframes += 1;
                        println!("  Keyframe at {}", timestamp);
                    }
                    if video_frames % 100 == 0 {
                        println!(
                            "Progress: {} video, {} audio, {} keyframes",
                            video_frames, audio_frames, keyframes
                        );
                    }
                }
                ClientEvent::AudioFrame { timestamp: _, data: _ } => {
                    audio_frames += 1;
                }
                ClientEvent::VideoTag(_) | ClientEvent::AudioTag(_) => {
                    // Raw tags - we're using parsed frames
                }
                ClientEvent::StreamEnd => {
                    println!("Stream ended");
                    break;
                }
                ClientEvent::Error(e) => {
                    eprintln!("Error: {}", e);
                    break;
                }
                ClientEvent::Disconnected => {
                    println!("Disconnected");
                    break;
                }
            }
        }

        println!(
            "Final stats: {} video, {} audio, {} keyframes",
            video_frames, audio_frames, keyframes
        );
    });

    // Start puller
    if let Err(e) = puller.start().await {
        eprintln!("Puller error: {}", e);
    }

    // Wait for event handler
    let _ = event_handle.await;

    Ok(())
}
