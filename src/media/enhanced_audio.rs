//! Enhanced RTMP audio parsing
//!
//! This module handles the Enhanced RTMP audio format which uses FOURCC
//! codec signaling and supports modern codecs like Opus, FLAC, and AC-3.
//!
//! Enhanced audio is signaled by SoundFormat=9 (ExHeader) in the FLV audio tag.
//!
//! Reference: E-RTMP v2 specification - "Enhancing Audio"

use bytes::{Buf, Bytes};

use crate::error::{MediaError, Result};
use crate::media::fourcc::AudioFourCc;

/// Sound format value that signals enhanced audio mode.
pub const SOUND_FORMAT_EX_HEADER: u8 = 9;

/// Enhanced audio packet type (lower 4 bits when SoundFormat=ExHeader).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AudioPacketType {
    /// Sequence header (codec configuration)
    SequenceStart = 0,
    /// Coded audio frames
    CodedFrames = 1,
    /// End of sequence
    SequenceEnd = 2,
    // 3 is reserved
    /// Multichannel configuration
    MultichannelConfig = 4,
    /// Multitrack audio
    Multitrack = 5,
    // 6 is reserved
    /// ModEx signal (extensions follow)
    ModEx = 7,
}

impl AudioPacketType {
    /// Parse from the lower 4 bits of the audio packet type byte.
    pub fn from_byte(b: u8) -> Option<Self> {
        match b & 0x0F {
            0 => Some(AudioPacketType::SequenceStart),
            1 => Some(AudioPacketType::CodedFrames),
            2 => Some(AudioPacketType::SequenceEnd),
            4 => Some(AudioPacketType::MultichannelConfig),
            5 => Some(AudioPacketType::Multitrack),
            7 => Some(AudioPacketType::ModEx),
            _ => None, // 3, 6, 8-15 are reserved
        }
    }
}

/// Audio channel ordering scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AudioChannelOrder {
    /// Channel order unspecified
    Unspecified = 0,
    /// Native channel order (codec-specific)
    Native = 1,
    /// Custom channel order with explicit mapping
    Custom = 2,
}

impl AudioChannelOrder {
    /// Parse from byte value.
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0 => Some(AudioChannelOrder::Unspecified),
            1 => Some(AudioChannelOrder::Native),
            2 => Some(AudioChannelOrder::Custom),
            _ => None,
        }
    }
}

/// Multitrack type for audio (same as video).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AudioMultitrackType {
    /// Single track with track ID
    OneTrack = 0,
    /// Multiple tracks, same codec
    ManyTracks = 1,
    /// Multiple tracks, different codecs
    ManyTracksManyCodecs = 2,
}

impl AudioMultitrackType {
    /// Parse from byte value.
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0 => Some(AudioMultitrackType::OneTrack),
            1 => Some(AudioMultitrackType::ManyTracks),
            2 => Some(AudioMultitrackType::ManyTracksManyCodecs),
            _ => None,
        }
    }
}

/// Parsed enhanced audio data.
#[derive(Debug, Clone)]
pub enum EnhancedAudioData {
    /// Sequence header (codec configuration).
    SequenceHeader {
        /// Audio codec
        codec: AudioFourCc,
        /// Codec-specific configuration data
        config: Bytes,
    },

    /// Coded audio frame.
    Frame {
        /// Audio codec
        codec: AudioFourCc,
        /// Frame data
        data: Bytes,
    },

    /// End of sequence marker.
    SequenceEnd {
        /// Audio codec
        codec: AudioFourCc,
    },

    /// Multichannel configuration.
    MultichannelConfig {
        /// Audio codec
        codec: AudioFourCc,
        /// Channel ordering scheme
        channel_order: AudioChannelOrder,
        /// Number of channels
        channel_count: u8,
        /// Channel mapping (if Custom order)
        channel_mapping: Option<Bytes>,
    },

