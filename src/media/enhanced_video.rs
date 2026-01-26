//! Enhanced RTMP video parsing
//!
//! This module handles the Enhanced RTMP video format which uses FOURCC
//! codec signaling and supports modern codecs like HEVC, AV1, and VP9.
//!
//! Enhanced video is signaled by the `isExVideoHeader` bit (bit 7 of first byte).
//! When set, the lower 4 bits become `VideoPacketType` instead of codec ID.
//!
//! Reference: E-RTMP v2 specification - "Enhancing Video"

use bytes::{Buf, Bytes};

use crate::error::{MediaError, Result};
use crate::media::fourcc::VideoFourCc;

/// Enhanced video packet type (lower 4 bits when isExVideoHeader=1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum VideoPacketType {
    /// Sequence header (codec configuration)
    SequenceStart = 0,
    /// Coded video frames
    CodedFrames = 1,
    /// End of sequence
    SequenceEnd = 2,
    /// Coded frames with composition time = 0 (optimization)
    CodedFramesX = 3,
    /// Metadata (HDR info, etc.)
    Metadata = 4,
    /// MPEG-2 TS sequence start
    Mpeg2TsSequenceStart = 5,
    /// Multitrack video
    Multitrack = 6,
    /// ModEx signal (extensions follow)
    ModEx = 7,
}

impl VideoPacketType {
    /// Parse from the lower 4 bits of the first byte.
    pub fn from_byte(b: u8) -> Option<Self> {
        match b & 0x0F {
            0 => Some(VideoPacketType::SequenceStart),
            1 => Some(VideoPacketType::CodedFrames),
            2 => Some(VideoPacketType::SequenceEnd),
            3 => Some(VideoPacketType::CodedFramesX),
            4 => Some(VideoPacketType::Metadata),
            5 => Some(VideoPacketType::Mpeg2TsSequenceStart),
            6 => Some(VideoPacketType::Multitrack),
            7 => Some(VideoPacketType::ModEx),
            _ => None,
        }
    }
}

/// Enhanced video frame type (bits 4-6 when isExVideoHeader=1).
///
/// Note: In enhanced mode, frame type is 3 bits (1-5 valid values).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ExVideoFrameType {
    /// Keyframe (IDR for AVC/HEVC, key for VP9/AV1)
    Keyframe = 1,
    /// Inter frame (P-frame)
    InterFrame = 2,
    /// Disposable inter frame
    DisposableInterFrame = 3,
    /// Generated keyframe (server-side)
    GeneratedKeyframe = 4,
    /// Command frame (video info)
    CommandFrame = 5,
}

impl ExVideoFrameType {
    /// Parse from bits 4-6 of the first byte.
    pub fn from_byte(b: u8) -> Option<Self> {
        match (b >> 4) & 0x07 {
            1 => Some(ExVideoFrameType::Keyframe),
            2 => Some(ExVideoFrameType::InterFrame),
            3 => Some(ExVideoFrameType::DisposableInterFrame),
            4 => Some(ExVideoFrameType::GeneratedKeyframe),
            5 => Some(ExVideoFrameType::CommandFrame),
            _ => None,
        }
    }

    /// Check if this is a keyframe type.
    pub fn is_keyframe(&self) -> bool {
        matches!(
            self,
            ExVideoFrameType::Keyframe | ExVideoFrameType::GeneratedKeyframe
        )
    }
}

/// Multitrack type for video/audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AvMultitrackType {
    /// Single track with track ID
    OneTrack = 0,
    /// Multiple tracks, same codec
    ManyTracks = 1,
    /// Multiple tracks, different codecs
    ManyTracksManyCodecs = 2,
}

impl AvMultitrackType {
    /// Parse from byte value.
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0 => Some(AvMultitrackType::OneTrack),
            1 => Some(AvMultitrackType::ManyTracks),
            2 => Some(AvMultitrackType::ManyTracksManyCodecs),
            _ => None,
        }
    }
}

/// Parsed enhanced video data.
#[derive(Debug, Clone)]
pub enum EnhancedVideoData {
    /// Sequence header (codec configuration record).
    SequenceHeader {
        /// Video codec
        codec: VideoFourCc,
        /// Frame type (usually keyframe for sequence headers)
        frame_type: ExVideoFrameType,
        /// Codec-specific configuration data
        config: Bytes,
    },

