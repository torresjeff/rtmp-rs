# rtmp-rs: RTMP server and client library for Rust

[![Crates.io Version](https://img.shields.io/crates/v/rtmp-rs)](https://crates.io/crates/rtmp-rs)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)
[![Build and tests](https://github.com/torresjeff/rtmp-rs/actions/workflows/rust.yml/badge.svg)](https://github.com/torresjeff/rtmp-rs/actions/workflows/rust.yml)

rtmp-rs is an async RTMP server and RTMP client for Rust, built on Tokio. Use it to ingest live streams from OBS or FFmpeg, relay them to viewers, pull a stream from another server, or publish to YouTube, Twitch and other RTMPS endpoints. It supports legacy RTMP (H.264 and AAC) as well as Enhanced RTMP v2 with HEVC, AV1, VP9, Opus and FLAC.

The server accepts what real encoders send. Empty app names, timestamp regressions and other quirks are tolerated instead of dropping the connection. A GOP cache gives late joiners a keyframe right away, and slow subscribers lose video frames before they lose audio.

## What you get

* An RTMP server with stream key routing and pub/sub built in. Publishers and players are matched by stream key with no extra code.
* An RTMP client that can pull a stream or publish one, over plain RTMP or RTMPS (TLS).
* Enhanced RTMP v2 with codec negotiation. HEVC, AV1, VP9 and VP8 video, Opus, FLAC, AC-3 and E-AC-3 audio, plus H.264 and AAC. Older clients fall back to legacy RTMP automatically.
* A `RtmpHandler` trait with optional callbacks for authentication, metadata and per-frame access. Every callback has a default, so you override only the ones you care about.
* GOP caching so a viewer joining mid-stream starts on a keyframe instead of waiting for the next one.
* Backpressure handling that drops video for slow subscribers while keeping audio continuous.
* Media payloads passed around as `bytes::Bytes`, so a frame is shared between subscribers rather than copied per connection.
* Parsers for FLV tags, H.264 NAL units, AAC frames and the Enhanced RTMP packet formats.

## Install

```bash
cargo add rtmp-rs
```

For RTMPS, enable the `tls` feature:

```bash
cargo add rtmp-rs --features tls
```

## RTMP server in Rust

A working server is a handler struct and a few lines of `main`:

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
        // Validate the stream key here
        AuthResult::Accept
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = RtmpServer::new(ServerConfig::default(), MyHandler);
    server.run().await?;
    Ok(())
}
```

Then point an encoder at it:

```bash
# OBS: Server rtmp://localhost/live, Stream Key test

# FFmpeg
ffmpeg -re -i input.mp4 -c copy -f flv rtmp://localhost/live/test

# Watch it
ffplay rtmp://localhost/live/test
```

### Server configuration

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

`RtmpServer::run_until` takes a shutdown future if you need graceful shutdown.

## RTMP client: pull a stream

`RtmpPuller` connects to a server, plays a stream and hands you parsed frames and raw FLV tags over a channel:

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

## RTMP client: publish a stream

`RtmpPublisher` pushes a stream to a remote server. It is codec-agnostic. You hand it FLV audio tag bodies with timestamps and it does the chunking and the RTMP handshake:

```rust
use bytes::Bytes;
use rtmp_rs::client::{ClientConfig, PublishEvent, RtmpPublisher};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ClientConfig::new("rtmp://server/live/stream_key");
    let (mut publisher, mut events) = RtmpPublisher::new(config);

    tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            if let PublishEvent::Error(e) = event {
                eprintln!("publish error: {e}");
            }
        }
    });

    publisher.connect().await?;

    // AAC sequence header: FLV audio tag header (0xAF) + packet type 0 + AudioSpecificConfig
    let header = Bytes::from(vec![0xAF, 0x00, 0x12, 0x10]);
    publisher.send_audio(header, 0).await?;

    // Raw AAC frames follow with packet type 1
    // publisher.send_audio(Bytes::from(vec![0xAF, 0x01, /* AAC frame */]), timestamp_ms).await?;

    publisher.disconnect().await;
    Ok(())
}
```

The publisher currently sends audio only. See `RtmpPublisher::send_audio` for the FLV tag layout per codec.

## RTMPS (RTMP over TLS)

With the `tls` feature enabled, any `rtmps://` URL uses TLS. The default port is 443. Certificates are checked against the Mozilla root store bundled by `webpki-roots`, so public endpoints such as YouTube and Twitch work without configuration:

```rust
let config = ClientConfig::new("rtmps://a.rtmps.youtube.com:443/live2/STREAM_KEY");
```

To trust a private CA or a self-signed certificate on a server you run, add it as an extra root. The certificate type comes from `rustls`, so add `rustls` (or `rustls-pki-types`) to your own dependencies to name it:

```rust
use rustls::pki_types::CertificateDer;

let cert: CertificateDer<'static> = /* load DER */;
let config = ClientConfig::new("rtmps://ingest.internal:1936/live/key")
    .tls_root_cert(cert);
```

Extra roots are added on top of the built-in store.

## Handler callbacks

`RtmpHandler` is where authentication and media processing live. Override what you need and leave the rest:

```rust
use rtmp_rs::{RtmpHandler, AuthResult};
use rtmp_rs::session::SessionContext;
use rtmp_rs::protocol::message::PublishParams;

struct AuthHandler;

impl RtmpHandler for AuthHandler {
    async fn on_publish(&self, _ctx: &SessionContext, params: &PublishParams) -> AuthResult {
        if validate_stream_key(&params.stream_key) {
            AuthResult::Accept
        } else {
            AuthResult::Reject("Invalid stream key".into())
        }
    }
}
```