    /// Multitrack audio container.
    Multitrack {
        /// Multitrack type
        multitrack_type: AudioMultitrackType,
        /// Individual tracks
        tracks: Vec<AudioTrack>,
    },
}

/// A single audio track within a multitrack container.
#[derive(Debug, Clone)]
pub struct AudioTrack {
    /// Track ID (0-255)
    pub track_id: u8,
    /// Audio codec for this track
    pub codec: AudioFourCc,
    /// Track data
    pub data: EnhancedAudioTrackData,
}

/// Data within an audio track.
#[derive(Debug, Clone)]
pub enum EnhancedAudioTrackData {
    /// Sequence header for this track
    SequenceHeader { config: Bytes },
    /// Coded frame for this track
    Frame { data: Bytes },
    /// Sequence end for this track
    SequenceEnd,
}

impl EnhancedAudioData {
    /// Check if audio data uses enhanced format (SoundFormat=9).
    ///
    /// The first byte of FLV audio tag has SoundFormat in upper 4 bits.
    #[inline]
    pub fn is_enhanced(first_byte: u8) -> bool {
        (first_byte >> 4) == SOUND_FORMAT_EX_HEADER
    }

    /// Parse enhanced audio from RTMP audio message payload.
    ///
    /// The payload should start with the FLV audio tag header byte.
    pub fn parse(data: Bytes) -> Result<Self> {
        if data.is_empty() {
            return Err(MediaError::InvalidEnhancedAudioPacket.into());
        }

        let first_byte = data[0];

        // Verify this is enhanced audio (SoundFormat=9)
        if !Self::is_enhanced(first_byte) {
            return Err(MediaError::InvalidEnhancedAudioPacket.into());
        }

        let packet_type =
            AudioPacketType::from_byte(first_byte).ok_or(MediaError::InvalidEnhancedAudioPacket)?;

        let mut cursor = data.slice(1..);

        // Handle packet types
        match packet_type {
            AudioPacketType::ModEx => Self::parse_with_modex(cursor),
            AudioPacketType::Multitrack => Self::parse_multitrack(cursor),
            AudioPacketType::MultichannelConfig => Self::parse_multichannel_config(cursor),
            _ => {
                // Regular enhanced audio: FOURCC follows
                if cursor.len() < 4 {
                    return Err(MediaError::InvalidEnhancedAudioPacket.into());
                }

                let fourcc_bytes = [cursor[0], cursor[1], cursor[2], cursor[3]];
                let codec = AudioFourCc::from_bytes(&fourcc_bytes)
                    .ok_or(MediaError::UnsupportedAudioCodec)?;
                cursor.advance(4);

                Self::parse_by_packet_type(packet_type, codec, cursor)
            }
        }
    }

    /// Parse with ModEx prefix handling.
    fn parse_with_modex(mut data: Bytes) -> Result<Self> {
        if data.is_empty() {
            return Err(MediaError::InvalidEnhancedAudioPacket.into());
        }

        let modex_type = data[0];
        data.advance(1);

        // TimestampOffsetNano (type 0) has 3 bytes of nanosecond offset
        if modex_type == 0 {
            if data.len() < 3 {
                return Err(MediaError::InvalidEnhancedAudioPacket.into());
            }
            data.advance(3);
        }

        // After ModEx, parse FOURCC
        if data.len() < 4 {
            return Err(MediaError::InvalidEnhancedAudioPacket.into());
        }

        let fourcc_bytes = [data[0], data[1], data[2], data[3]];
        if let Some(codec) = AudioFourCc::from_bytes(&fourcc_bytes) {
            data.advance(4);
            Ok(EnhancedAudioData::Frame { codec, data })
        } else {
            Err(MediaError::InvalidEnhancedAudioPacket.into())
        }
    }