    /// Coded video frame.
    Frame {
        /// Video codec
        codec: VideoFourCc,
        /// Frame type
        frame_type: ExVideoFrameType,
        /// Composition time offset in milliseconds (for B-frames)
        composition_time: i32,
        /// Frame data (NALUs for AVC/HEVC, OBUs for AV1, etc.)
        data: Bytes,
    },

    /// End of sequence marker.
    SequenceEnd {
        /// Video codec
        codec: VideoFourCc,
    },

    /// Metadata frame (HDR info, etc.).
    Metadata {
        /// Raw metadata bytes (AMF encoded)
        data: Bytes,
    },

    /// Multitrack video container.
    Multitrack {
        /// Multitrack type
        multitrack_type: AvMultitrackType,
        /// Individual tracks
        tracks: Vec<VideoTrack>,
    },
}

/// A single video track within a multitrack container.
#[derive(Debug, Clone)]
pub struct VideoTrack {
    /// Track ID (0-255)
    pub track_id: u8,
    /// Video codec for this track
    pub codec: VideoFourCc,
    /// Track data
    pub data: EnhancedVideoTrackData,
}

/// Data within a video track.
#[derive(Debug, Clone)]
pub enum EnhancedVideoTrackData {
    /// Sequence header for this track
    SequenceHeader { config: Bytes },
    /// Coded frame for this track
    Frame {
        frame_type: ExVideoFrameType,
        composition_time: i32,
        data: Bytes,
    },
    /// Sequence end for this track
    SequenceEnd,
}

impl EnhancedVideoData {
    /// Check if video data uses enhanced format (isExVideoHeader bit set).
    #[inline]
    pub fn is_enhanced(first_byte: u8) -> bool {
        (first_byte & 0x80) != 0
    }

    /// Parse enhanced video from RTMP video message payload.
    ///
    /// The payload should start with the FLV video tag header byte.
    pub fn parse(data: Bytes) -> Result<Self> {
        if data.is_empty() {
            return Err(MediaError::InvalidEnhancedVideoPacket.into());
        }

        let first_byte = data[0];

        // Verify this is enhanced video
        if !Self::is_enhanced(first_byte) {
            return Err(MediaError::InvalidEnhancedVideoPacket.into());
        }

        let frame_type = ExVideoFrameType::from_byte(first_byte)
            .ok_or(MediaError::InvalidEnhancedVideoPacket)?;

        let packet_type =
            VideoPacketType::from_byte(first_byte).ok_or(MediaError::InvalidEnhancedVideoPacket)?;

        let mut cursor = data.slice(1..);

        // Handle packet types
        match packet_type {
            VideoPacketType::ModEx => {
                // ModEx signals that extensions follow
                // For now, skip ModEx bytes and parse the actual content
                Self::parse_with_modex(cursor, frame_type)
            }
            VideoPacketType::Multitrack => Self::parse_multitrack(cursor, frame_type),
            VideoPacketType::Metadata => Ok(EnhancedVideoData::Metadata { data: cursor }),
            _ => {
                // Regular enhanced video: FOURCC follows
                if cursor.len() < 4 {
                    return Err(MediaError::InvalidEnhancedVideoPacket.into());
                }

                let fourcc_bytes = [cursor[0], cursor[1], cursor[2], cursor[3]];
                let codec = VideoFourCc::from_bytes(&fourcc_bytes)
                    .ok_or(MediaError::UnsupportedVideoCodec)?;
                cursor.advance(4);

                Self::parse_by_packet_type(packet_type, codec, frame_type, cursor)
            }
        }
    }

