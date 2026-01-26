//! FOURCC codec identifiers for Enhanced RTMP
//!
//! FOURCC (Four Character Code) is a sequence of four bytes used to uniquely
//! identify data formats. E-RTMP uses FOURCC to signal modern codecs like
//! HEVC, AV1, VP9, Opus, etc.
//!
//! Reference: E-RTMP v2 specification

use std::fmt;

/// A four-character ASCII code identifying a codec or format.
///
/// FOURCC values are stored as big-endian u32 (e.g., "av01" = 0x61763031).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct FourCC([u8; 4]);

impl FourCC {
    /// Create a FOURCC from 4 ASCII bytes.
    pub const fn new(bytes: [u8; 4]) -> Self {
        Self(bytes)
    }

    /// Create a FOURCC from a 4-character string.
    ///
    /// Returns None if the string is not exactly 4 ASCII characters.
    pub fn from_str(s: &str) -> Option<Self> {
        let bytes = s.as_bytes();
        if bytes.len() == 4 && bytes.iter().all(|b| b.is_ascii()) {
            Some(Self([bytes[0], bytes[1], bytes[2], bytes[3]]))
        } else {
            None
        }
    }

    /// Create a FOURCC from a big-endian u32.
    pub const fn from_u32(value: u32) -> Self {
        Self(value.to_be_bytes())
    }

    /// Convert to big-endian u32 (for AMF encoding).
    pub const fn as_u32(&self) -> u32 {
        u32::from_be_bytes(self.0)
    }

    /// Get the raw bytes.
    pub const fn as_bytes(&self) -> &[u8; 4] {
        &self.0
    }

    /// Convert to string slice (panics if not valid UTF-8, which is fine for ASCII).
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or("????")
    }
}

impl fmt::Debug for FourCC {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FourCC(\"{}\")", self.as_str())
    }
}

impl fmt::Display for FourCC {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Video codec FOURCC values defined by E-RTMP.
///
/// These are used in the enhanced video pipeline when `isExVideoHeader` is set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VideoFourCc {
    /// H.264/AVC ("avc1")
    Avc,
    /// H.265/HEVC ("hvc1")
    Hevc,
    /// AV1 ("av01")
    Av1,
    /// VP9 ("vp09")
    Vp9,
    /// VP8 ("vp08")
    Vp8,
}

impl VideoFourCc {
    /// FOURCC for H.264/AVC
    pub const AVC_FOURCC: FourCC = FourCC::new(*b"avc1");
    /// FOURCC for H.265/HEVC
    pub const HEVC_FOURCC: FourCC = FourCC::new(*b"hvc1");
    /// FOURCC for AV1
    pub const AV1_FOURCC: FourCC = FourCC::new(*b"av01");
    /// FOURCC for VP9
    pub const VP9_FOURCC: FourCC = FourCC::new(*b"vp09");
    /// FOURCC for VP8
    pub const VP8_FOURCC: FourCC = FourCC::new(*b"vp08");

    /// Get the FOURCC for this codec.
    pub const fn fourcc(&self) -> FourCC {
        match self {
            VideoFourCc::Avc => Self::AVC_FOURCC,
            VideoFourCc::Hevc => Self::HEVC_FOURCC,
            VideoFourCc::Av1 => Self::AV1_FOURCC,
            VideoFourCc::Vp9 => Self::VP9_FOURCC,
            VideoFourCc::Vp8 => Self::VP8_FOURCC,
        }
    }

    /// Parse from a FOURCC value.
    pub fn from_fourcc(fourcc: FourCC) -> Option<Self> {
        match fourcc.as_bytes() {
            b"avc1" => Some(VideoFourCc::Avc),
            b"hvc1" => Some(VideoFourCc::Hevc),
            b"av01" => Some(VideoFourCc::Av1),
            b"vp09" => Some(VideoFourCc::Vp9),
            b"vp08" => Some(VideoFourCc::Vp8),
            _ => None,
        }
    }