    /// Parse multichannel configuration.
    fn parse_multichannel_config(mut data: Bytes) -> Result<Self> {
        if data.len() < 6 {
            // Need at least FOURCC (4) + channelOrder (1) + channelCount (1)
            return Err(MediaError::InvalidEnhancedAudioPacket.into());
        }

        let fourcc_bytes = [data[0], data[1], data[2], data[3]];
        let codec =
            AudioFourCc::from_bytes(&fourcc_bytes).ok_or(MediaError::UnsupportedAudioCodec)?;
        data.advance(4);

        let channel_order =
            AudioChannelOrder::from_byte(data[0]).ok_or(MediaError::InvalidEnhancedAudioPacket)?;
        data.advance(1);

        let channel_count = data[0];
        data.advance(1);

        let channel_mapping = if channel_order == AudioChannelOrder::Custom && !data.is_empty() {
            Some(data)
        } else {
            None
        };

        Ok(EnhancedAudioData::MultichannelConfig {
            codec,
            channel_order,
            channel_count,
            channel_mapping,
        })
    }

    /// Parse multitrack audio container.
    fn parse_multitrack(mut data: Bytes) -> Result<Self> {
        if data.is_empty() {
            return Err(MediaError::InvalidEnhancedAudioPacket.into());
        }

        let multitrack_byte = data[0];
        data.advance(1);

        let multitrack_type = AudioMultitrackType::from_byte(multitrack_byte >> 4)
            .ok_or(MediaError::InvalidEnhancedAudioPacket)?;
        let packet_type = AudioPacketType::from_byte(multitrack_byte)
            .ok_or(MediaError::InvalidEnhancedAudioPacket)?;

        let mut tracks = Vec::new();

        match multitrack_type {
            AudioMultitrackType::OneTrack => {
                if data.len() < 5 {
                    return Err(MediaError::InvalidEnhancedAudioPacket.into());
                }
                let track_id = data[0];
                data.advance(1);

                let fourcc_bytes = [data[0], data[1], data[2], data[3]];
                let codec = AudioFourCc::from_bytes(&fourcc_bytes)
                    .ok_or(MediaError::UnsupportedAudioCodec)?;
                data.advance(4);

                let track_data = Self::parse_track_data(packet_type, data)?;
                tracks.push(AudioTrack {
                    track_id,
                    codec,
                    data: track_data,
                });
            }
            AudioMultitrackType::ManyTracks => {
                if data.len() < 4 {
                    return Err(MediaError::InvalidEnhancedAudioPacket.into());
                }
                let fourcc_bytes = [data[0], data[1], data[2], data[3]];
                let codec = AudioFourCc::from_bytes(&fourcc_bytes)
                    .ok_or(MediaError::UnsupportedAudioCodec)?;
                data.advance(4);

                while !data.is_empty() {
                    if data.len() < 4 {
                        break;
                    }
                    let track_id = data[0];
                    data.advance(1);

                    let track_size =
                        ((data[0] as usize) << 16) | ((data[1] as usize) << 8) | (data[2] as usize);
                    data.advance(3);

                    if data.len() < track_size {
                        break;
                    }
                    let track_bytes = data.slice(..track_size);
                    data.advance(track_size);

                    tracks.push(AudioTrack {
                        track_id,
                        codec,
                        data: EnhancedAudioTrackData::Frame { data: track_bytes },
                    });
                }
            }
            AudioMultitrackType::ManyTracksManyCodecs => {
                while !data.is_empty() {
                    if data.len() < 8 {
                        break;
                    }
                    let track_id = data[0];
                    data.advance(1);

                    let fourcc_bytes = [data[0], data[1], data[2], data[3]];
                    let codec = AudioFourCc::from_bytes(&fourcc_bytes)
                        .ok_or(MediaError::UnsupportedAudioCodec)?;
                    data.advance(4);

                    let track_size =
                        ((data[0] as usize) << 16) | ((data[1] as usize) << 8) | (data[2] as usize);
                    data.advance(3);

                    if data.len() < track_size {
                        break;
                    }
                    let track_bytes = data.slice(..track_size);
                    data.advance(track_size);

                    tracks.push(AudioTrack {
                        track_id,
                        codec,
                        data: EnhancedAudioTrackData::Frame { data: track_bytes },
                    });
                }
            }
        }

        Ok(EnhancedAudioData::Multitrack {
            multitrack_type,
            tracks,
        })
    }

