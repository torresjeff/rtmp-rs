//! Media handling for RTMP
//!
//! This module provides:
//! - FLV tag parsing and generation
//! - H.264/AVC NALU parsing
//! - AAC frame parsing
//! - GOP buffering for late-joiner support
//! - FOURCC codec identifiers for Enhanced RTMP

pub mod aac;
pub mod flv;
pub mod fourcc;
pub mod gop;
pub mod h264;

pub use aac::{AacData, AacPacketType, AudioSpecificConfig};
pub use flv::{FlvTag, FlvTagType};
pub use fourcc::{AudioFourCc, FourCC, VideoFourCc};
pub use gop::GopBuffer;
pub use h264::{AvcPacketType, H264Data, NaluType};
