//! FLV tag parsing
//!
//! FLV (Flash Video) is the container format used by RTMP for audio/video data.
//! Each RTMP audio/video message is essentially an FLV tag without the tag header.
//!
//! FLV Tag Structure (for reference, RTMP messages don't include this header):
//! ```text
//! +--------+-------------+-----------+
//! | Type(1)| DataSize(3) | TS(3+1)   | StreamID(3) | Data(N) |
//! +--------+-------------+-----------+
//! ```
//!
//! RTMP Video Data:
//! ```text
//! +----------+----------+
//! | FrameType| CodecID  | CodecData...
//! | (4 bits) | (4 bits) |
//! +----------+----------+
//! ```
//!
//! RTMP Audio Data:
//! ```text
//! +----------+----------+----------+----------+
//! |SoundFormat|SoundRate|SoundSize |SoundType | AudioData...
//! | (4 bits)  | (2 bits)| (1 bit)  | (1 bit)  |
//! +----------+----------+----------+----------+
//! ```

use bytes::Bytes;


/// FLV tag type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlvTagType {
    Audio,
    Video,
    Script,
}

/// Parsed FLV tag
#[derive(Debug, Clone)]
pub struct FlvTag {
    /// Tag type
    pub tag_type: FlvTagType,
    /// Timestamp in milliseconds
    pub timestamp: u32,
    /// Raw tag data (including codec headers)
    pub data: Bytes,
}

/// Video frame type (upper 4 bits of first byte)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoFrameType {
    /// Keyframe (for AVC, a seekable frame)
    Keyframe = 1,
    /// Inter frame (for AVC, a non-seekable frame)
    InterFrame = 2,
    /// Disposable inter frame (H.263 only)
    DisposableInterFrame = 3,
    /// Generated keyframe (reserved for server use)
    GeneratedKeyframe = 4,
    /// Video info/command frame
    VideoInfoFrame = 5,
}

impl VideoFrameType {
    pub fn from_byte(b: u8) -> Option<Self> {
        match (b >> 4) & 0x0F {
            1 => Some(VideoFrameType::Keyframe),
            2 => Some(VideoFrameType::InterFrame),
            3 => Some(VideoFrameType::DisposableInterFrame),
            4 => Some(VideoFrameType::GeneratedKeyframe),
            5 => Some(VideoFrameType::VideoInfoFrame),
            _ => None,
        }
    }

    pub fn is_keyframe(&self) -> bool {
        matches!(self, VideoFrameType::Keyframe | VideoFrameType::GeneratedKeyframe)
    }
}

/// Video codec ID (lower 4 bits of first byte)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoCodec {
    /// Sorenson H.263
    SorensonH263 = 2,
    /// Screen video
    ScreenVideo = 3,
    /// VP6
    Vp6 = 4,
    /// VP6 with alpha
    Vp6Alpha = 5,
    /// Screen video v2
    ScreenVideoV2 = 6,
    /// AVC (H.264)
    Avc = 7,
    /// HEVC (H.265) - enhanced RTMP extension
    Hevc = 12,
    /// AV1 - enhanced RTMP extension
    Av1 = 13,
}

impl VideoCodec {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b & 0x0F {
            2 => Some(VideoCodec::SorensonH263),
            3 => Some(VideoCodec::ScreenVideo),
            4 => Some(VideoCodec::Vp6),
            5 => Some(VideoCodec::Vp6Alpha),
            6 => Some(VideoCodec::ScreenVideoV2),
            7 => Some(VideoCodec::Avc),
            12 => Some(VideoCodec::Hevc),
            13 => Some(VideoCodec::Av1),
            _ => None,
        }
    }
}

/// Audio format (upper 4 bits of first byte)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    /// Linear PCM, platform endian
    LinearPcmPlatform = 0,
    /// ADPCM
    Adpcm = 1,
    /// MP3
    Mp3 = 2,
    /// Linear PCM, little endian
    LinearPcmLe = 3,
    /// Nellymoser 16kHz mono
    Nellymoser16kMono = 4,
    /// Nellymoser 8kHz mono
    Nellymoser8kMono = 5,
    /// Nellymoser
    Nellymoser = 6,
    /// G.711 A-law
    G711ALaw = 7,
    /// G.711 mu-law
    G711MuLaw = 8,
    /// AAC
    Aac = 10,
    /// Speex
    Speex = 11,
    /// MP3 8kHz
    Mp38k = 14,
    /// Device-specific sound
    DeviceSpecific = 15,
}

