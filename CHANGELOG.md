# Changelog

All notable changes to this project will be documented in this file.

## [0.2.0] - 2026-01-23

### BREAKING CHANGES

- **Removed `async_trait` dependency** - The library now uses Rust's native async traits (stabilized in Rust 1.75). This is a breaking change that affects all `RtmpHandler` implementations.
  - **Minimum Rust version increased from 1.70 to 1.75**
  - The `#[async_trait]` attribute is no longer needed on handler implementations
  - The `async_trait` crate is no longer required as a dependency

### Added

- New `flv_recorder` example demonstrating how to record RTMP streams to FLV files without external dependencies
- `on_play_stop` callback is now properly invoked when play sessions end (was previously defined but never called)
- Significantly expanded test coverage from ~41 tests to 271 tests

### Changed

- Refactored registry module into separate files (`frame.rs`, `entry.rs`, `store.rs`, `error.rs`) for improved code organization
- Updated README with complete Handler Callbacks table documenting all available callbacks
- Improved documentation for `media_delivery_mode` in examples

### Fixed

- `on_play_stop` callback now properly invoked when play sessions end

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

## [0.1.0] - Initial Release

- Initial release of rtmp-rs
- RTMP server implementation with `RtmpHandler` trait
- RTMP client with `RtmpConnector` and `RtmpPuller`
- AMF0/AMF3 serialization support
- H.264/AVC and AAC parsing
- GOP buffering for late-joiner support
- OBS and encoder compatibility handling
- Examples: `simple_server`, `puller`

[0.2.0]: https://github.com/torresjeff/rtmp-rs/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/torresjeff/rtmp-rs/releases/tag/v0.1.0