    /// Parse with ModEx prefix handling.
    fn parse_with_modex(mut data: Bytes, frame_type: ExVideoFrameType) -> Result<Self> {
        // ModEx format: one or more extension bytes followed by actual packet
        // For now, we just skip the ModEx byte and try to parse what follows
        // A full implementation would parse TimestampOffsetNano etc.

        if data.is_empty() {
            return Err(MediaError::InvalidEnhancedVideoPacket.into());
        }

        let modex_type = data[0];
        data.advance(1);

        // TimestampOffsetNano (type 0) has 3 bytes of nanosecond offset
        if modex_type == 0 {
            if data.len() < 3 {
                return Err(MediaError::InvalidEnhancedVideoPacket.into());
            }
            // Skip the 3-byte nanosecond offset for now
            data.advance(3);
        }

        // After ModEx data, check if there's another packet type byte or FOURCC
        if data.len() < 4 {
            return Err(MediaError::InvalidEnhancedVideoPacket.into());
        }

        // Try to parse as FOURCC
        let fourcc_bytes = [data[0], data[1], data[2], data[3]];
        if let Some(codec) = VideoFourCc::from_bytes(&fourcc_bytes) {
            data.advance(4);
            // Assume CodedFramesX after ModEx (composition time = 0)
            Ok(EnhancedVideoData::Frame {
                codec,
                frame_type,
                composition_time: 0,
                data,
            })
        } else {
            Err(MediaError::InvalidEnhancedVideoPacket.into())
        }
    }

    /// Parse multitrack video container.
    fn parse_multitrack(mut data: Bytes, _frame_type: ExVideoFrameType) -> Result<Self> {
        if data.is_empty() {
            return Err(MediaError::InvalidEnhancedVideoPacket.into());
        }

        let multitrack_byte = data[0];
        data.advance(1);

        let multitrack_type = AvMultitrackType::from_byte(multitrack_byte >> 4)
            .ok_or(MediaError::InvalidEnhancedVideoPacket)?;
        let packet_type = VideoPacketType::from_byte(multitrack_byte)
            .ok_or(MediaError::InvalidEnhancedVideoPacket)?;

        let mut tracks = Vec::new();

        match multitrack_type {
            AvMultitrackType::OneTrack => {
                // Single track: trackId (1 byte) + FOURCC (4 bytes) + data
                if data.len() < 5 {
                    return Err(MediaError::InvalidEnhancedVideoPacket.into());
                }
                let track_id = data[0];
                data.advance(1);

                let fourcc_bytes = [data[0], data[1], data[2], data[3]];
                let codec = VideoFourCc::from_bytes(&fourcc_bytes)
                    .ok_or(MediaError::UnsupportedVideoCodec)?;
                data.advance(4);

                let track_data = Self::parse_track_data(packet_type, &mut data)?;
                tracks.push(VideoTrack {
                    track_id,
                    codec,
                    data: track_data,
                });
            }
            AvMultitrackType::ManyTracks => {
                // Multiple tracks, same codec: FOURCC (4 bytes) then track entries
                if data.len() < 4 {
                    return Err(MediaError::InvalidEnhancedVideoPacket.into());
                }
                let fourcc_bytes = [data[0], data[1], data[2], data[3]];
                let codec = VideoFourCc::from_bytes(&fourcc_bytes)
                    .ok_or(MediaError::UnsupportedVideoCodec)?;
                data.advance(4);

                // Parse track entries until end of data
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

                    tracks.push(VideoTrack {
                        track_id,
                        codec,
                        data: EnhancedVideoTrackData::Frame {
                            frame_type: ExVideoFrameType::InterFrame, // Default
                            composition_time: 0,
                            data: track_bytes,
                        },
                    });
                }
            }
            AvMultitrackType::ManyTracksManyCodecs => {
                // Multiple tracks with different codecs
                while !data.is_empty() {
                    if data.len() < 8 {
                        break;
                    }
                    let track_id = data[0];
                    data.advance(1);

                    let fourcc_bytes = [data[0], data[1], data[2], data[3]];
                    let codec = VideoFourCc::from_bytes(&fourcc_bytes)
                        .ok_or(MediaError::UnsupportedVideoCodec)?;
                    data.advance(4);

                    let track_size =
                        ((data[0] as usize) << 16) | ((data[1] as usize) << 8) | (data[2] as usize);
                    data.advance(3);

                    if data.len() < track_size {
                        break;
                    }
                    let track_bytes = data.slice(..track_size);
                    data.advance(track_size);

                    tracks.push(VideoTrack {
                        track_id,
                        codec,
                        data: EnhancedVideoTrackData::Frame {
                            frame_type: ExVideoFrameType::InterFrame,
                            composition_time: 0,
                            data: track_bytes,
                        },
                    });
                }
            }
        }