    /// Parse from raw bytes (must be exactly 4 bytes).
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() >= 4 {
            let fourcc = FourCC::new([bytes[0], bytes[1], bytes[2], bytes[3]]);
            Self::from_fourcc(fourcc)
        } else {
            None
        }
    }

    /// Parse from a u32 value (big-endian FOURCC encoding).
    pub fn from_u32(value: u32) -> Option<Self> {
        Self::from_fourcc(FourCC::from_u32(value))
    }

    /// Get the codec name as a string.
    pub const fn name(&self) -> &'static str {
        match self {
            VideoFourCc::Avc => "H.264/AVC",
            VideoFourCc::Hevc => "H.265/HEVC",
            VideoFourCc::Av1 => "AV1",
            VideoFourCc::Vp9 => "VP9",
            VideoFourCc::Vp8 => "VP8",
        }
    }
}

impl fmt::Display for VideoFourCc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Audio codec FOURCC values defined by E-RTMP.
///
/// These are used in the enhanced audio pipeline when `SoundFormat` is `ExHeader` (9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AudioFourCc {
    /// AAC ("mp4a")
    Aac,
    /// Opus ("Opus") - note capital O
    Opus,
    /// FLAC ("fLaC")
    Flac,
    /// AC-3 / Dolby Digital ("ac-3")
    Ac3,
    /// E-AC-3 / Dolby Digital Plus ("ec-3")
    Eac3,
    /// MP3 (".mp3")
    Mp3,
}

impl AudioFourCc {
    /// FOURCC for AAC
    pub const AAC_FOURCC: FourCC = FourCC::new(*b"mp4a");
    /// FOURCC for Opus
    pub const OPUS_FOURCC: FourCC = FourCC::new(*b"Opus");
    /// FOURCC for FLAC
    pub const FLAC_FOURCC: FourCC = FourCC::new(*b"fLaC");
    /// FOURCC for AC-3
    pub const AC3_FOURCC: FourCC = FourCC::new(*b"ac-3");
    /// FOURCC for E-AC-3
    pub const EAC3_FOURCC: FourCC = FourCC::new(*b"ec-3");
    /// FOURCC for MP3
    pub const MP3_FOURCC: FourCC = FourCC::new(*b".mp3");

    /// Get the FOURCC for this codec.
    pub const fn fourcc(&self) -> FourCC {
        match self {
            AudioFourCc::Aac => Self::AAC_FOURCC,
            AudioFourCc::Opus => Self::OPUS_FOURCC,
            AudioFourCc::Flac => Self::FLAC_FOURCC,
            AudioFourCc::Ac3 => Self::AC3_FOURCC,
            AudioFourCc::Eac3 => Self::EAC3_FOURCC,
            AudioFourCc::Mp3 => Self::MP3_FOURCC,
        }
    }

    /// Parse from a FOURCC value.
    pub fn from_fourcc(fourcc: FourCC) -> Option<Self> {
        match fourcc.as_bytes() {
            b"mp4a" => Some(AudioFourCc::Aac),
            b"Opus" => Some(AudioFourCc::Opus),
            b"fLaC" => Some(AudioFourCc::Flac),
            b"ac-3" => Some(AudioFourCc::Ac3),
            b"ec-3" => Some(AudioFourCc::Eac3),
            b".mp3" => Some(AudioFourCc::Mp3),
            _ => None,
        }
    }

    /// Parse from raw bytes (must be at least 4 bytes).
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() >= 4 {
            let fourcc = FourCC::new([bytes[0], bytes[1], bytes[2], bytes[3]]);
            Self::from_fourcc(fourcc)
        } else {
            None
        }
    }

    /// Parse from a u32 value (big-endian FOURCC encoding).
    pub fn from_u32(value: u32) -> Option<Self> {
        Self::from_fourcc(FourCC::from_u32(value))
    }

    /// Get the codec name as a string.
    pub const fn name(&self) -> &'static str {
        match self {
            AudioFourCc::Aac => "AAC",
            AudioFourCc::Opus => "Opus",
            AudioFourCc::Flac => "FLAC",
            AudioFourCc::Ac3 => "AC-3",
            AudioFourCc::Eac3 => "E-AC-3",
            AudioFourCc::Mp3 => "MP3",
        }
    }
}

impl fmt::Display for AudioFourCc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fourcc_from_str() {
        let fourcc = FourCC::from_str("avc1").unwrap();
        assert_eq!(fourcc.as_str(), "avc1");
        assert_eq!(fourcc.as_bytes(), b"avc1");