impl AudioFormat {
    pub fn from_byte(b: u8) -> Option<Self> {
        match (b >> 4) & 0x0F {
            0 => Some(AudioFormat::LinearPcmPlatform),
            1 => Some(AudioFormat::Adpcm),
            2 => Some(AudioFormat::Mp3),
            3 => Some(AudioFormat::LinearPcmLe),
            4 => Some(AudioFormat::Nellymoser16kMono),
            5 => Some(AudioFormat::Nellymoser8kMono),
            6 => Some(AudioFormat::Nellymoser),
            7 => Some(AudioFormat::G711ALaw),
            8 => Some(AudioFormat::G711MuLaw),
            10 => Some(AudioFormat::Aac),
            11 => Some(AudioFormat::Speex),
            14 => Some(AudioFormat::Mp38k),
            15 => Some(AudioFormat::DeviceSpecific),
            _ => None,
        }
    }
}

/// Audio sample rate
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioSampleRate {
    Rate5512 = 0,
    Rate11025 = 1,
    Rate22050 = 2,
    Rate44100 = 3,
}

impl AudioSampleRate {
    pub fn from_byte(b: u8) -> Self {
        match (b >> 2) & 0x03 {
            0 => AudioSampleRate::Rate5512,
            1 => AudioSampleRate::Rate11025,
            2 => AudioSampleRate::Rate22050,
            _ => AudioSampleRate::Rate44100,
        }
    }

    pub fn to_hz(&self) -> u32 {
        match self {
            AudioSampleRate::Rate5512 => 5512,
            AudioSampleRate::Rate11025 => 11025,
            AudioSampleRate::Rate22050 => 22050,
            AudioSampleRate::Rate44100 => 44100,
        }
    }
}

impl FlvTag {
    /// Create a new video tag
    pub fn video(timestamp: u32, data: Bytes) -> Self {
        Self {
            tag_type: FlvTagType::Video,
            timestamp,
            data,
        }
    }

    /// Create a new audio tag
    pub fn audio(timestamp: u32, data: Bytes) -> Self {
        Self {
            tag_type: FlvTagType::Audio,
            timestamp,
            data,
        }
    }

    /// Check if this is a video tag
    pub fn is_video(&self) -> bool {
        self.tag_type == FlvTagType::Video
    }

    /// Check if this is an audio tag
    pub fn is_audio(&self) -> bool {
        self.tag_type == FlvTagType::Audio
    }

    /// For video tags, get the frame type
    pub fn video_frame_type(&self) -> Option<VideoFrameType> {
        if self.is_video() && !self.data.is_empty() {
            VideoFrameType::from_byte(self.data[0])
        } else {
            None
        }
    }

    /// For video tags, get the codec
    pub fn video_codec(&self) -> Option<VideoCodec> {
        if self.is_video() && !self.data.is_empty() {
            VideoCodec::from_byte(self.data[0])
        } else {
            None
        }
    }

    /// For audio tags, get the format
    pub fn audio_format(&self) -> Option<AudioFormat> {
        if self.is_audio() && !self.data.is_empty() {
            AudioFormat::from_byte(self.data[0])
        } else {
            None
        }
    }

    /// Check if this is a keyframe
    pub fn is_keyframe(&self) -> bool {
        self.video_frame_type()
            .map(|ft| ft.is_keyframe())
            .unwrap_or(false)
    }

    /// Check if this is an AVC sequence header
    pub fn is_avc_sequence_header(&self) -> bool {
        if self.is_video() && self.data.len() >= 2 {
            let codec = VideoCodec::from_byte(self.data[0]);
            codec == Some(VideoCodec::Avc) && self.data[1] == 0
        } else {
            false
        }
    }

    /// Check if this is an AAC sequence header
    pub fn is_aac_sequence_header(&self) -> bool {
        if self.is_audio() && self.data.len() >= 2 {
            let format = AudioFormat::from_byte(self.data[0]);
            format == Some(AudioFormat::Aac) && self.data[1] == 0
        } else {
            false
        }
    }

    /// Get the size of the tag data
    pub fn size(&self) -> usize {
        self.data.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_video_frame_type() {
        // Keyframe + AVC
        assert_eq!(VideoFrameType::from_byte(0x17), Some(VideoFrameType::Keyframe));
        assert_eq!(VideoCodec::from_byte(0x17), Some(VideoCodec::Avc));

        // Inter frame + AVC
        assert_eq!(VideoFrameType::from_byte(0x27), Some(VideoFrameType::InterFrame));
    }

    #[test]
    fn test_avc_sequence_header() {
        let header = FlvTag::video(0, Bytes::from_static(&[0x17, 0x00, 0x00, 0x00, 0x00]));
        assert!(header.is_avc_sequence_header());
        assert!(header.is_keyframe());

        let frame = FlvTag::video(0, Bytes::from_static(&[0x17, 0x01, 0x00, 0x00, 0x00]));
        assert!(!frame.is_avc_sequence_header());
    }

    #[test]
    fn test_aac_sequence_header() {
        let header = FlvTag::audio(0, Bytes::from_static(&[0xAF, 0x00, 0x12, 0x10]));
        assert!(header.is_aac_sequence_header());

        let frame = FlvTag::audio(0, Bytes::from_static(&[0xAF, 0x01, 0x21, 0x00]));
        assert!(!frame.is_aac_sequence_header());
    }
}
