//! FLV Recorder - Records an RTMP stream to an FLV file
//!
//! Run with: cargo run --example flv_recorder -- rtmp://localhost/live/test_key output.flv
//!
//! This example demonstrates:
//! - Connecting to an RTMP server as a client
//! - Writing the FLV file format manually (no external libraries)
//! - Graceful shutdown on Ctrl+C
//!
//! # FLV File Format Overview
//!
//! FLV is a simple container format originally designed for Flash Video:
//!
//! ```text
//! +============+==================+==============+==================+==============+
//! | FLV Header | PrevTagSize0 (0) | Tag 1        | PrevTagSize1     | Tag 2  ...   |
//! | (9 bytes)  | (4 bytes)        | (11+N bytes) | (4 bytes)        |              |
//! +============+==================+==============+==================+==============+
//! ```
//!
//! Each tag consists of:
//! ```text
//! +------+----------+-----------+----------+----------+------+
//! | Type | DataSize | Timestamp | TSExt    | StreamID | Data |
//! | 1B   | 3B BE    | 3B BE     | 1B       | 3B (=0)  | N B  |
//! +------+----------+-----------+----------+----------+------+
//! ```
//!
//! Tag types: 8 = audio, 9 = video, 18 = script data (metadata)

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rtmp_rs::client::{ClientConfig, ClientEvent, RtmpPuller};

/// FLV file signature: "FLV" in ASCII
const FLV_SIGNATURE: [u8; 3] = [0x46, 0x4C, 0x56]; // "FLV"

/// FLV version (always 1)
const FLV_VERSION: u8 = 0x01;

/// FLV type flags: bit 0 = video present, bit 2 = audio present
/// 0x05 = 0b00000101 = both audio and video
const FLV_TYPE_FLAGS_AV: u8 = 0x05;

/// FLV header size is always 9 bytes
const FLV_HEADER_SIZE: u32 = 9;

/// FLV tag types
const FLV_TAG_AUDIO: u8 = 8;
const FLV_TAG_VIDEO: u8 = 9;
#[allow(dead_code)]
const FLV_TAG_SCRIPT: u8 = 18;

/// Writes the FLV file header
///
/// The header structure (9 bytes total):
/// - Bytes 0-2: Signature "FLV"
/// - Byte 3: Version (always 1)
/// - Byte 4: Type flags (0x05 for audio+video)
/// - Bytes 5-8: Header size as 32-bit BE (always 9)
fn write_flv_header(writer: &mut impl Write) -> std::io::Result<()> {
    writer.write_all(&FLV_SIGNATURE)?;
    writer.write_all(&[FLV_VERSION])?;
    writer.write_all(&[FLV_TYPE_FLAGS_AV])?;
    writer.write_all(&FLV_HEADER_SIZE.to_be_bytes())?;

    // PreviousTagSize0 is always 0 (no previous tag before first tag)
    writer.write_all(&0u32.to_be_bytes())?;

    Ok(())
}