        // Too short
        assert!(FourCC::from_str("avc").is_none());
        // Too long
        assert!(FourCC::from_str("avc12").is_none());
        // Non-ASCII
        assert!(FourCC::from_str("avc\u{00e9}").is_none());
    }

    #[test]
    fn test_fourcc_u32_conversion() {
        let fourcc = FourCC::from_str("av01").unwrap();
        // "av01" = 0x61 0x76 0x30 0x31 = 0x61763031
        assert_eq!(fourcc.as_u32(), 0x61763031);

        let fourcc2 = FourCC::from_u32(0x61763031);
        assert_eq!(fourcc2.as_str(), "av01");
        assert_eq!(fourcc, fourcc2);
    }

    #[test]
    fn test_fourcc_debug_display() {
        let fourcc = FourCC::from_str("hvc1").unwrap();
        assert_eq!(format!("{:?}", fourcc), "FourCC(\"hvc1\")");
        assert_eq!(format!("{}", fourcc), "hvc1");
    }

    #[test]
    fn test_video_fourcc_values() {
        assert_eq!(VideoFourCc::Avc.fourcc().as_str(), "avc1");
        assert_eq!(VideoFourCc::Hevc.fourcc().as_str(), "hvc1");
        assert_eq!(VideoFourCc::Av1.fourcc().as_str(), "av01");
        assert_eq!(VideoFourCc::Vp9.fourcc().as_str(), "vp09");
        assert_eq!(VideoFourCc::Vp8.fourcc().as_str(), "vp08");
    }

    #[test]
    fn test_video_fourcc_parsing() {
        assert_eq!(
            VideoFourCc::from_fourcc(FourCC::from_str("avc1").unwrap()),
            Some(VideoFourCc::Avc)
        );
        assert_eq!(
            VideoFourCc::from_fourcc(FourCC::from_str("hvc1").unwrap()),
            Some(VideoFourCc::Hevc)
        );
        assert_eq!(
            VideoFourCc::from_fourcc(FourCC::from_str("av01").unwrap()),
            Some(VideoFourCc::Av1)
        );
        assert_eq!(
            VideoFourCc::from_fourcc(FourCC::from_str("vp09").unwrap()),
            Some(VideoFourCc::Vp9)
        );
        assert_eq!(
            VideoFourCc::from_fourcc(FourCC::from_str("vp08").unwrap()),
            Some(VideoFourCc::Vp8)
        );

        // Unknown codec
        assert_eq!(
            VideoFourCc::from_fourcc(FourCC::from_str("xxxx").unwrap()),
            None
        );
    }

    #[test]
    fn test_video_fourcc_from_bytes() {
        assert_eq!(VideoFourCc::from_bytes(b"avc1"), Some(VideoFourCc::Avc));
        assert_eq!(
            VideoFourCc::from_bytes(b"hvc1extra"),
            Some(VideoFourCc::Hevc)
        );
        assert_eq!(VideoFourCc::from_bytes(b"av"), None); // Too short
    }

    #[test]
    fn test_video_fourcc_from_u32() {
        // "avc1" = 0x61766331
        assert_eq!(VideoFourCc::from_u32(0x61766331), Some(VideoFourCc::Avc));
        // "av01" = 0x61763031
        assert_eq!(VideoFourCc::from_u32(0x61763031), Some(VideoFourCc::Av1));
    }

    #[test]
    fn test_video_fourcc_name() {
        assert_eq!(VideoFourCc::Avc.name(), "H.264/AVC");
        assert_eq!(VideoFourCc::Hevc.name(), "H.265/HEVC");
        assert_eq!(VideoFourCc::Av1.name(), "AV1");
        assert_eq!(VideoFourCc::Vp9.name(), "VP9");
        assert_eq!(VideoFourCc::Vp8.name(), "VP8");
    }

    #[test]
    fn test_audio_fourcc_values() {
        assert_eq!(AudioFourCc::Aac.fourcc().as_str(), "mp4a");
        assert_eq!(AudioFourCc::Opus.fourcc().as_str(), "Opus");
        assert_eq!(AudioFourCc::Flac.fourcc().as_str(), "fLaC");
        assert_eq!(AudioFourCc::Ac3.fourcc().as_str(), "ac-3");
        assert_eq!(AudioFourCc::Eac3.fourcc().as_str(), "ec-3");
        assert_eq!(AudioFourCc::Mp3.fourcc().as_str(), ".mp3");
    }

    #[test]
    fn test_audio_fourcc_parsing() {
        assert_eq!(
            AudioFourCc::from_fourcc(FourCC::from_str("mp4a").unwrap()),
            Some(AudioFourCc::Aac)
        );
        assert_eq!(
            AudioFourCc::from_fourcc(FourCC::from_str("Opus").unwrap()),
            Some(AudioFourCc::Opus)
        );
        assert_eq!(
            AudioFourCc::from_fourcc(FourCC::from_str("fLaC").unwrap()),
            Some(AudioFourCc::Flac)
        );
        assert_eq!(
            AudioFourCc::from_fourcc(FourCC::from_str("ac-3").unwrap()),
            Some(AudioFourCc::Ac3)
        );
        assert_eq!(
            AudioFourCc::from_fourcc(FourCC::from_str("ec-3").unwrap()),
            Some(AudioFourCc::Eac3)
        );
        assert_eq!(
            AudioFourCc::from_fourcc(FourCC::from_str(".mp3").unwrap()),
            Some(AudioFourCc::Mp3)
        );

        // Unknown codec
        assert_eq!(
            AudioFourCc::from_fourcc(FourCC::from_str("xxxx").unwrap()),
            None
        );
    }

    #[test]
    fn test_audio_fourcc_case_sensitivity() {
        // Opus has capital O
        assert_eq!(
            AudioFourCc::from_fourcc(FourCC::from_str("Opus").unwrap()),
            Some(AudioFourCc::Opus)
        );
        // lowercase should not match
        assert_eq!(
            AudioFourCc::from_fourcc(FourCC::from_str("opus").unwrap()),
            None
        );
    }

    #[test]
    fn test_audio_fourcc_from_bytes() {
        assert_eq!(AudioFourCc::from_bytes(b"mp4a"), Some(AudioFourCc::Aac));
        assert_eq!(AudioFourCc::from_bytes(b"Opus"), Some(AudioFourCc::Opus));
        assert_eq!(AudioFourCc::from_bytes(b"mp"), None); // Too short
    }

    #[test]
    fn test_audio_fourcc_from_u32() {
        // "mp4a" = 0x6d703461
        assert_eq!(AudioFourCc::from_u32(0x6d703461), Some(AudioFourCc::Aac));
        // "Opus" = 0x4f707573
        assert_eq!(AudioFourCc::from_u32(0x4f707573), Some(AudioFourCc::Opus));
    }

    #[test]
    fn test_audio_fourcc_name() {
        assert_eq!(AudioFourCc::Aac.name(), "AAC");
        assert_eq!(AudioFourCc::Opus.name(), "Opus");
        assert_eq!(AudioFourCc::Flac.name(), "FLAC");
        assert_eq!(AudioFourCc::Ac3.name(), "AC-3");
        assert_eq!(AudioFourCc::Eac3.name(), "E-AC-3");
        assert_eq!(AudioFourCc::Mp3.name(), "MP3");
    }

    #[test]
    fn test_fourcc_equality() {
        let a = FourCC::from_str("avc1").unwrap();
        let b = FourCC::new(*b"avc1");
        let c = FourCC::from_u32(0x61766331);

        assert_eq!(a, b);
        assert_eq!(b, c);
        assert_eq!(a, c);
    }

    #[test]
    fn test_fourcc_hash() {
        use std::collections::HashSet;

        let mut set = HashSet::new();
        set.insert(VideoFourCc::Avc);
        set.insert(VideoFourCc::Hevc);
        set.insert(VideoFourCc::Avc); // duplicate

        assert_eq!(set.len(), 2);
        assert!(set.contains(&VideoFourCc::Avc));
        assert!(set.contains(&VideoFourCc::Hevc));
    }

    #[test]
    fn test_video_fourcc_display() {
        assert_eq!(format!("{}", VideoFourCc::Avc), "H.264/AVC");
        assert_eq!(format!("{}", VideoFourCc::Hevc), "H.265/HEVC");
    }

    #[test]
    fn test_audio_fourcc_display() {
        assert_eq!(format!("{}", AudioFourCc::Aac), "AAC");
        assert_eq!(format!("{}", AudioFourCc::Opus), "Opus");
    }
}
