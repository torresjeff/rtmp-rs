//! H.264/AVC parsing
//!
//! RTMP transports H.264 in AVCC format (length-prefixed NAL units).
//!
//! AVC Video Packet Structure:
//! ```text
//! +----------+----------+-----------------+
//! |FrameType | CodecID  | AVCPacketType   | CompositionTime | Data
//! | (4 bits) | (4 bits) | (1 byte)        | (3 bytes, SI24) |
//! +----------+----------+-----------------+
//! ```
//!
//! AVCPacketType:
//! - 0: AVC sequence header (AVCDecoderConfigurationRecord)
//! - 1: AVC NALU (one or more NALUs)
//! - 2: AVC end of sequence
//!
//! AVCDecoderConfigurationRecord (sequence header):
//! ```text
//! configurationVersion (1) | AVCProfileIndication (1) | profile_compatibility (1)
//! | AVCLevelIndication (1) | lengthSizeMinusOne (1, lower 2 bits)
//! | numOfSPS (1, lower 5 bits) | { spsLength (2) | spsNALUnit }*
//! | numOfPPS (1) | { ppsLength (2) | ppsNALUnit }*
//! ```

use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::error::{MediaError, Result};

/// AVC packet type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvcPacketType {
    /// Sequence header (AVCDecoderConfigurationRecord)
    SequenceHeader = 0,
    /// NAL units
    Nalu = 1,
    /// End of sequence
    EndOfSequence = 2,
}

impl AvcPacketType {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0 => Some(AvcPacketType::SequenceHeader),
            1 => Some(AvcPacketType::Nalu),
            2 => Some(AvcPacketType::EndOfSequence),
            _ => None,
        }
    }
}

/// NAL unit type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NaluType {
    /// Non-IDR slice
    Slice = 1,
    /// Slice data partition A
    SlicePartA = 2,
    /// Slice data partition B
    SlicePartB = 3,
    /// Slice data partition C
    SlicePartC = 4,
    /// IDR slice (keyframe)
    Idr = 5,
    /// Supplemental enhancement information
    Sei = 6,
    /// Sequence parameter set
    Sps = 7,
    /// Picture parameter set
    Pps = 8,
    /// Access unit delimiter
    Aud = 9,
    /// End of sequence
    EndSeq = 10,
    /// End of stream
    EndStream = 11,
    /// Filler data
    Filler = 12,
}

impl NaluType {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b & 0x1F {
            1 => Some(NaluType::Slice),
            2 => Some(NaluType::SlicePartA),
            3 => Some(NaluType::SlicePartB),
            4 => Some(NaluType::SlicePartC),
            5 => Some(NaluType::Idr),
            6 => Some(NaluType::Sei),
            7 => Some(NaluType::Sps),
            8 => Some(NaluType::Pps),
            9 => Some(NaluType::Aud),
            10 => Some(NaluType::EndSeq),
            11 => Some(NaluType::EndStream),
            12 => Some(NaluType::Filler),
            _ => None,
        }
    }

    pub fn is_keyframe(&self) -> bool {
        matches!(self, NaluType::Idr)
    }

    pub fn is_parameter_set(&self) -> bool {
        matches!(self, NaluType::Sps | NaluType::Pps)
    }
}

/// Parsed H.264 data
#[derive(Debug, Clone)]
pub enum H264Data {
    /// Sequence header with SPS/PPS
    SequenceHeader(AvcConfig),

    /// Video frame (one or more NAL units)
    Frame {
        /// Whether this is a keyframe (IDR)
        keyframe: bool,
        /// Composition time offset (for B-frames)
        composition_time: i32,
        /// NAL units in AVCC format (length-prefixed)
        nalus: Bytes,
    },

    /// End of sequence marker
    EndOfSequence,
}