    /// Parse track data based on packet type.
    fn parse_track_data(
        packet_type: AudioPacketType,
        data: Bytes,
    ) -> Result<EnhancedAudioTrackData> {
        match packet_type {
            AudioPacketType::SequenceStart => {
                Ok(EnhancedAudioTrackData::SequenceHeader { config: data })
            }
            AudioPacketType::SequenceEnd => Ok(EnhancedAudioTrackData::SequenceEnd),
            AudioPacketType::CodedFrames => Ok(EnhancedAudioTrackData::Frame { data }),
            _ => Err(MediaError::InvalidEnhancedAudioPacket.into()),
        }
    }

    /// Parse by specific packet type.
    fn parse_by_packet_type(
        packet_type: AudioPacketType,
        codec: AudioFourCc,
        data: Bytes,
    ) -> Result<Self> {
        match packet_type {
            AudioPacketType::SequenceStart => Ok(EnhancedAudioData::SequenceHeader {
                codec,
                config: data,
            }),
            AudioPacketType::SequenceEnd => Ok(EnhancedAudioData::SequenceEnd { codec }),
            AudioPacketType::CodedFrames => Ok(EnhancedAudioData::Frame { codec, data }),
            _ => Err(MediaError::InvalidEnhancedAudioPacket.into()),
        }
    }

    /// Check if this is a sequence header.
    pub fn is_sequence_header(&self) -> bool {
        matches!(self, EnhancedAudioData::SequenceHeader { .. })
    }

