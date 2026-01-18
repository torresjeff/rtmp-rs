//! OBS and encoder compatibility quirks
//!
//! Different RTMP encoders (OBS, ffmpeg, Wirecast, etc.) have various
//! non-standard behaviors. This module documents known quirks and provides
//! helpers for handling them.
//!
//! # Known Quirks
//!
//! ## OBS Studio
//! - Sends FCPublish before publish (Twitch/YouTube compatibility)
//! - May send releaseStream before connect completes
//! - Sometimes omits object end markers in AMF
//! - Sends @setDataFrame with onMetaData as nested name
//!
//! ## ffmpeg
//! - Uses different transaction IDs than expected
//! - May send createStream before connect response
//! - Duration in metadata may be 0 for live streams
//!
//! ## Flash Media Encoder
//! - Uses legacy AMF0 object encoding
//! - May send duplicate metadata
//!
//! ## Wirecast
//! - Sends multiple audio/video sequence headers
//! - May have timestamp discontinuities

use crate::protocol::message::Command;

/// Configuration for handling encoder quirks
#[derive(Debug, Clone)]
pub struct QuirksConfig {
    /// Accept commands before handshake completes
    pub allow_early_commands: bool,

    /// Accept FCPublish/releaseStream before connect
    pub allow_fc_before_connect: bool,

    /// Accept malformed AMF (missing end markers)
    pub lenient_amf: bool,

    /// Accept timestamp regression
    pub allow_timestamp_regression: bool,

    /// Accept duplicate metadata
    pub allow_duplicate_metadata: bool,

    /// Accept empty app names
    pub allow_empty_app: bool,

    /// Accept oversized chunks (larger than negotiated)
    pub allow_oversized_chunks: bool,
}

impl Default for QuirksConfig {
    fn default() -> Self {
        Self {
            // Default to lenient for maximum compatibility
            allow_early_commands: true,
            allow_fc_before_connect: true,
            lenient_amf: true,
            allow_timestamp_regression: true,
            allow_duplicate_metadata: true,
            allow_empty_app: true,
            allow_oversized_chunks: true,
        }
    }
}

impl QuirksConfig {
    /// Strict mode - reject non-conformant streams
    pub fn strict() -> Self {
        Self {
            allow_early_commands: false,
            allow_fc_before_connect: false,
            lenient_amf: false,
            allow_timestamp_regression: false,
            allow_duplicate_metadata: false,
            allow_empty_app: false,
            allow_oversized_chunks: false,
        }
    }
}

/// Detected encoder type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncoderType {
    Unknown,
    Obs,
    Ffmpeg,
    Wirecast,
    FlashMediaEncoder,
    Xsplit,
    Larix,
    Other,
}

impl EncoderType {
    /// Detect encoder from connect command's flashVer
    pub fn from_flash_ver(flash_ver: &str) -> Self {
        let lower = flash_ver.to_lowercase();

        if lower.contains("obs") {
            EncoderType::Obs
        } else if lower.contains("fmle") || lower.contains("flash media") {
            EncoderType::FlashMediaEncoder
        } else if lower.contains("wirecast") {
            EncoderType::Wirecast
        } else if lower.contains("xsplit") {
            EncoderType::Xsplit
        } else if lower.contains("larix") {
            EncoderType::Larix
        } else if lower.contains("lavf") || lower.contains("librtmp") {
            EncoderType::Ffmpeg
        } else {
            EncoderType::Other
        }
    }
}

/// OBS/Twitch command sequence helper
///
/// Many streaming platforms expect a specific command sequence:
/// 1. connect -> _result
/// 2. releaseStream (optional)
/// 3. FCPublish
/// 4. createStream -> _result
/// 5. publish -> onStatus
pub struct CommandSequence {
    state: CommandSequenceState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandSequenceState {
    Initial,
    Connected,
    StreamCreated,
    Publishing,
    Playing,
}

impl CommandSequence {
    pub fn new() -> Self {
        Self {
            state: CommandSequenceState::Initial,
        }
    }