/// AVC decoder configuration (from sequence header)
#[derive(Debug, Clone)]
pub struct AvcConfig {
    /// AVC profile (66=Baseline, 77=Main, 100=High, etc.)
    pub profile: u8,
    /// Profile compatibility flags
    pub compatibility: u8,
    /// AVC level (e.g., 31 = 3.1)
    pub level: u8,
    /// NALU length size minus 1 (usually 3, meaning 4-byte lengths)
    pub nalu_length_size: u8,
    /// Sequence Parameter Sets
    pub sps: Vec<Bytes>,
    /// Picture Parameter Sets
    pub pps: Vec<Bytes>,
    /// Raw AVCDecoderConfigurationRecord bytes
    pub raw: Bytes,
}

impl AvcConfig {
    /// Parse from AVCDecoderConfigurationRecord
    pub fn parse(data: Bytes) -> Result<Self> {
        if data.len() < 7 {
            return Err(MediaError::InvalidAvcPacket.into());
        }

        // Clone the raw data before consuming it
        let raw = data.clone();
        let mut data = data;

        let version = data.get_u8();
        if version != 1 {
            return Err(MediaError::InvalidAvcPacket.into());
        }

        let profile = data.get_u8();
        let compatibility = data.get_u8();
        let level = data.get_u8();
        let nalu_length_size = (data.get_u8() & 0x03) + 1;

        // Parse SPS
        let num_sps = (data.get_u8() & 0x1F) as usize;
        let mut sps = Vec::with_capacity(num_sps);
        for _ in 0..num_sps {
            if data.len() < 2 {
                return Err(MediaError::InvalidAvcPacket.into());
            }
            let sps_len = data.get_u16() as usize;
            if data.len() < sps_len {
                return Err(MediaError::InvalidAvcPacket.into());
            }
            sps.push(data.copy_to_bytes(sps_len));
        }

        // Parse PPS
        if data.is_empty() {
            return Err(MediaError::InvalidAvcPacket.into());
        }
        let num_pps = data.get_u8() as usize;
        let mut pps = Vec::with_capacity(num_pps);
        for _ in 0..num_pps {
            if data.len() < 2 {
                return Err(MediaError::InvalidAvcPacket.into());
            }
            let pps_len = data.get_u16() as usize;
            if data.len() < pps_len {
                return Err(MediaError::InvalidAvcPacket.into());
            }
            pps.push(data.copy_to_bytes(pps_len));
        }

        Ok(AvcConfig {
            profile,
            compatibility,
            level,
            nalu_length_size,
            sps,
            pps,
            raw,
        })
    }

    /// Get profile name
    pub fn profile_name(&self) -> &'static str {
        match self.profile {
            66 => "Baseline",
            77 => "Main",
            88 => "Extended",
            100 => "High",
            110 => "High 10",
            122 => "High 4:2:2",
            244 => "High 4:4:4",
            _ => "Unknown",
        }
    }

    /// Get level as string (e.g., "3.1")
    pub fn level_string(&self) -> String {
        format!("{}.{}", self.level / 10, self.level % 10)
    }
}

impl H264Data {
    /// Parse from RTMP video data (after frame type and codec ID bytes)
    pub fn parse(mut data: Bytes) -> Result<Self> {
        if data.len() < 4 {
            return Err(MediaError::InvalidAvcPacket.into());
        }

        let packet_type = data.get_u8();

        // Composition time (signed 24-bit)
        let ct0 = data.get_u8() as i32;
        let ct1 = data.get_u8() as i32;
        let ct2 = data.get_u8() as i32;
        let composition_time = (ct0 << 16) | (ct1 << 8) | ct2;
        // Sign extend from 24 bits
        let composition_time = if composition_time & 0x800000 != 0 {
            composition_time | !0xFFFFFF
        } else {
            composition_time
        };

        match AvcPacketType::from_byte(packet_type) {
            Some(AvcPacketType::SequenceHeader) => {
                let config = AvcConfig::parse(data)?;
                Ok(H264Data::SequenceHeader(config))
            }
            Some(AvcPacketType::Nalu) => {
                // Check for IDR in the NAL units
                let keyframe = Self::contains_idr(&data);
                Ok(H264Data::Frame {
                    keyframe,
                    composition_time,
                    nalus: data,
                })
            }
            Some(AvcPacketType::EndOfSequence) => Ok(H264Data::EndOfSequence),
            None => Err(MediaError::InvalidAvcPacket.into()),
        }
    }