        Ok(EnhancedVideoData::Multitrack {
            multitrack_type,
            tracks,
        })
    }

    /// Parse track data based on packet type.
    fn parse_track_data(
        packet_type: VideoPacketType,
        data: &mut Bytes,
    ) -> Result<EnhancedVideoTrackData> {
        match packet_type {
            VideoPacketType::SequenceStart => Ok(EnhancedVideoTrackData::SequenceHeader {
                config: data.clone(),
            }),
            VideoPacketType::SequenceEnd => Ok(EnhancedVideoTrackData::SequenceEnd),
            VideoPacketType::CodedFramesX => Ok(EnhancedVideoTrackData::Frame {
                frame_type: ExVideoFrameType::InterFrame,
                composition_time: 0,
                data: data.clone(),
            }),
            VideoPacketType::CodedFrames => {
                if data.len() < 3 {
                    return Err(MediaError::InvalidEnhancedVideoPacket.into());
                }
                let ct = read_si24(data)?;
                Ok(EnhancedVideoTrackData::Frame {
                    frame_type: ExVideoFrameType::InterFrame,
                    composition_time: ct,
                    data: data.clone(),
                })
            }
            _ => Err(MediaError::InvalidEnhancedVideoPacket.into()),
        }
    }

    /// Parse by specific packet type.
    fn parse_by_packet_type(
        packet_type: VideoPacketType,
        codec: VideoFourCc,
        frame_type: ExVideoFrameType,
        mut data: Bytes,
    ) -> Result<Self> {
        match packet_type {
            VideoPacketType::SequenceStart => Ok(EnhancedVideoData::SequenceHeader {
                codec,
                frame_type,
                config: data,
            }),
            VideoPacketType::SequenceEnd => Ok(EnhancedVideoData::SequenceEnd { codec }),
            VideoPacketType::CodedFramesX => {
                // Composition time is implicitly 0
                Ok(EnhancedVideoData::Frame {
                    codec,
                    frame_type,
                    composition_time: 0,
                    data,
                })
            }
            VideoPacketType::CodedFrames => {
                // SI24 composition time follows
                if data.len() < 3 {
                    return Err(MediaError::InvalidEnhancedVideoPacket.into());
                }
                let composition_time = read_si24(&mut data)?;
                Ok(EnhancedVideoData::Frame {
                    codec,
                    frame_type,
                    composition_time,
                    data,
                })
            }
            VideoPacketType::Mpeg2TsSequenceStart => {
                // MPEG-2 TS format sequence header
                Ok(EnhancedVideoData::SequenceHeader {
                    codec,
                    frame_type,
                    config: data,
                })
            }
            _ => Err(MediaError::InvalidEnhancedVideoPacket.into()),
        }
    }

    /// Check if this is a keyframe.
    pub fn is_keyframe(&self) -> bool {
        match self {
            EnhancedVideoData::SequenceHeader { frame_type, .. } => frame_type.is_keyframe(),
            EnhancedVideoData::Frame { frame_type, .. } => frame_type.is_keyframe(),
            EnhancedVideoData::SequenceEnd { .. } => false,
            EnhancedVideoData::Metadata { .. } => false,
            EnhancedVideoData::Multitrack { .. } => false, // Would need to check tracks
        }
    }

    /// Check if this is a sequence header.
    pub fn is_sequence_header(&self) -> bool {
        matches!(self, EnhancedVideoData::SequenceHeader { .. })
    }

    /// Get the codec if available.
    pub fn codec(&self) -> Option<VideoFourCc> {
        match self {
            EnhancedVideoData::SequenceHeader { codec, .. } => Some(*codec),
            EnhancedVideoData::Frame { codec, .. } => Some(*codec),
            EnhancedVideoData::SequenceEnd { codec } => Some(*codec),
            EnhancedVideoData::Metadata { .. } => None,
            EnhancedVideoData::Multitrack { .. } => None,
        }
    }
}

