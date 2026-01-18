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

use bytes::{Buf, Bytes};

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
}

impl AvcConfig {
    /// Parse from AVCDecoderConfigurationRecord
    pub fn parse(mut data: Bytes) -> Result<Self> {
        if data.len() < 7 {
            return Err(MediaError::InvalidAvcPacket.into());
        }

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
            Some(AvcPacketType::EndOfSequence) => {
                Ok(H264Data::EndOfSequence)
            }
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

        let config = AvcConfig::parse(data).unwrap();
        assert_eq!(config.profile, 100);
        assert_eq!(config.level, 31);
        assert_eq!(config.nalu_length_size, 4);
        assert_eq!(config.sps.len(), 1);
        assert_eq!(config.pps.len(), 1);
        assert_eq!(config.profile_name(), "High");
        assert_eq!(config.level_string(), "3.1");
    }
}