    /// Check if a command is valid in the current state
    pub fn is_valid_command(&self, cmd: &Command) -> bool {
        match cmd.name.as_str() {
            "connect" => self.state == CommandSequenceState::Initial,
            "releaseStream" | "FCPublish" => {
                // These can come before or after connect completes (OBS quirk)
                true
            }
            "createStream" => {
                self.state == CommandSequenceState::Connected
                    || self.state == CommandSequenceState::Initial // OBS quirk
            }
            "publish" => self.state == CommandSequenceState::StreamCreated,
            "play" => self.state == CommandSequenceState::StreamCreated,
            "FCUnpublish" | "deleteStream" | "closeStream" => {
                self.state == CommandSequenceState::Publishing
                    || self.state == CommandSequenceState::Playing
            }
            _ => true, // Allow unknown commands
        }
    }

    /// Transition state based on command response
    pub fn on_command(&mut self, cmd_name: &str) {
        match cmd_name {
            "connect" => self.state = CommandSequenceState::Connected,
            "createStream" => self.state = CommandSequenceState::StreamCreated,
            "publish" => self.state = CommandSequenceState::Publishing,
            "play" => self.state = CommandSequenceState::Playing,
            "FCUnpublish" | "deleteStream" | "closeStream" => {
                self.state = CommandSequenceState::Connected;
            }
            _ => {}
        }
    }

    /// Get current state
    pub fn state(&self) -> &'static str {
        match self.state {
            CommandSequenceState::Initial => "initial",
            CommandSequenceState::Connected => "connected",
            CommandSequenceState::StreamCreated => "stream_created",
            CommandSequenceState::Publishing => "publishing",
            CommandSequenceState::Playing => "playing",
        }
    }
}

impl Default for CommandSequence {
    fn default() -> Self {
        Self::new()
    }
}

/// Normalize timestamp to handle regression
///
/// Some encoders have timestamp discontinuities or regressions.
/// This function adjusts timestamps to be monotonically increasing.
pub struct TimestampNormalizer {
    last_timestamp: u32,
    offset: u32,
}

impl TimestampNormalizer {
    pub fn new() -> Self {
        Self {
            last_timestamp: 0,
            offset: 0,
        }
    }

    /// Normalize a timestamp, handling regression
    pub fn normalize(&mut self, timestamp: u32) -> u32 {
        // Check for significant regression (more than 1 second)
        if timestamp < self.last_timestamp && self.last_timestamp - timestamp > 1000 {
            // Timestamp regressed significantly, adjust offset
            self.offset = self.last_timestamp + 1;
        }

        let normalized = timestamp.wrapping_add(self.offset);
        self.last_timestamp = normalized;
        normalized
    }

    /// Reset normalizer state
    pub fn reset(&mut self) {
        self.last_timestamp = 0;
        self.offset = 0;
    }
}

impl Default for TimestampNormalizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encoder_detection() {
        assert_eq!(
            EncoderType::from_flash_ver("OBS-Studio/29.1.3"),
            EncoderType::Obs
        );
        assert_eq!(
            EncoderType::from_flash_ver("FMLE/3.0"),
            EncoderType::FlashMediaEncoder
        );
        assert_eq!(
            EncoderType::from_flash_ver("Lavf58.76.100"),
            EncoderType::Ffmpeg
        );
    }

    #[test]
    fn test_timestamp_normalizer() {
        let mut normalizer = TimestampNormalizer::new();

        assert_eq!(normalizer.normalize(0), 0);
        assert_eq!(normalizer.normalize(100), 100);
        assert_eq!(normalizer.normalize(200), 200);

        // Small regression (within 1 second) - allow it
        assert_eq!(normalizer.normalize(150), 150);

        // Large regression - adjust offset
        assert_eq!(normalizer.normalize(50), 251); // 50 + (200+1)
    }
}