/// Writes an FLV tag (audio or video)
///
/// Tag structure (11 bytes header + data + 4 bytes previous tag size):
/// - Byte 0: Tag type (8=audio, 9=video, 18=script)
/// - Bytes 1-3: Data size (24-bit BE)
/// - Bytes 4-6: Timestamp lower 24 bits (BE)
/// - Byte 7: Timestamp upper 8 bits (extension for timestamps > 16777215ms)
/// - Bytes 8-10: Stream ID (always 0)
/// - Bytes 11..11+N: Tag data
/// - After data: PreviousTagSize (4 bytes BE) = 11 + data length
///
/// Returns the total number of bytes written (11 + data.len() + 4).
fn write_flv_tag(
    writer: &mut impl Write,
    tag_type: u8,
    timestamp: u32,
    data: &[u8],
) -> std::io::Result<usize> {
    let data_size = data.len() as u32;

    // Tag type (1 byte)
    writer.write_all(&[tag_type])?;

    // Data size (3 bytes, big-endian 24-bit)
    // We extract the lower 24 bits of data_size
    writer.write_all(&[
        ((data_size >> 16) & 0xFF) as u8,
        ((data_size >> 8) & 0xFF) as u8,
        (data_size & 0xFF) as u8,
    ])?;

    // Timestamp (3 bytes lower + 1 byte upper)
    // FLV uses a peculiar format: lower 24 bits first, then upper 8 bits
    // This allows timestamps up to ~49 days (2^32 milliseconds)
    writer.write_all(&[
        ((timestamp >> 16) & 0xFF) as u8, // bits 16-23
        ((timestamp >> 8) & 0xFF) as u8,  // bits 8-15
        (timestamp & 0xFF) as u8,         // bits 0-7
        ((timestamp >> 24) & 0xFF) as u8, // bits 24-31 (extension)
    ])?;

    // Stream ID (3 bytes, always 0 in FLV files)
    writer.write_all(&[0, 0, 0])?;

    // Tag data
    writer.write_all(data)?;

    // PreviousTagSize = tag header (11) + data length
    let prev_tag_size = 11 + data_size;
    writer.write_all(&prev_tag_size.to_be_bytes())?;

    Ok(11 + data.len() + 4)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("rtmp_rs=info".parse()?)
                .add_directive("flv_recorder=info".parse()?),
        )
        .init();

    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: flv_recorder <rtmp_url> <output.flv>");
        eprintln!("Example: flv_recorder rtmp://localhost/live/test_key recording.flv");
        std::process::exit(1);
    }

    let url = &args[1];
    let output_path = PathBuf::from(&args[2]);

    println!("RTMP FLV Recorder");
    println!("=================");
    println!("Source: {}", url);
    println!("Output: {}", output_path.display());
    println!();
    println!("Press Ctrl+C to stop recording...");
    println!();

    // Create output file with buffered writer for better I/O performance
    let file = File::create(&output_path)?;
    let mut writer = BufWriter::new(file);

    // Write FLV header
    write_flv_header(&mut writer)?;

    // Track recording statistics
    let mut video_tags = 0u64;
    let mut audio_tags = 0u64;
    let mut bytes_written = 9u64 + 4; // Header + PreviousTagSize0
    let mut first_timestamp: Option<u32> = None;
    let mut last_timestamp = 0u32;

    // Setup Ctrl+C handler
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = Arc::clone(&running);

    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        running_clone.store(false, Ordering::SeqCst);
    });

    // Connect to RTMP server
    let config = ClientConfig::new(url);
    let (puller, mut events) = RtmpPuller::new(config);

    // Spawn the puller task
    let puller_handle = tokio::spawn(async move {
        if let Err(e) = puller.start().await {
            eprintln!("Puller error: {}", e);
        }
    });

    // Process events and write to FLV file
    while running.load(Ordering::SeqCst) {
        tokio::select! {
            event = events.recv() => {
                match event {
                    Some(ClientEvent::Connected) => {
                        println!("Connected to RTMP server");
                    }

                    Some(ClientEvent::Metadata(metadata)) => {
                        println!("Received stream metadata:");
                        if let Some(width) = metadata.get("width") {
                            if let Some(height) = metadata.get("height") {
                                println!("  Resolution: {:?} x {:?}", width, height);
                            }
                        }
                        if let Some(fps) = metadata.get("framerate") {
                            println!("  Framerate: {:?}", fps);
                        }
                        // Note: We could write metadata as a script tag here,
                        // but it requires AMF serialization. For simplicity,
                        // we just record the raw audio/video tags.
                    }

                    Some(ClientEvent::VideoTag(tag)) => {
                        // Normalize timestamp relative to first frame
                        if first_timestamp.is_none() {
                            first_timestamp = Some(tag.timestamp);
                        }
                        let relative_ts = tag.timestamp.saturating_sub(first_timestamp.unwrap_or(0));
                        last_timestamp = relative_ts;

                        bytes_written += write_flv_tag(&mut writer, FLV_TAG_VIDEO, relative_ts, &tag.data)? as u64;
                        video_tags += 1;

                        // Log keyframes
                        if tag.is_keyframe() {
                            println!(
                                "  Keyframe at {}ms (video: {}, audio: {})",
                                relative_ts, video_tags, audio_tags
                            );
                        }
                    }

                    Some(ClientEvent::AudioTag(tag)) => {
                        // Normalize timestamp relative to first frame
                        if first_timestamp.is_none() {
                            first_timestamp = Some(tag.timestamp);
                        }
                        let relative_ts = tag.timestamp.saturating_sub(first_timestamp.unwrap_or(0));
                        last_timestamp = relative_ts;

                        bytes_written += write_flv_tag(&mut writer, FLV_TAG_AUDIO, relative_ts, &tag.data)? as u64;
                        audio_tags += 1;
                    }

                    Some(ClientEvent::VideoFrame { .. }) | Some(ClientEvent::AudioFrame { .. }) => {
                        // We use raw tags instead of parsed frames for FLV recording
                    }

                    Some(ClientEvent::StreamEnd) => {
                        println!("Stream ended by server");
                        break;
                    }

                    Some(ClientEvent::Error(e)) => {
                        eprintln!("Error: {}", e);
                        break;
                    }

                    Some(ClientEvent::Disconnected) => {
                        println!("Disconnected from server");
                        break;
                    }

                    None => {
                        // Channel closed
                        break;
                    }
                }
            }
        }
    }

    // Ensure all data is flushed to disk
    writer.flush()?;

    // Print final statistics
    println!();
    println!("Recording complete!");
    println!("==================");
    println!("  Video tags: {}", video_tags);
    println!("  Audio tags: {}", audio_tags);
    println!("  Duration:   {:.2}s", last_timestamp as f64 / 1000.0);
    println!("  File size:  {} bytes", bytes_written);
    println!("  Output:     {}", output_path.display());

    // Abort the puller task (it may be blocked on network I/O)
    puller_handle.abort();

    Ok(())
}
