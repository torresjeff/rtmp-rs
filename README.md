# rtmp-rs

[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)
[![Build and tests](https://github.com/torresjeff/rtmp-rs/actions/workflows/rust.yml/badge.svg)](https://github.com/torresjeff/rtmp-rs/actions/workflows/rust.yml)

**Async RTMP server and client library for Rust** - Build live video streaming infrastructure with Tokio.

**rtmp-rs** is an RTMP server and client library written in Rust, built to handle the messy reality of live streaming. Encoders like OBS and FFmpeg all have their quirks, so rtmp-rs uses lenient parsing to roll with it instead of rejecting non-conforming streams. Late joiners get instant playback with keyframe caching, and smart backpressure handling keeps audio flowing for slow subscribers while selectively dropping video frames. If you're building a streaming service or relay and want RTMP ingest that just works, this might be what you're looking for.


## Features

* **Async/Await** - Built on Tokio for high-performance concurrent connections
* **Zero-Copy** - Uses `bytes::Bytes` throughout for efficient memory handling
* **Backpressure Handling** - Slow subscribers drop video frames while audio keeps flowing, so viewers hear continuous sound instead of staring at a frozen frame
* **Built-in Pub/Sub** - Stream key routing works out of the box; no code required
* **Late-Joiner GOP Cache** - Buffers keyframes so viewers don't wait for the next IDR frame when joining mid-stream
* **Lenient Parsing** - Handles encoder quirks like empty app names and timestamp regression (OBS, Twitch compatible)
* **Extensible** - Optional `RtmpHandler` callbacks for custom auth and media processing

## Quick Start

**1. Add dependency:**

```bash
cargo add rtmp-rs
```

**2. Create a server:**

```rust
use rtmp_rs::{RtmpServer, ServerConfig, RtmpHandler, AuthResult};
use rtmp_rs::session::SessionContext;
use rtmp_rs::protocol::message::{ConnectParams, PublishParams};

struct MyHandler;

impl RtmpHandler for MyHandler {
    async fn on_connect(&self, _ctx: &SessionContext, params: &ConnectParams) -> AuthResult {
        println!("App: {}", params.app);
        AuthResult::Accept
    }

    async fn on_publish(&self, _ctx: &SessionContext, params: &PublishParams) -> AuthResult {
        println!("Stream key: {}", params.stream_key);
        // Add your stream key validation here
        AuthResult::Accept
    }
    
    // See RtmpHandler trait for other available callbacks
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = RtmpServer::new(ServerConfig::default(), MyHandler);
    server.run().await?;
    Ok(())
}
```

**3. Stream to it:**

```bash
# OBS: Server: rtmp://localhost/live  Stream Key: test

# ffmpeg:
ffmpeg -re -i input.mp4 -c copy -f flv rtmp://localhost/live/test
```

## Client Example (Pull Stream)

```rust
use rtmp_rs::client::{ClientConfig, ClientEvent, RtmpPuller};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ClientConfig::new("rtmp://server/live/stream_key");
    let (puller, mut events) = RtmpPuller::new(config);

    tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            match event {
                ClientEvent::VideoFrame { timestamp, data } => {
                    if data.is_keyframe() {
                        println!("Keyframe at {}ms", timestamp);
                    }
                }
                ClientEvent::AudioFrame { timestamp, .. } => {
                    println!("Audio at {}ms", timestamp);
                }
                ClientEvent::Disconnected => break,
                _ => {}
            }
        }
    });

    puller.start().await?;
    Ok(())
}
```

## Handler Callbacks

The `RtmpHandler` trait provides optional hooks for custom logic. All callbacks have sensible defaults that accept connections and allow streams - you only override what you need:

```rust
use rtmp_rs::{RtmpHandler, AuthResult};
use rtmp_rs::session::SessionContext;
use rtmp_rs::protocol::message::PublishParams;

struct AuthHandler;

impl RtmpHandler for AuthHandler {
    // Only override what you need - everything else uses defaults
    async fn on_publish(&self, _ctx: &SessionContext, params: &PublishParams) -> AuthResult {
        if validate_stream_key(&params.stream_key) {
            AuthResult::Accept
        } else {
            AuthResult::Reject("Invalid stream key".into())
        }
    }
}
```

Available callbacks:

| Callback | Use Case |
|----------|----------|
| `on_connection` | IP blocklist, rate limiting |
| `on_handshake_complete` | Post-handshake setup, before connect command, logging |
| `on_connect` | Validate app name, parse auth tokens from tcUrl |
| `on_disconnect` | Connection cleanup, logging |
| `on_fc_publish` | Early stream key validation (OBS sends this first) |
| `on_publish` | Main stream key authentication |
| `on_publish_stop` | Publisher cleanup, notifications |
| `on_play` | Subscriber authorization |
| `on_pause` | Handle subscriber pause |
| `on_unpause` | Handle subscriber resume |
| `on_metadata` | Capture stream info (resolution, bitrate, codec) |
| `on_media_tag` | Raw FLV tag access, custom filtering |
| `on_video_frame` | Process H.264 NALUs |
| `on_audio_frame` | Process AAC frames |
| `on_keyframe` | Track GOP boundaries |

## Configuration

```rust
use std::time::Duration;
use rtmp_rs::ServerConfig;

let config = ServerConfig::default()
    .bind("0.0.0.0:1935".parse()?)
    .max_connections(1000)
    .chunk_size(4096)
    .connection_timeout(Duration::from_secs(10))
    .idle_timeout(Duration::from_secs(60));
```

## Testing

```bash
# Stream (publish) with ffmpeg
ffmpeg -re -i test.mp4 -c copy -f flv rtmp://localhost/live/test_key

# Play with ffplay
ffplay rtmp://localhost/live/test_key
```


## AI disclaimer

This repo is a Rust rewrite of my [RTMP Go server](https://github.com/torresjeff/rtmp), which was my first time dabbling in video codecs and streaming. Almost all of the code in this Rust version was written by AI (Claude Opus 4.5).

I recently had an idea that required an RTMP server, so I used it as an excuse to write some Rust and try out some agentic programming. This repo is partly an experiment to see how far I could get by vibecoding the entire thing with Claude Code. The answer? **Far!**
 
The whole thing took around 8 hours. It probably could have been faster if I auto-accepted edits without reading the code, but I like to review everything the agent generates. I started with Plan Mode to define the requirements, then moved on to implementation.

That said, there was a tricky timestamp bug that caused audio/video stuttering, and Claude kept hallucinating answers instead of helping. After a deep-dive on my own, I found the root cause. I also noticed some parts of the code that could be improved, but I decided to keep things as-is for now. Any future improvements I'll have Claude handle.


## License

Licensed under [MIT license](LICENSE)
