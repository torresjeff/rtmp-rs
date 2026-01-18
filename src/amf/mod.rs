//! AMF (Action Message Format) implementation
//!
//! AMF is Adobe's binary serialization format used in RTMP for encoding
//! command parameters and metadata. This module implements both AMF0
//! (original format) and AMF3 (ActionScript 3.0 format).
//!
//! Most RTMP implementations use AMF0 for commands. AMF3 is encapsulated
//! inside AMF0 via the avmplus-object marker (0x11).

pub mod amf0;
pub mod amf3;
pub mod value;

pub use amf0::{Amf0Decoder, Amf0Encoder};
pub use amf3::{Amf3Decoder, Amf3Encoder};
pub use value::AmfValue;