    /// Check if NAL units contain an IDR frame
    fn contains_idr(data: &Bytes) -> bool {
        let mut offset = 0;
        while offset + 4 < data.len() {
            // Read NALU length (assume 4 bytes, most common)
            let len = u32::from_be_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]) as usize;
            offset += 4;

            if offset >= data.len() {
                break;
            }

            // Check NAL unit type
            let nalu_type = NaluType::from_byte(data[offset]);
            if nalu_type == Some(NaluType::Idr) {
                return true;
            }

            offset += len;
        }
        false
    }

    /// Check if this is a keyframe
    pub fn is_keyframe(&self) -> bool {
        match self {
            H264Data::SequenceHeader(_) => true, // Sequence headers are keyframe-associated
            H264Data::Frame { keyframe, .. } => *keyframe,
            H264Data::EndOfSequence => false,
        }
    }

    /// Check if this is a sequence header
    pub fn is_sequence_header(&self) -> bool {
        matches!(self, H264Data::SequenceHeader(_))
    }

    /// Build the FLV video tag body for this AVC data (transmux, no re-encode).
    pub fn to_flv_tag_body(&self) -> Bytes {
        match self {
            H264Data::SequenceHeader(cfg) => {
                let mut body = BytesMut::with_capacity(5 + cfg.raw.len());
                body.put_u8(0x17); // keyframe, AVC
                body.put_u8(0x00); // sequence header
                body.put_u8(0x00); // composition time = 0 (SI24)
                body.put_u8(0x00);
                body.put_u8(0x00);
                body.extend_from_slice(&cfg.raw);
                body.freeze()
            }
            H264Data::Frame {
                keyframe,
                composition_time,
                nalus,
            } => {
                let mut body = BytesMut::with_capacity(5 + nalus.len());
                body.put_u8(if *keyframe { 0x17 } else { 0x27 });
                body.put_u8(0x01); // NALU
                let ct = *composition_time;
                body.put_u8(((ct >> 16) & 0xFF) as u8); // SI24 (signed)
                body.put_u8(((ct >> 8) & 0xFF) as u8);
                body.put_u8((ct & 0xFF) as u8);
                body.extend_from_slice(nalus);
                body.freeze()
            }
            H264Data::EndOfSequence => {
                let mut body = BytesMut::with_capacity(5);
                body.put_u8(0x17);
                body.put_u8(0x02); // end of sequence
                body.put_u8(0x00);
                body.put_u8(0x00);
                body.put_u8(0x00);
                body.freeze()
            }
        }
    }
}

/// Iterator over NAL units in AVCC format
pub struct NaluIterator<'a> {
    data: &'a [u8],
    offset: usize,
    nalu_length_size: usize,
}

impl<'a> NaluIterator<'a> {
    pub fn new(data: &'a [u8], nalu_length_size: u8) -> Self {
        Self {
            data,
            offset: 0,
            nalu_length_size: nalu_length_size as usize,
        }
    }
}

