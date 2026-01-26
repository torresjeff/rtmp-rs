//! Media handling for RTMP
//!
//! This module provides:
//! - FLV tag parsing and generation
//! - H.264/AVC NALU parsing
//! - AAC frame parsing
//! - GOP buffering for late-joiner support
//! - FOURCC codec identifiers for Enhanced RTMP
//! - Enhanced video/audio parsing for E-RTMP

pub mod aac;
pub mod enhanced_audio;
pub mod enhanced_video;
pub mod flv;
pub mod fourcc;
pub mod gop;
pub mod h264;

pub use aac::{AacData, AacPacketType, AudioSpecificConfig};
pub use enhanced_audio::{AudioPacketType, EnhancedAudioData};
pub use enhanced_video::{AvMultitrackType, EnhancedVideoData, ExVideoFrameType, VideoPacketType};
pub use flv::{FlvTag, FlvTagType};
pub use fourcc::{AudioFourCc, FourCC, VideoFourCc};
pub use gop::GopBuffer;
pub use h264::{AvcPacketType, H264Data, NaluType};
