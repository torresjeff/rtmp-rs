//! AAC audio parsing
//!
//! RTMP transports AAC audio in raw format (without ADTS headers).
//!
//! AAC Audio Packet Structure:
//! ```text
//! +----------+----------+----------+----------+---------+
//! |SoundFormat|SoundRate|SoundSize |SoundType | AACType | AACData
//! | (4 bits)  | (2 bits)| (1 bit)  | (1 bit)  | (1 byte)|
//! +----------+----------+----------+----------+---------+
//! ```
//!
//! AACPacketType:
//! - 0: AAC sequence header (AudioSpecificConfig)
//! - 1: AAC raw frame data

use bytes::{Buf, Bytes};

use crate::error::{MediaError, Result};

/// AAC packet type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AacPacketType {
    /// Sequence header (AudioSpecificConfig)
    SequenceHeader = 0,
    /// Raw AAC frame data
    Raw = 1,
}

impl AacPacketType {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0 => Some(AacPacketType::SequenceHeader),
            1 => Some(AacPacketType::Raw),
            _ => None,
        }
    }
}

/// AAC profile (audio object type)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AacProfile {
    /// AAC Main
    Main = 1,
    /// AAC LC (Low Complexity) - most common
    Lc = 2,
    /// AAC SSR (Scalable Sample Rate)
    Ssr = 3,
    /// AAC LTP (Long Term Prediction)
    Ltp = 4,
    /// SBR (Spectral Band Replication) - HE-AAC
    Sbr = 5,
    /// AAC Scalable
    Scalable = 6,
}

impl AacProfile {
    pub fn from_object_type(ot: u8) -> Option<Self> {
        match ot {
            1 => Some(AacProfile::Main),
            2 => Some(AacProfile::Lc),
            3 => Some(AacProfile::Ssr),
            4 => Some(AacProfile::Ltp),
            5 => Some(AacProfile::Sbr),
            6 => Some(AacProfile::Scalable),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            AacProfile::Main => "AAC Main",
            AacProfile::Lc => "AAC LC",
            AacProfile::Ssr => "AAC SSR",
            AacProfile::Ltp => "AAC LTP",
            AacProfile::Sbr => "HE-AAC",
            AacProfile::Scalable => "AAC Scalable",
        }
    }
}

/// AudioSpecificConfig (from sequence header)
#[derive(Debug, Clone)]
pub struct AudioSpecificConfig {
    /// Audio object type (profile)
    pub audio_object_type: u8,
    /// Sampling frequency index
    pub sampling_frequency_index: u8,
    /// Sampling frequency in Hz
    pub sampling_frequency: u32,
    /// Channel configuration (1=mono, 2=stereo, etc.)
    pub channel_configuration: u8,
    /// Frame length flag (960 or 1024 samples)
    pub frame_length_flag: bool,
    /// Depends on core coder flag
    pub depends_on_core_coder: bool,
    /// Extension flag
    pub extension_flag: bool,
    /// Raw config bytes
    pub raw: Bytes,
}

impl AudioSpecificConfig {
    /// Standard sampling frequencies by index
    const SAMPLING_FREQUENCIES: [u32; 16] = [
        96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050,
        16000, 12000, 11025, 8000, 7350, 0, 0, 0,
    ];

    /// Parse from AAC sequence header data
    pub fn parse(data: Bytes) -> Result<Self> {
        if data.len() < 2 {
            return Err(MediaError::InvalidAacPacket.into());
        }

        // AudioSpecificConfig is bit-packed
        // audioObjectType: 5 bits
        // samplingFrequencyIndex: 4 bits
        // if (samplingFrequencyIndex == 0xf) samplingFrequency: 24 bits
        // channelConfiguration: 4 bits
        // ... more optional fields

        let b0 = data[0];
        let b1 = data[1];

        let audio_object_type = (b0 >> 3) & 0x1F;
        let sampling_frequency_index = ((b0 & 0x07) << 1) | ((b1 >> 7) & 0x01);

        let sampling_frequency = if sampling_frequency_index == 0x0F {
            // Explicit frequency in next 24 bits
            if data.len() < 5 {
                return Err(MediaError::InvalidAacPacket.into());
            }
            let f0 = (data[1] & 0x7F) as u32;
            let f1 = data[2] as u32;
            let f2 = data[3] as u32;
            let f3 = (data[4] >> 1) as u32;
            (f0 << 17) | (f1 << 9) | (f2 << 1) | f3
        } else if (sampling_frequency_index as usize) < Self::SAMPLING_FREQUENCIES.len() {
            Self::SAMPLING_FREQUENCIES[sampling_frequency_index as usize]
        } else {
            return Err(MediaError::InvalidAacPacket.into());
        };

        let channel_configuration = (b1 >> 3) & 0x0F;
        let frame_length_flag = (b1 & 0x04) != 0;
        let depends_on_core_coder = (b1 & 0x02) != 0;
        let extension_flag = (b1 & 0x01) != 0;

        Ok(AudioSpecificConfig {
            audio_object_type,
            sampling_frequency_index,
            sampling_frequency,
            channel_configuration,
            frame_length_flag,
            depends_on_core_coder,
            extension_flag,
            raw: data,
        })
    }