| Callback | When to use it |
|----------|----------------|
| `on_connection` | New TCP connection. IP blocklists, rate limiting |
| `on_handshake_complete` | After the RTMP handshake, before `connect` |
| `on_connect` | Validate the app name, read auth tokens from `tcUrl` |
| `on_disconnect` | Cleanup and logging |
| `on_fc_publish` | Early stream key check (OBS sends this before `publish`) |
| `on_publish` | Stream key authentication |
| `on_unpublish` | Publisher went away |
| `on_play` | Subscriber authorization |
| `on_pause`, `on_unpause` | Subscriber paused or resumed |
| `on_metadata` | Resolution, bitrate, codec from `onMetaData` |
| `on_media_tag` | Raw FLV tags, for recording or filtering |
| `on_video_frame` | H.264 NAL units (legacy RTMP) |
| `on_audio_frame` | AAC frames (legacy RTMP) |
| `on_enhanced_video_frame` | HEVC, AV1, VP9, VP8 frames (Enhanced RTMP) |
| `on_enhanced_audio_frame` | Opus, FLAC, AC-3, E-AC-3 frames (Enhanced RTMP) |
| `on_keyframe` | GOP boundaries |

## Enhanced RTMP: HEVC, AV1, Opus and more

rtmp-rs implements [Enhanced RTMP v2](https://github.com/veovera/enhanced-rtmp). The default mode is `Auto`, which advertises capabilities during `connect` and falls back to legacy RTMP for clients that do not understand them. OBS 30+ and recent FFmpeg builds can send HEVC and AV1 this way.

| Video | Audio |
|-------|-------|
| H.264/AVC | AAC |
| H.265/HEVC | Opus |
| AV1 | FLAC |
| VP9 | AC-3 |
| VP8 | E-AC-3 |

### Choosing a mode and codecs

```rust
use rtmp_rs::{ServerConfig, EnhancedRtmpMode, EnhancedServerCapabilities};
use rtmp_rs::media::fourcc::{VideoFourCc, AudioFourCc};
use rtmp_rs::protocol::enhanced::FourCcCapability;

// Default: Auto mode with the common codecs
let config = ServerConfig::default();

// Require Enhanced RTMP and reject legacy clients
let config = ServerConfig::default()
    .enhanced_rtmp(EnhancedRtmpMode::EnhancedOnly);

// Legacy RTMP only
let config = ServerConfig::default()
    .enhanced_rtmp(EnhancedRtmpMode::LegacyOnly);

// Advertise a specific codec set
let caps = EnhancedServerCapabilities::minimal()
    .with_video_codec(VideoFourCc::Hevc, FourCcCapability::forward())
    .with_video_codec(VideoFourCc::Av1, FourCcCapability::forward())
    .with_audio_codec(AudioFourCc::Opus, FourCcCapability::forward());

let config = ServerConfig::default()
    .enhanced_capabilities(caps);
```

### Handling Enhanced RTMP frames

```rust
use rtmp_rs::media::{EnhancedVideoData, EnhancedAudioData};
use rtmp_rs::session::StreamContext;

impl RtmpHandler for MyHandler {
    async fn on_enhanced_video_frame(
        &self,
        ctx: &StreamContext,
        frame: &EnhancedVideoData,
        timestamp: u32,
    ) {
        match frame {
            EnhancedVideoData::SequenceHeader { codec, config, .. } => {
                println!("Received {} sequence header", codec);
            }
            EnhancedVideoData::Frame { codec, frame_type, data, .. } => {
                if frame_type.is_keyframe() {
                    println!("{} keyframe at {}ms", codec, timestamp);
                }
            }
            _ => {}
        }
    }

    async fn on_enhanced_audio_frame(
        &self,
        ctx: &StreamContext,
        frame: &EnhancedAudioData,
        timestamp: u32,
    ) {
        if let EnhancedAudioData::SequenceHeader { codec, .. } = frame {
            println!("Received {} audio config", codec);
        }
    }
}
```

## Examples

The `examples/` directory has runnable programs for the common setups:

```bash
cargo run --example simple_server            # accept publishers, log events
cargo run --example enhanced_server          # same, with HEVC/AV1 handling
cargo run --example flv_recorder_server -- ./recordings   # record every incoming stream to FLV
cargo run --example puller -- rtmp://localhost/live/test
cargo run --example flv_recorder_client -- rtmp://localhost/live/test out.flv
```

## Compatibility

Tested with OBS Studio, FFmpeg and ffplay as publishers and players. Lenient parsing covers the quirks these encoders are known for, including empty app names, `FCPublish` before `publish`, and non-monotonic timestamps. The RTMPS client is exercised in CI against a TLS-terminating proxy with a self-signed certificate.

## AI disclaimer

This repo is a Rust rewrite of my [RTMP Go server](https://github.com/torresjeff/rtmp). Almost all of the code in this Rust version was written by AI (Claude Opus 4.5).

I recently had an idea that required an RTMP server, so I used it as an excuse to write some Rust and try out some agentic programming. This repo is partly an experiment to see how far I could get by vibecoding the entire thing with Claude Code. The answer? **Far!**

The whole thing took around 8 hours. It probably could have been faster if I auto-accepted edits without reading the code, but I like to review everything the agent generates. I started with Plan Mode to define the requirements, then moved on to implementation.

That said, there was a tricky timestamp bug that caused audio/video stuttering, and Claude kept hallucinating answers instead of helping. After a deep-dive on my own, I found the root cause. I also noticed some parts of the code that could be improved, but I decided to keep things as-is for now. Any future improvements I'll have Claude handle.

## License

Licensed under the [MIT license](LICENSE).