/// Read a signed 24-bit integer (SI24) from the buffer.
fn read_si24(data: &mut Bytes) -> Result<i32> {
    if data.len() < 3 {
        return Err(MediaError::InvalidEnhancedVideoPacket.into());
    }

    let b0 = data.get_u8() as i32;
    let b1 = data.get_u8() as i32;
    let b2 = data.get_u8() as i32;

    let value = (b0 << 16) | (b1 << 8) | b2;

    // Sign extend from 24 bits
    if value & 0x800000 != 0 {
        Ok(value | !0xFFFFFF)
    } else {
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_enhanced() {
        // Enhanced video has bit 7 set
        assert!(EnhancedVideoData::is_enhanced(0x80));
        assert!(EnhancedVideoData::is_enhanced(0x90));
        assert!(EnhancedVideoData::is_enhanced(0xFF));

        // Legacy video has bit 7 clear
        assert!(!EnhancedVideoData::is_enhanced(0x17)); // Keyframe + AVC
        assert!(!EnhancedVideoData::is_enhanced(0x27)); // InterFrame + AVC
        assert!(!EnhancedVideoData::is_enhanced(0x00));
    }

    #[test]
    fn test_video_packet_type_parsing() {
        assert_eq!(
            VideoPacketType::from_byte(0x80),
            Some(VideoPacketType::SequenceStart)
        );
        assert_eq!(
            VideoPacketType::from_byte(0x81),
            Some(VideoPacketType::CodedFrames)
        );
        assert_eq!(
            VideoPacketType::from_byte(0x82),
            Some(VideoPacketType::SequenceEnd)
        );
        assert_eq!(
            VideoPacketType::from_byte(0x83),
            Some(VideoPacketType::CodedFramesX)
        );
        assert_eq!(
            VideoPacketType::from_byte(0x84),
            Some(VideoPacketType::Metadata)
        );
        assert_eq!(
            VideoPacketType::from_byte(0x85),
            Some(VideoPacketType::Mpeg2TsSequenceStart)
        );
        assert_eq!(
            VideoPacketType::from_byte(0x86),
            Some(VideoPacketType::Multitrack)
        );
        assert_eq!(
            VideoPacketType::from_byte(0x87),
            Some(VideoPacketType::ModEx)
        );

        // Values 8-15 are reserved/invalid
        assert_eq!(VideoPacketType::from_byte(0x88), None);
        assert_eq!(VideoPacketType::from_byte(0x8F), None);
    }

    #[test]
    fn test_ex_video_frame_type_parsing() {
        // Frame type is in bits 4-6
        assert_eq!(
            ExVideoFrameType::from_byte(0x90),
            Some(ExVideoFrameType::Keyframe)
        ); // 0b1001_0000
        assert_eq!(
            ExVideoFrameType::from_byte(0xA0),
            Some(ExVideoFrameType::InterFrame)
        ); // 0b1010_0000
        assert_eq!(
            ExVideoFrameType::from_byte(0xB0),
            Some(ExVideoFrameType::DisposableInterFrame)
        );
        assert_eq!(
            ExVideoFrameType::from_byte(0xC0),
            Some(ExVideoFrameType::GeneratedKeyframe)
        );
        assert_eq!(
            ExVideoFrameType::from_byte(0xD0),
            Some(ExVideoFrameType::CommandFrame)
        );

        // Frame type 0 is invalid
        assert_eq!(ExVideoFrameType::from_byte(0x80), None);
        // Frame types 6-7 are invalid
        assert_eq!(ExVideoFrameType::from_byte(0xE0), None);
        assert_eq!(ExVideoFrameType::from_byte(0xF0), None);
    }

    #[test]
    fn test_frame_type_is_keyframe() {
        assert!(ExVideoFrameType::Keyframe.is_keyframe());
        assert!(ExVideoFrameType::GeneratedKeyframe.is_keyframe());
        assert!(!ExVideoFrameType::InterFrame.is_keyframe());
        assert!(!ExVideoFrameType::DisposableInterFrame.is_keyframe());
        assert!(!ExVideoFrameType::CommandFrame.is_keyframe());
    }

    #[test]
    fn test_multitrack_type_parsing() {
        assert_eq!(
            AvMultitrackType::from_byte(0),
            Some(AvMultitrackType::OneTrack)
        );
        assert_eq!(
            AvMultitrackType::from_byte(1),
            Some(AvMultitrackType::ManyTracks)
        );
        assert_eq!(
            AvMultitrackType::from_byte(2),
            Some(AvMultitrackType::ManyTracksManyCodecs)
        );
        assert_eq!(AvMultitrackType::from_byte(3), None);
    }

    #[test]
    fn test_parse_sequence_header() {
        // Enhanced HEVC sequence header
        // 0x90 = isExHeader(1) + Keyframe(001) + SequenceStart(0000)
        let mut data = vec![0x90];
        data.extend_from_slice(b"hvc1"); // FOURCC
        data.extend_from_slice(&[0x01, 0x02, 0x03, 0x04]); // Config data

        let parsed = EnhancedVideoData::parse(Bytes::from(data)).unwrap();

        match parsed {
            EnhancedVideoData::SequenceHeader {
                codec,
                frame_type,
                config,
            } => {
                assert_eq!(codec, VideoFourCc::Hevc);
                assert_eq!(frame_type, ExVideoFrameType::Keyframe);
                assert_eq!(config.as_ref(), &[0x01, 0x02, 0x03, 0x04]);
            }
            _ => panic!("Expected SequenceHeader"),
        }
    }

    #[test]
    fn test_parse_coded_frames_x() {
        // Enhanced AV1 frame with CodedFramesX (no composition time)
        // 0xA3 = isExHeader(1) + InterFrame(010) + CodedFramesX(0011)
        let mut data = vec![0xA3];
        data.extend_from_slice(b"av01"); // FOURCC
        data.extend_from_slice(&[0xAA, 0xBB, 0xCC]); // Frame data

        let parsed = EnhancedVideoData::parse(Bytes::from(data)).unwrap();

        match parsed {
            EnhancedVideoData::Frame {
                codec,
                frame_type,
                composition_time,
                data,
            } => {
                assert_eq!(codec, VideoFourCc::Av1);
                assert_eq!(frame_type, ExVideoFrameType::InterFrame);
                assert_eq!(composition_time, 0);
                assert_eq!(data.as_ref(), &[0xAA, 0xBB, 0xCC]);
            }
            _ => panic!("Expected Frame"),
        }
    }

    #[test]
    fn test_parse_coded_frames_with_composition_time() {
        // Enhanced VP9 frame with CodedFrames (has composition time)
        // 0x91 = isExHeader(1) + Keyframe(001) + CodedFrames(0001)
        let mut data = vec![0x91];
        data.extend_from_slice(b"vp09"); // FOURCC
        data.extend_from_slice(&[0x00, 0x01, 0x00]); // Composition time = 256
        data.extend_from_slice(&[0xDD, 0xEE, 0xFF]); // Frame data

        let parsed = EnhancedVideoData::parse(Bytes::from(data)).unwrap();

        match parsed {
            EnhancedVideoData::Frame {
                codec,
                frame_type,
                composition_time,
                data,
            } => {
                assert_eq!(codec, VideoFourCc::Vp9);
                assert_eq!(frame_type, ExVideoFrameType::Keyframe);
                assert_eq!(composition_time, 256);
                assert_eq!(data.as_ref(), &[0xDD, 0xEE, 0xFF]);
            }
            _ => panic!("Expected Frame"),
        }
    }

    #[test]
    fn test_parse_negative_composition_time() {
        // Frame with negative composition time
        let mut data = vec![0x91]; // Keyframe + CodedFrames
        data.extend_from_slice(b"avc1"); // FOURCC
        data.extend_from_slice(&[0xFF, 0xFF, 0x00]); // -256 as SI24
        data.extend_from_slice(&[0x11, 0x22]); // Frame data

        let parsed = EnhancedVideoData::parse(Bytes::from(data)).unwrap();

        match parsed {
            EnhancedVideoData::Frame {
                composition_time, ..
            } => {
                assert_eq!(composition_time, -256);
            }
            _ => panic!("Expected Frame"),
        }
    }

    #[test]
    fn test_parse_sequence_end() {
        // Sequence end for HEVC
        // 0x92 = isExHeader(1) + Keyframe(001) + SequenceEnd(0010)
        let mut data = vec![0x92];
        data.extend_from_slice(b"hvc1");

        let parsed = EnhancedVideoData::parse(Bytes::from(data)).unwrap();

        match parsed {
            EnhancedVideoData::SequenceEnd { codec } => {
                assert_eq!(codec, VideoFourCc::Hevc);
            }
            _ => panic!("Expected SequenceEnd"),
        }
    }

    #[test]
    fn test_parse_metadata() {
        // Metadata packet (e.g., HDR info)
        // 0x94 = isExHeader(1) + Keyframe(001) + Metadata(0100)
        let data = vec![0x94, 0x01, 0x02, 0x03];

        let parsed = EnhancedVideoData::parse(Bytes::from(data)).unwrap();

        match parsed {
            EnhancedVideoData::Metadata { data } => {
                assert_eq!(data.as_ref(), &[0x01, 0x02, 0x03]);
            }
            _ => panic!("Expected Metadata"),
        }
    }

    #[test]
    fn test_parse_error_empty() {
        let result = EnhancedVideoData::parse(Bytes::new());
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_error_not_enhanced() {
        // Legacy AVC - not enhanced
        let data = Bytes::from_static(&[0x17, 0x00, 0x00, 0x00, 0x00]);
        let result = EnhancedVideoData::parse(data);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_error_no_fourcc() {
        // Enhanced header but no FOURCC
        let data = Bytes::from_static(&[0x90, 0x01, 0x02]);
        let result = EnhancedVideoData::parse(data);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_error_unknown_fourcc() {
        // Enhanced header with unknown FOURCC
        let mut data = vec![0x90];
        data.extend_from_slice(b"xxxx"); // Unknown codec
        let result = EnhancedVideoData::parse(Bytes::from(data));
        assert!(result.is_err());
    }

    #[test]
    fn test_is_keyframe() {
        // Sequence header
        let data = EnhancedVideoData::SequenceHeader {
            codec: VideoFourCc::Hevc,
            frame_type: ExVideoFrameType::Keyframe,
            config: Bytes::new(),
        };
        assert!(data.is_keyframe());

        // Keyframe
        let data = EnhancedVideoData::Frame {
            codec: VideoFourCc::Av1,
            frame_type: ExVideoFrameType::Keyframe,
            composition_time: 0,
            data: Bytes::new(),
        };
        assert!(data.is_keyframe());

        // Inter frame
        let data = EnhancedVideoData::Frame {
            codec: VideoFourCc::Av1,
            frame_type: ExVideoFrameType::InterFrame,
            composition_time: 0,
            data: Bytes::new(),
        };
        assert!(!data.is_keyframe());

        // Sequence end
        let data = EnhancedVideoData::SequenceEnd {
            codec: VideoFourCc::Hevc,
        };
        assert!(!data.is_keyframe());
    }

    #[test]
    fn test_is_sequence_header() {
        let header = EnhancedVideoData::SequenceHeader {
            codec: VideoFourCc::Hevc,
            frame_type: ExVideoFrameType::Keyframe,
            config: Bytes::new(),
        };
        assert!(header.is_sequence_header());

        let frame = EnhancedVideoData::Frame {
            codec: VideoFourCc::Hevc,
            frame_type: ExVideoFrameType::Keyframe,
            composition_time: 0,
            data: Bytes::new(),
        };
        assert!(!frame.is_sequence_header());
    }

    #[test]
    fn test_codec_accessor() {
        let header = EnhancedVideoData::SequenceHeader {
            codec: VideoFourCc::Av1,
            frame_type: ExVideoFrameType::Keyframe,
            config: Bytes::new(),
        };
        assert_eq!(header.codec(), Some(VideoFourCc::Av1));

        let metadata = EnhancedVideoData::Metadata { data: Bytes::new() };
        assert_eq!(metadata.codec(), None);
    }

    #[test]
    fn test_read_si24_positive() {
        let mut data = Bytes::from_static(&[0x00, 0x01, 0x00]); // 256
        assert_eq!(read_si24(&mut data).unwrap(), 256);

        let mut data = Bytes::from_static(&[0x7F, 0xFF, 0xFF]); // Max positive
        assert_eq!(read_si24(&mut data).unwrap(), 8388607);
    }

    #[test]
    fn test_read_si24_negative() {
        let mut data = Bytes::from_static(&[0xFF, 0xFF, 0x00]); // -256
        assert_eq!(read_si24(&mut data).unwrap(), -256);

        let mut data = Bytes::from_static(&[0x80, 0x00, 0x00]); // Min negative
        assert_eq!(read_si24(&mut data).unwrap(), -8388608);
    }

    #[test]
    fn test_read_si24_zero() {
        let mut data = Bytes::from_static(&[0x00, 0x00, 0x00]);
        assert_eq!(read_si24(&mut data).unwrap(), 0);
    }

    #[test]
    fn test_read_si24_too_short() {
        let mut data = Bytes::from_static(&[0x00, 0x01]);
        assert!(read_si24(&mut data).is_err());
    }
}