impl<'a> Iterator for NaluIterator<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset + self.nalu_length_size > self.data.len() {
            return None;
        }

        // Read length (big-endian)
        let mut len: usize = 0;
        for i in 0..self.nalu_length_size {
            len = (len << 8) | (self.data[self.offset + i] as usize);
        }
        self.offset += self.nalu_length_size;

        if self.offset + len > self.data.len() {
            return None;
        }

        let nalu = &self.data[self.offset..self.offset + len];
        self.offset += len;
        Some(nalu)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nalu_type() {
        assert_eq!(NaluType::from_byte(0x65), Some(NaluType::Idr));
        assert_eq!(NaluType::from_byte(0x67), Some(NaluType::Sps));
        assert_eq!(NaluType::from_byte(0x68), Some(NaluType::Pps));
        assert_eq!(NaluType::from_byte(0x41), Some(NaluType::Slice));
    }

    #[test]
    fn test_avc_config_parse() {
        // Minimal valid AVCDecoderConfigurationRecord
        let data = Bytes::from_static(&[
            0x01, // version
            0x64, // profile (High)
            0x00, // compatibility
            0x1F, // level 3.1
            0xFF, // nalu length size = 4
            0xE1, // 1 SPS
            0x00, 0x04, // SPS length
            0x67, 0x64, 0x00, 0x1F, // SPS data
            0x01, // 1 PPS
            0x00, 0x03, // PPS length
            0x68, 0xEF, 0x38, // PPS data
        ]);

        let config = AvcConfig::parse(data.clone()).unwrap();
        assert_eq!(config.profile, 100);
        assert_eq!(config.level, 31);
        assert_eq!(config.nalu_length_size, 4);
        assert_eq!(config.sps.len(), 1);
        assert_eq!(config.pps.len(), 1);
        assert_eq!(config.profile_name(), "High");
        assert_eq!(config.level_string(), "3.1");
        // Verify raw field contains the original bytes
        assert_eq!(config.raw.len(), data.len());
        assert_eq!(config.raw, data);
    }

    #[test]
    fn test_avc_packet_type() {
        assert_eq!(
            AvcPacketType::from_byte(0),
            Some(AvcPacketType::SequenceHeader)
        );
        assert_eq!(AvcPacketType::from_byte(1), Some(AvcPacketType::Nalu));
        assert_eq!(
            AvcPacketType::from_byte(2),
            Some(AvcPacketType::EndOfSequence)
        );
        assert_eq!(AvcPacketType::from_byte(3), None);
        assert_eq!(AvcPacketType::from_byte(255), None);
    }

    #[test]
    fn test_nalu_type_parsing() {
        // Test all documented NALU types
        assert_eq!(NaluType::from_byte(0x01), Some(NaluType::Slice));
        assert_eq!(NaluType::from_byte(0x02), Some(NaluType::SlicePartA));
        assert_eq!(NaluType::from_byte(0x03), Some(NaluType::SlicePartB));
        assert_eq!(NaluType::from_byte(0x04), Some(NaluType::SlicePartC));
        assert_eq!(NaluType::from_byte(0x05), Some(NaluType::Idr));
        assert_eq!(NaluType::from_byte(0x06), Some(NaluType::Sei));
        assert_eq!(NaluType::from_byte(0x07), Some(NaluType::Sps));
        assert_eq!(NaluType::from_byte(0x08), Some(NaluType::Pps));
        assert_eq!(NaluType::from_byte(0x09), Some(NaluType::Aud));
        assert_eq!(NaluType::from_byte(0x0A), Some(NaluType::EndSeq));
        assert_eq!(NaluType::from_byte(0x0B), Some(NaluType::EndStream));
        assert_eq!(NaluType::from_byte(0x0C), Some(NaluType::Filler));

        // Test with forbidden_zero_bit and nal_ref_idc bits set
        assert_eq!(NaluType::from_byte(0x65), Some(NaluType::Idr)); // 0x65 & 0x1F = 5
        assert_eq!(NaluType::from_byte(0x67), Some(NaluType::Sps)); // 0x67 & 0x1F = 7
    }

    #[test]
    fn test_nalu_type_is_keyframe() {
        assert!(NaluType::Idr.is_keyframe());
        assert!(!NaluType::Slice.is_keyframe());
        assert!(!NaluType::Sps.is_keyframe());
        assert!(!NaluType::Pps.is_keyframe());
    }

    #[test]
    fn test_nalu_type_is_parameter_set() {
        assert!(NaluType::Sps.is_parameter_set());
        assert!(NaluType::Pps.is_parameter_set());
        assert!(!NaluType::Idr.is_parameter_set());
        assert!(!NaluType::Slice.is_parameter_set());
    }

    #[test]
    fn test_h264_data_sequence_header() {
        // Simulate parsing a sequence header packet
        let data = Bytes::from_static(&[
            0x00, // AVC sequence header
            0x00, 0x00, 0x00, // composition time (0)
            // AVCDecoderConfigurationRecord
            0x01, 0x64, 0x00, 0x1F, 0xFF, // version, profile, compat, level, length-1
            0xE1, // 1 SPS
            0x00, 0x04, 0x67, 0x64, 0x00, 0x1F, // SPS
            0x01, // 1 PPS
            0x00, 0x03, 0x68, 0xEF, 0x38, // PPS
        ]);

        let h264 = H264Data::parse(data).unwrap();
        assert!(h264.is_sequence_header());
        assert!(h264.is_keyframe());

        if let H264Data::SequenceHeader(config) = h264 {
            assert_eq!(config.profile, 100);
        } else {
            panic!("Expected SequenceHeader");
        }
    }

    #[test]
    fn test_h264_data_end_of_sequence() {
        let data = Bytes::from_static(&[
            0x02, // End of sequence
            0x00, 0x00, 0x00, // composition time
        ]);

        let h264 = H264Data::parse(data).unwrap();
        assert!(matches!(h264, H264Data::EndOfSequence));
        assert!(!h264.is_keyframe());
        assert!(!h264.is_sequence_header());
    }

    #[test]
    fn test_h264_data_nalu_keyframe() {
        // Create NALU data with IDR frame
        let data = Bytes::from_static(&[
            0x01, // AVC NALU
            0x00, 0x00, 0x00, // composition time (0)
            // NALU with length prefix
            0x00, 0x00, 0x00, 0x05, // length = 5
            0x65, 0x88, 0x84, 0x00, 0x00, // IDR NALU (type 5)
        ]);

        let h264 = H264Data::parse(data).unwrap();
        assert!(h264.is_keyframe());
        assert!(!h264.is_sequence_header());

        if let H264Data::Frame {
            keyframe,
            composition_time,
            ..
        } = h264
        {
            assert!(keyframe);
            assert_eq!(composition_time, 0);
        } else {
            panic!("Expected Frame");
        }
    }

    #[test]
    fn test_h264_data_nalu_p_frame() {
        // Create NALU data with non-IDR slice
        let data = Bytes::from_static(&[
            0x01, // AVC NALU
            0x00, 0x00, 0x00, // composition time
            // NALU with length prefix
            0x00, 0x00, 0x00, 0x05, // length = 5
            0x41, 0x9A, 0x00, 0x00, 0x00, // Non-IDR slice (type 1)
        ]);

        let h264 = H264Data::parse(data).unwrap();
        assert!(!h264.is_keyframe());

        if let H264Data::Frame { keyframe, .. } = h264 {
            assert!(!keyframe);
        }
    }

    #[test]
    fn test_h264_composition_time_positive() {
        let data = Bytes::from_static(&[
            0x01, // AVC NALU
            0x00, 0x01, 0x00, // composition time = 256
            0x00, 0x00, 0x00, 0x01, // length
            0x41, // Non-IDR
        ]);

        let h264 = H264Data::parse(data).unwrap();
        if let H264Data::Frame {
            composition_time, ..
        } = h264
        {
            assert_eq!(composition_time, 256);
        }
    }

    #[test]
    fn test_h264_composition_time_negative() {
        // Negative composition time (sign-extended from 24 bits)
        let data = Bytes::from_static(&[
            0x01, // AVC NALU
            0xFF, 0xFF, 0x00, // composition time = -256 (as signed 24-bit)
            0x00, 0x00, 0x00, 0x01, // length
            0x41, // Non-IDR
        ]);

        let h264 = H264Data::parse(data).unwrap();
        if let H264Data::Frame {
            composition_time, ..
        } = h264
        {
            assert_eq!(composition_time, -256);
        }
    }

    #[test]
    fn test_h264_data_invalid_packet_type() {
        let data = Bytes::from_static(&[
            0x03, // Invalid packet type
            0x00, 0x00, 0x00,
        ]);

        let result = H264Data::parse(data);
        assert!(result.is_err());
    }

    #[test]
    fn test_h264_data_too_short() {
        let data = Bytes::from_static(&[0x00, 0x00]); // Less than 4 bytes
        let result = H264Data::parse(data);
        assert!(result.is_err());
    }

    #[test]
    fn test_avc_config_profile_names() {
        // Test various profile names
        let profiles = [
            (66, "Baseline"),
            (77, "Main"),
            (88, "Extended"),
            (100, "High"),
            (110, "High 10"),
            (122, "High 4:2:2"),
            (244, "High 4:4:4"),
            (99, "Unknown"),
        ];

        for (profile, expected_name) in profiles {
            let config = AvcConfig {
                profile,
                compatibility: 0,
                level: 31,
                nalu_length_size: 4,
                sps: vec![],
                pps: vec![],
                raw: Bytes::new(),
            };
            assert_eq!(config.profile_name(), expected_name);
        }
    }

    #[test]
    fn test_avc_config_level_string() {
        let config = AvcConfig {
            profile: 100,
            compatibility: 0,
            level: 41, // Level 4.1
            nalu_length_size: 4,
            sps: vec![],
            pps: vec![],
            raw: Bytes::new(),
        };
        assert_eq!(config.level_string(), "4.1");

        let config2 = AvcConfig {
            profile: 100,
            compatibility: 0,
            level: 52, // Level 5.2
            nalu_length_size: 4,
            sps: vec![],
            pps: vec![],
            raw: Bytes::new(),
        };
        assert_eq!(config2.level_string(), "5.2");
    }

    #[test]
    fn test_avc_config_invalid_version() {
        let data = Bytes::from_static(&[
            0x02, // Invalid version (should be 1)
            0x64, 0x00, 0x1F, 0xFF, 0xE1, 0x00, 0x04, 0x67, 0x64, 0x00, 0x1F, 0x01, 0x00, 0x03,
            0x68, 0xEF, 0x38,
        ]);

        let result = AvcConfig::parse(data);
        assert!(result.is_err());
    }

    #[test]
    fn test_avc_config_too_short() {
        let data = Bytes::from_static(&[0x01, 0x64, 0x00]); // Less than 7 bytes
        let result = AvcConfig::parse(data);
        assert!(result.is_err());
    }

    #[test]
    fn test_nalu_iterator() {
        // Create AVCC-format data with multiple NALUs
        let data: &[u8] = &[
            0x00, 0x00, 0x00, 0x03, // length = 3
            0x67, 0x64, 0x00, // SPS NALU
            0x00, 0x00, 0x00, 0x02, // length = 2
            0x68, 0xEF, // PPS NALU
        ];

        let mut iter = NaluIterator::new(data, 4);

        let nalu1 = iter.next().unwrap();
        assert_eq!(nalu1.len(), 3);
        assert_eq!(NaluType::from_byte(nalu1[0]), Some(NaluType::Sps));

        let nalu2 = iter.next().unwrap();
        assert_eq!(nalu2.len(), 2);
        assert_eq!(NaluType::from_byte(nalu2[0]), Some(NaluType::Pps));

        assert!(iter.next().is_none());
    }

    #[test]
    fn test_nalu_iterator_different_length_sizes() {
        // Test with 2-byte length prefix
        let data: &[u8] = &[
            0x00, 0x02, // length = 2
            0x65, 0x88, // IDR NALU
        ];

        let mut iter = NaluIterator::new(data, 2);
        let nalu = iter.next().unwrap();
        assert_eq!(nalu.len(), 2);
    }

    #[test]
    fn test_nalu_iterator_empty() {
        let data: &[u8] = &[];
        let mut iter = NaluIterator::new(data, 4);
        assert!(iter.next().is_none());
    }

    #[test]
    fn test_nalu_iterator_truncated() {
        // Length says 10 bytes but only 3 available
        let data: &[u8] = &[
            0x00, 0x00, 0x00, 0x0A, // length = 10
            0x67, 0x64, 0x00, // Only 3 bytes
        ];

        let mut iter = NaluIterator::new(data, 4);
        assert!(iter.next().is_none()); // Should return None for truncated data
    }
}
