# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.0] - 2026-01-27

### Added

- **Enhanced RTMP (E-RTMP) v2 Support** - Full implementation of the [E-RTMP specification](https://github.com/veovera/enhanced-rtmp) for modern codec streaming.

#### Modern Codec Support

- **Video codecs**: HEVC (H.265), AV1, VP9, VP8 in addition to legacy H.264/AVC
- **Audio codecs**: Opus, FLAC, AC-3, E-AC-3, MP3 in addition to legacy AAC
- FOURCC-based codec identification for extensibility

#### Capability Negotiation

- Server and client negotiate supported codecs during the connect handshake
- Backwards compatible - automatically falls back to legacy RTMP for older clients
- Self-describing packet format allows HEVC/AV1 streaming even without explicit negotiation

#### New Types

- `EnhancedRtmpMode` - Control E-RTMP behavior: `Auto` (default), `LegacyOnly`, `EnhancedOnly`
- `EnhancedServerCapabilities` - Configure server-side codec and feature support
- `EnhancedClientCapabilities` - Configure client-side codec and feature support
- `EnhancedCapabilities` - Negotiated capabilities for a session
- `CapsEx` - Extended capability flags (reconnect, multitrack, ModEx, nanosecond timestamps)
- `FourCcCapability` - Codec capability flags (decode, encode, forward)
- `VideoFunctionFlags` - Video function flags (client seek support)
- `VideoFourCc` - Video codec identifiers (Avc, Hevc, Av1, Vp9, Vp8)
- `AudioFourCc` - Audio codec identifiers (Aac, Opus, Flac, Ac3, Eac3, Mp3)
- `FourCC` - Generic four-character code for codec identification
- `EnhancedVideoData` - Parsed enhanced video frames with codec info
- `EnhancedAudioData` - Parsed enhanced audio frames with codec info
- `VideoPacketType` - Enhanced video packet types (SequenceStart, CodedFrames, etc.)
- `AudioPacketType` - Enhanced audio packet types
- `ExVideoFrameType` - Enhanced video frame types (Keyframe, InterFrame, etc.)
- `AvMultitrackType` - Multitrack container types
- `AudioChannelOrder` - Multichannel audio ordering schemes
- `VideoTrack`, `AudioTrack` - Individual tracks in multitrack containers
- `EnhancedVideoTrackData`, `EnhancedAudioTrackData` - Track-level data types

#### New Handler Callbacks

- `on_enhanced_video_frame` - Called for E-RTMP video (HEVC, AV1, VP9, VP8)
- `on_enhanced_audio_frame` - Called for E-RTMP audio (Opus, FLAC, AC-3, E-AC-3)

#### New Configuration Options

- `ServerConfig::enhanced_rtmp()` - Set E-RTMP mode
- `ServerConfig::enhanced_capabilities()` - Configure server codec support
- `ClientConfig::enhanced_rtmp()` - Set E-RTMP mode for client
- `ClientConfig::enhanced_capabilities()` - Configure client codec support

#### New Example

- `enhanced_server` - Demonstrates E-RTMP capability negotiation and modern codec handling

### Notes

- E-RTMP is enabled by default in `Auto` mode, which negotiates with clients and falls back gracefully
- The `ExVideoTagHeader` and `ExAudioTagHeader` formats are self-describing, so modern codecs work even when capability negotiation doesn't occur (e.g., OBS 30+ streaming HEVC)
- Legacy `on_video_frame` and `on_audio_frame` callbacks continue to work for H.264/AAC streams
- For relay servers, use `FourCcCapability::forward()` to indicate passthrough support without transcoding

## [0.4.0] - 2026-01-24

### Added

- `AvcConfig` now exposes a `raw: Bytes` field containing the original AVCDecoderConfigurationRecord bytes. This allows direct access to the H.264 sequence header data for use cases like FLV recording, stream relaying, or custom media processing. This matches the existing `raw` field pattern in `AudioSpecificConfig` for AAC.

## [0.3.0] - 2026-01-23

### Added

- New `on_unpublish` callback in `RtmpHandler` trait, called when a publisher stops streaming. This provides a cleaner, more intuitive API naming that pairs with `on_publish`.

### Deprecated

- `on_publish_stop` is now deprecated in favor of `on_unpublish`. The method is marked with `#[deprecated(since = "0.3.0", note = "Use on_unpublish instead")]`.

### Notes

- **Backward compatibility**: Both `on_publish_stop` and `on_unpublish` are called when a publish stream ends. Existing implementations using `on_publish_stop` will continue to work, but should migrate to `on_unpublish` to avoid deprecation warnings.

## [0.2.0] - 2026-01-23

### BREAKING CHANGES

- **Removed `async_trait` dependency** - The library now uses Rust's native async traits (stabilized in Rust 1.75). This is a breaking change that affects all `RtmpHandler` implementations.
  - **Minimum Rust version increased from 1.70 to 1.75**
  - The `#[async_trait]` attribute is no longer needed on handler implementations
  - The `async_trait` crate is no longer required as a dependency

### Changed

- Migrated all async trait methods to use `impl Future` return types instead of `#[async_trait]` macro
- Updated all examples and documentation to reflect native async trait syntax

## Migration from 0.1.x

### Updating your RtmpHandler implementation

**Before (0.1.x):**

```rust
use async_trait::async_trait;
use rtmp_rs::{RtmpHandler, SessionContext, StreamContext};

struct MyHandler;

#[async_trait]
impl RtmpHandler for MyHandler {
    async fn on_connect(&self, ctx: &SessionContext) -> Result<(), Error> {
        // ...
    }

    async fn on_publish(&self, ctx: &StreamContext) -> Result<(), Error> {
        // ...
    }
}
```

**After (0.2.0):**

```rust
use rtmp_rs::{RtmpHandler, SessionContext, StreamContext};

struct MyHandler;

impl RtmpHandler for MyHandler {
    async fn on_connect(&self, ctx: &SessionContext) -> Result<(), Error> {
        // ...
    }

    async fn on_publish(&self, ctx: &StreamContext) -> Result<(), Error> {
        // ...
    }
}
```

### Steps to migrate

1. **Update your Rust toolchain** to 1.75 or later:
   ```bash
   rustup update stable
   ```

2. **Remove the `async_trait` dependency** from your `Cargo.toml`:
   ```diff
   [dependencies]
   - async_trait = "0.1"
   ```

3. **Remove `#[async_trait]` attributes** from your handler implementations:
   ```diff
   - use async_trait::async_trait;

   - #[async_trait]
     impl RtmpHandler for MyHandler {
   ```

4. **Rebuild your project**:
   ```bash
   cargo build
   ```

## [0.1.4] - 2026-01-22

### Changed

- Refactored registry module into separate files for improved code organization:
  - `entry.rs` - Stream entry management
  - `error.rs` - Registry error types
  - `frame.rs` - Frame handling
  - `store.rs` - Stream storage
- Improved README Handler Callbacks documentation with expanded table showing all available callbacks

## [0.1.3] - 2026-01-21

### Added

- New `flv_recorder` example demonstrating how to record RTMP streams to FLV files without external dependencies

### Changed

- Improved README documentation with clearer descriptions for Pub/Sub and handler callbacks

## [0.1.2] - 2026-01-20

### Added

- GitHub Actions CI workflow for automated builds and tests
- Build status badge in README

### Changed

- Improved backpressure handling description - clarified that slow subscribers drop video frames while audio keeps flowing
- Moved AI disclaimer section to bottom of README for better documentation flow

## [0.1.1] - 2026-01-19

### Fixed

- Minor bug fixes and stability improvements

## [0.1.0] - 2026-01-18

### Added

- Initial release of rtmp-rs
- RTMP server implementation with `RtmpHandler` trait for custom authentication and media processing
- RTMP client with `RtmpConnector` and `RtmpPuller` for pulling streams
- AMF0/AMF3 serialization support
- H.264/AVC and AAC parsing
- GOP buffering for late-joiner support
- OBS and encoder compatibility with lenient parsing mode
- Backpressure handling for slow subscribers
- Zero-copy design using `bytes::Bytes`
- Examples: `simple_server`, `puller`

[0.5.0]: https://github.com/torresjeff/rtmp-rs/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/torresjeff/rtmp-rs/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/torresjeff/rtmp-rs/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/torresjeff/rtmp-rs/compare/v0.1.4...v0.2.0
[0.1.4]: https://github.com/torresjeff/rtmp-rs/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/torresjeff/rtmp-rs/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/torresjeff/rtmp-rs/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/torresjeff/rtmp-rs/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/torresjeff/rtmp-rs/releases/tag/v0.1.0