    /// Get the profile
    pub fn profile(&self) -> Option<AacProfile> {
        AacProfile::from_object_type(self.audio_object_type)
    }

    /// Get channel count
    pub fn channels(&self) -> u8 {
        match self.channel_configuration {
            0 => 0, // Defined in stream
            1 => 1, // Mono
            2 => 2, // Stereo
            3 => 3, // 3.0
            4 => 4, // 4.0
            5 => 5, // 5.0
            6 => 6, // 5.1
            7 => 8, // 7.1
            _ => 0,
        }
    }

    /// Get samples per frame
    pub fn samples_per_frame(&self) -> u32 {
        if self.frame_length_flag { 960 } else { 1024 }
    }
}

/// Parsed AAC data
#[derive(Debug, Clone)]
pub enum AacData {
    /// Sequence header (AudioSpecificConfig)
    SequenceHeader(AudioSpecificConfig),

    /// Raw AAC frame
    Frame {
        /// Raw AAC data (without ADTS header)
        data: Bytes,
    },
}

impl AacData {
    /// Parse from RTMP audio data (after format byte)
    pub fn parse(mut data: Bytes) -> Result<Self> {
        if data.is_empty() {
            return Err(MediaError::InvalidAacPacket.into());
        }

        let packet_type = data.get_u8();

        match AacPacketType::from_byte(packet_type) {
            Some(AacPacketType::SequenceHeader) => {
                let config = AudioSpecificConfig::parse(data)?;
                Ok(AacData::SequenceHeader(config))
            }
            Some(AacPacketType::Raw) => {
                Ok(AacData::Frame { data })
            }
            None => Err(MediaError::InvalidAacPacket.into()),
        }
    }

    /// Check if this is a sequence header
    pub fn is_sequence_header(&self) -> bool {
        matches!(self, AacData::SequenceHeader(_))
    }
}

/// Generate ADTS header for a raw AAC frame
///
/// This is useful when writing AAC to a file that requires ADTS headers.
pub fn generate_adts_header(config: &AudioSpecificConfig, frame_length: usize) -> [u8; 7] {
    let profile = config.audio_object_type.saturating_sub(1); // ADTS uses profile - 1
    let freq_idx = config.sampling_frequency_index;
    let channels = config.channel_configuration;

    // ADTS header is 7 bytes (without CRC)
    let frame_len = frame_length + 7;

    let mut header = [0u8; 7];

    // Syncword (12 bits) + ID (1 bit) + Layer (2 bits) + Protection (1 bit)
    header[0] = 0xFF;
    header[1] = 0xF1; // MPEG-4, Layer 0, no CRC

    // Profile (2 bits) + Freq (4 bits) + Private (1 bit) + Channels (1 bit)
    header[2] = ((profile & 0x03) << 6)
        | ((freq_idx & 0x0F) << 2)
        | ((channels >> 2) & 0x01);

    // Channels (3 bits) + Original (1 bit) + Home (1 bit) + Copyright (1 bit) + Length (2 bits)
    header[3] = ((channels & 0x03) << 6)
        | ((frame_len >> 11) & 0x03) as u8;

    // Length (8 bits)
    header[4] = ((frame_len >> 3) & 0xFF) as u8;

    // Length (3 bits) + Buffer fullness (5 bits)
    header[5] = (((frame_len & 0x07) << 5) | 0x1F) as u8;

    // Buffer fullness (6 bits) + Number of frames (2 bits)
    header[6] = 0xFC;

    header
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_specific_config_parse() {
        // AAC-LC, 44100 Hz, Stereo
        let data = Bytes::from_static(&[0x12, 0x10]);

        let config = AudioSpecificConfig::parse(data).unwrap();
        assert_eq!(config.audio_object_type, 2); // AAC-LC
        assert_eq!(config.sampling_frequency_index, 4); // 44100 Hz
        assert_eq!(config.sampling_frequency, 44100);
        assert_eq!(config.channel_configuration, 2); // Stereo
        assert_eq!(config.channels(), 2);
        assert_eq!(config.profile(), Some(AacProfile::Lc));
    }

    #[test]
    fn test_adts_header() {
        let config = AudioSpecificConfig {
            audio_object_type: 2,
            sampling_frequency_index: 4,
            sampling_frequency: 44100,
            channel_configuration: 2,
            frame_length_flag: false,
            depends_on_core_coder: false,
            extension_flag: false,
            raw: Bytes::new(),
        };

        let header = generate_adts_header(&config, 100);

        // Check syncword
        assert_eq!(header[0], 0xFF);
        assert_eq!(header[1] & 0xF0, 0xF0);
    }
}