    /// Get the codec if available.
    pub fn codec(&self) -> Option<AudioFourCc> {
        match self {
            EnhancedAudioData::SequenceHeader { codec, .. } => Some(*codec),
            EnhancedAudioData::Frame { codec, .. } => Some(*codec),
            EnhancedAudioData::SequenceEnd { codec } => Some(*codec),
            EnhancedAudioData::MultichannelConfig { codec, .. } => Some(*codec),
            EnhancedAudioData::Multitrack { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_enhanced() {
        // SoundFormat=9 in upper 4 bits
        assert!(EnhancedAudioData::is_enhanced(0x90)); // 9 << 4
        assert!(EnhancedAudioData::is_enhanced(0x91)); // 9 << 4 | SequenceStart
        assert!(EnhancedAudioData::is_enhanced(0x9F)); // 9 << 4 | 0x0F

        // Other sound formats
        assert!(!EnhancedAudioData::is_enhanced(0xA0)); // 10 = AAC
        assert!(!EnhancedAudioData::is_enhanced(0x00)); // 0 = PCM
        assert!(!EnhancedAudioData::is_enhanced(0x20)); // 2 = MP3
    }

    #[test]
    fn test_audio_packet_type_parsing() {
        assert_eq!(
            AudioPacketType::from_byte(0x90),
            Some(AudioPacketType::SequenceStart)
        );
        assert_eq!(
            AudioPacketType::from_byte(0x91),
            Some(AudioPacketType::CodedFrames)
        );
        assert_eq!(
            AudioPacketType::from_byte(0x92),
            Some(AudioPacketType::SequenceEnd)
        );
        assert_eq!(AudioPacketType::from_byte(0x93), None); // Reserved
        assert_eq!(
            AudioPacketType::from_byte(0x94),
            Some(AudioPacketType::MultichannelConfig)
        );
        assert_eq!(
            AudioPacketType::from_byte(0x95),
            Some(AudioPacketType::Multitrack)
        );
        assert_eq!(AudioPacketType::from_byte(0x96), None); // Reserved
        assert_eq!(
            AudioPacketType::from_byte(0x97),
            Some(AudioPacketType::ModEx)
        );
    }

    #[test]
    fn test_audio_channel_order_parsing() {
        assert_eq!(
            AudioChannelOrder::from_byte(0),
            Some(AudioChannelOrder::Unspecified)
        );
        assert_eq!(
            AudioChannelOrder::from_byte(1),
            Some(AudioChannelOrder::Native)
        );
        assert_eq!(
            AudioChannelOrder::from_byte(2),
            Some(AudioChannelOrder::Custom)
        );
        assert_eq!(AudioChannelOrder::from_byte(3), None);
    }

    #[test]
    fn test_audio_multitrack_type_parsing() {
        assert_eq!(
            AudioMultitrackType::from_byte(0),
            Some(AudioMultitrackType::OneTrack)
        );
        assert_eq!(
            AudioMultitrackType::from_byte(1),
            Some(AudioMultitrackType::ManyTracks)
        );
        assert_eq!(
            AudioMultitrackType::from_byte(2),
            Some(AudioMultitrackType::ManyTracksManyCodecs)
        );
        assert_eq!(AudioMultitrackType::from_byte(3), None);
    }

    #[test]
    fn test_parse_sequence_header() {
        // Enhanced Opus sequence header
        // 0x90 = SoundFormat(9) | SequenceStart(0)
        let mut data = vec![0x90];
        data.extend_from_slice(b"Opus"); // FOURCC (capital O)
        data.extend_from_slice(&[0x01, 0x02, 0x03, 0x04]); // Config data

        let parsed = EnhancedAudioData::parse(Bytes::from(data)).unwrap();

        match parsed {
            EnhancedAudioData::SequenceHeader { codec, config } => {
                assert_eq!(codec, AudioFourCc::Opus);
                assert_eq!(config.as_ref(), &[0x01, 0x02, 0x03, 0x04]);
            }
            _ => panic!("Expected SequenceHeader"),
        }
    }

    #[test]
    fn test_parse_coded_frames() {
        // Enhanced AAC frame
        // 0x91 = SoundFormat(9) | CodedFrames(1)
        let mut data = vec![0x91];
        data.extend_from_slice(b"mp4a"); // FOURCC
        data.extend_from_slice(&[0xAA, 0xBB, 0xCC]); // Frame data

        let parsed = EnhancedAudioData::parse(Bytes::from(data)).unwrap();

        match parsed {
            EnhancedAudioData::Frame { codec, data } => {
                assert_eq!(codec, AudioFourCc::Aac);
                assert_eq!(data.as_ref(), &[0xAA, 0xBB, 0xCC]);
            }
            _ => panic!("Expected Frame"),
        }
    }

    #[test]
    fn test_parse_sequence_end() {
        // 0x92 = SoundFormat(9) | SequenceEnd(2)
        let mut data = vec![0x92];
        data.extend_from_slice(b"fLaC"); // FLAC FOURCC

        let parsed = EnhancedAudioData::parse(Bytes::from(data)).unwrap();

        match parsed {
            EnhancedAudioData::SequenceEnd { codec } => {
                assert_eq!(codec, AudioFourCc::Flac);
            }
            _ => panic!("Expected SequenceEnd"),
        }
    }

    #[test]
    fn test_parse_multichannel_config() {
        // 0x94 = SoundFormat(9) | MultichannelConfig(4)
        let mut data = vec![0x94];
        data.extend_from_slice(b"Opus"); // FOURCC
        data.push(0x01); // Native channel order
        data.push(0x06); // 6 channels (5.1)

        let parsed = EnhancedAudioData::parse(Bytes::from(data)).unwrap();

        match parsed {
            EnhancedAudioData::MultichannelConfig {
                codec,
                channel_order,
                channel_count,
                channel_mapping,
            } => {
                assert_eq!(codec, AudioFourCc::Opus);
                assert_eq!(channel_order, AudioChannelOrder::Native);
                assert_eq!(channel_count, 6);
                assert!(channel_mapping.is_none());
            }
            _ => panic!("Expected MultichannelConfig"),
        }
    }

    #[test]
    fn test_parse_multichannel_config_custom() {
        // Custom channel order with mapping
        let mut data = vec![0x94];
        data.extend_from_slice(b"Opus");
        data.push(0x02); // Custom channel order
        data.push(0x02); // 2 channels
        data.extend_from_slice(&[0x00, 0x01]); // Channel mapping

        let parsed = EnhancedAudioData::parse(Bytes::from(data)).unwrap();

        match parsed {
            EnhancedAudioData::MultichannelConfig {
                channel_order,
                channel_mapping,
                ..
            } => {
                assert_eq!(channel_order, AudioChannelOrder::Custom);
                assert!(channel_mapping.is_some());
                assert_eq!(channel_mapping.unwrap().as_ref(), &[0x00, 0x01]);
            }
            _ => panic!("Expected MultichannelConfig"),
        }
    }

    #[test]
    fn test_parse_error_empty() {
        let result = EnhancedAudioData::parse(Bytes::new());
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_error_not_enhanced() {
        // AAC format (SoundFormat=10)
        let data = Bytes::from_static(&[0xAF, 0x00, 0x12, 0x10]);
        let result = EnhancedAudioData::parse(data);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_error_no_fourcc() {
        // Enhanced header but no FOURCC
        let data = Bytes::from_static(&[0x90, 0x01, 0x02]);
        let result = EnhancedAudioData::parse(data);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_error_unknown_fourcc() {
        let mut data = vec![0x90];
        data.extend_from_slice(b"xxxx"); // Unknown codec
        let result = EnhancedAudioData::parse(Bytes::from(data));
        assert!(result.is_err());
    }

    #[test]
    fn test_is_sequence_header() {
        let header = EnhancedAudioData::SequenceHeader {
            codec: AudioFourCc::Opus,
            config: Bytes::new(),
        };
        assert!(header.is_sequence_header());

        let frame = EnhancedAudioData::Frame {
            codec: AudioFourCc::Opus,
            data: Bytes::new(),
        };
        assert!(!frame.is_sequence_header());
    }

    #[test]
    fn test_codec_accessor() {
        let header = EnhancedAudioData::SequenceHeader {
            codec: AudioFourCc::Flac,
            config: Bytes::new(),
        };
        assert_eq!(header.codec(), Some(AudioFourCc::Flac));

        let frame = EnhancedAudioData::Frame {
            codec: AudioFourCc::Ac3,
            data: Bytes::new(),
        };
        assert_eq!(frame.codec(), Some(AudioFourCc::Ac3));

        let multichannel = EnhancedAudioData::MultichannelConfig {
            codec: AudioFourCc::Opus,
            channel_order: AudioChannelOrder::Native,
            channel_count: 2,
            channel_mapping: None,
        };
        assert_eq!(multichannel.codec(), Some(AudioFourCc::Opus));

        let multitrack = EnhancedAudioData::Multitrack {
            multitrack_type: AudioMultitrackType::OneTrack,
            tracks: vec![],
        };
        assert_eq!(multitrack.codec(), None);
    }

    #[test]
    fn test_sound_format_ex_header_constant() {
        assert_eq!(SOUND_FORMAT_EX_HEADER, 9);
    }

    #[test]
    fn test_all_supported_audio_codecs() {
        let codecs = [
            (b"mp4a", AudioFourCc::Aac),
            (b"Opus", AudioFourCc::Opus),
            (b"fLaC", AudioFourCc::Flac),
            (b"ac-3", AudioFourCc::Ac3),
            (b"ec-3", AudioFourCc::Eac3),
            (b".mp3", AudioFourCc::Mp3),
        ];

        for (fourcc, expected_codec) in codecs {
            let mut data = vec![0x90]; // SequenceStart
            data.extend_from_slice(fourcc);
            data.extend_from_slice(&[0x00]); // Empty config

            let parsed = EnhancedAudioData::parse(Bytes::from(data)).unwrap();
            assert_eq!(parsed.codec(), Some(expected_codec));
        }
    }
}
