//! RTMP chunk stream codec
//!
//! RTMP messages are split into chunks for multiplexing. Each chunk has a header
//! that identifies the chunk stream and message being sent.
//!
//! ```text
//! Chunk Format:
//! +-------------+----------------+-------------------+
//! | Basic Header| Message Header | Chunk Data        |
//! | (1-3 bytes) | (0,3,7,11 bytes)| (variable)       |
//! +-------------+----------------+-------------------+
//!
//! Basic Header formats:
//! - 1 byte:  fmt(2) + csid(6)        for csid 2-63
//! - 2 bytes: fmt(2) + 0 + csid(8)    for csid 64-319
//! - 3 bytes: fmt(2) + 1 + csid(16)   for csid 64-65599
//!
//! Message Header formats (based on fmt):
//! - Type 0 (11 bytes): timestamp(3) + length(3) + type(1) + stream_id(4)
//! - Type 1 (7 bytes):  timestamp_delta(3) + length(3) + type(1)
//! - Type 2 (3 bytes):  timestamp_delta(3)
//! - Type 3 (0 bytes):  (use previous chunk's values)
//!
//! Extended timestamp (4 bytes) is appended when timestamp >= 0xFFFFFF
//! ```
//!
//! Reference: RTMP Specification Section 5.3

use bytes::{Buf, BufMut, Bytes, BytesMut};
use std::collections::HashMap;

use crate::error::{ProtocolError, Result};
use crate::protocol::constants::*;

/// A complete RTMP message (reassembled from chunks)
#[derive(Debug, Clone)]
pub struct RtmpChunk {
    /// Chunk stream ID (for multiplexing)
    pub csid: u32,
    /// Message timestamp (milliseconds)
    pub timestamp: u32,
    /// Message type ID
    pub message_type: u8,
    /// Message stream ID
    pub stream_id: u32,
    /// Message payload
    pub payload: Bytes,
}

/// Per-chunk-stream state for reassembly
#[derive(Debug, Clone, Default)]
struct ChunkStreamState {
    /// Last timestamp (absolute)
    timestamp: u32,
    /// Last timestamp delta
    timestamp_delta: u32,
    /// Last message length
    message_length: u32,
    /// Last message type
    message_type: u8,
    /// Last message stream ID
    stream_id: u32,
    /// Whether we've received extended timestamp
    has_extended_timestamp: bool,
    /// Buffer for partial message reassembly
    partial_message: BytesMut,
    /// Expected total length of current message
    expected_length: u32,
}

/// Chunk stream decoder
///
/// Handles chunk demultiplexing and message reassembly.
pub struct ChunkDecoder {
    /// Maximum incoming chunk size
    chunk_size: u32,
    /// Per-chunk-stream state
    streams: HashMap<u32, ChunkStreamState>,
    /// Maximum message size (sanity limit)
    max_message_size: u32,
}

impl ChunkDecoder {
    /// Create a new decoder with default chunk size
    pub fn new() -> Self {
        Self {
            chunk_size: DEFAULT_CHUNK_SIZE,
            streams: HashMap::new(),
            max_message_size: MAX_MESSAGE_SIZE,
        }
    }

    /// Set the chunk size (called when receiving SetChunkSize message)
    pub fn set_chunk_size(&mut self, size: u32) {
        self.chunk_size = size.min(MAX_CHUNK_SIZE);
    }

    /// Get current chunk size
    pub fn chunk_size(&self) -> u32 {
        self.chunk_size
    }

    /// Try to decode a complete message from the buffer
    ///
    /// Returns Ok(Some(chunk)) if a complete message was decoded,
    /// Ok(None) if more data is needed, or Err on protocol error.
    pub fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<RtmpChunk>> {
        if buf.is_empty() {
            return Ok(None);
        }

        // Parse basic header to get csid and fmt
        let (fmt, csid, header_len) = match self.parse_basic_header(buf)? {
            Some(v) => v,
            None => return Ok(None),
        };

        // Get or create chunk stream state
        let state = self.streams.entry(csid).or_default();

        // Calculate message header size based on fmt
        let msg_header_size = match fmt {
            0 => 11,
            1 => 7,
            2 => 3,
            3 => 0,
            _ => return Err(ProtocolError::InvalidChunkHeader.into()),
        };

        // Check if we have enough data for headers
        let needs_extended = if fmt == 3 {
            state.has_extended_timestamp
        } else if buf.len() > header_len + 2 {
            // Peek at timestamp field to check for extended timestamp
            let ts_bytes = &buf[header_len..header_len + 3];
            let ts = ((ts_bytes[0] as u32) << 16) | ((ts_bytes[1] as u32) << 8) | (ts_bytes[2] as u32);
            ts >= EXTENDED_TIMESTAMP_THRESHOLD
        } else {
            false
        };

        let extended_size = if needs_extended { 4 } else { 0 };
        let total_header_size = header_len + msg_header_size + extended_size;

        if buf.len() < total_header_size {
            return Ok(None); // Need more header data
        }

        // Parse message header
        buf.advance(header_len);

        let (timestamp_field, message_length, message_type, stream_id) = match fmt {
            0 => {
                // Full header
                let ts = buf.get_uint(3) as u32;
                let len = buf.get_uint(3) as u32;
                let typ = buf.get_u8();
                let sid = buf.get_u32_le(); // Stream ID is little-endian!
                (ts, len, typ, sid)
            }
            1 => {
                // No stream ID
                let ts = buf.get_uint(3) as u32;
                let len = buf.get_uint(3) as u32;
                let typ = buf.get_u8();
                (ts, len, typ, state.stream_id)
            }
            2 => {
                // Timestamp delta only
                let ts = buf.get_uint(3) as u32;
                (ts, state.message_length, state.message_type, state.stream_id)
            }
            3 => {
                // No header, use previous values
                (state.timestamp_delta, state.message_length, state.message_type, state.stream_id)
            }
            _ => unreachable!(),
        };

        // Handle extended timestamp
        let timestamp = if timestamp_field >= EXTENDED_TIMESTAMP_THRESHOLD || (fmt == 3 && state.has_extended_timestamp) {
            if buf.len() < 4 {
                return Ok(None);
            }
            state.has_extended_timestamp = true;
            buf.get_u32()
        } else {
            state.has_extended_timestamp = false;
            timestamp_field
        };

        // Update state
        let absolute_timestamp = if fmt == 0 {
            timestamp
        } else {
            state.timestamp.wrapping_add(timestamp)
        };

        state.timestamp_delta = timestamp;
        state.message_length = message_length;
        state.message_type = message_type;
        state.stream_id = stream_id;
        state.timestamp = absolute_timestamp;

        // Validate message size
        if message_length > self.max_message_size {
            return Err(ProtocolError::MessageTooLarge {
                size: message_length,
                max: self.max_message_size,
            }.into());
        }

        // Initialize reassembly buffer if this is a new message
        if state.partial_message.is_empty() {
            state.expected_length = message_length;
            state.partial_message.reserve(message_length as usize);
        }

        // Calculate how much data to read for this chunk
        let remaining = state.expected_length - state.partial_message.len() as u32;
        let chunk_data_len = remaining.min(self.chunk_size) as usize;

        if buf.len() < chunk_data_len {
            return Ok(None); // Need more chunk data
        }

        // Read chunk data
        state.partial_message.put_slice(&buf[..chunk_data_len]);
        buf.advance(chunk_data_len);

        // Check if message is complete
        if state.partial_message.len() as u32 >= state.expected_length {
            let payload = state.partial_message.split().freeze();
            state.expected_length = 0;

            Ok(Some(RtmpChunk {
                csid,
                timestamp: state.timestamp,
                message_type: state.message_type,
                stream_id: state.stream_id,
                payload,
            }))
        } else {
            Ok(None) // Message not yet complete
        }
    }

    /// Parse basic header and return (fmt, csid, header_length)
    fn parse_basic_header(&self, buf: &[u8]) -> Result<Option<(u8, u32, usize)>> {
        if buf.is_empty() {
            return Ok(None);
        }

        let first = buf[0];
        let fmt = (first >> 6) & 0x03;
        let csid_low = first & 0x3F;

        match csid_low {
            0 => {
                // 2-byte header: csid = 64 + second byte
                if buf.len() < 2 {
                    return Ok(None);
                }
                let csid = 64 + buf[1] as u32;
                Ok(Some((fmt, csid, 2)))
            }
            1 => {
                // 3-byte header: csid = 64 + second + third*256
                if buf.len() < 3 {
                    return Ok(None);
                }
                let csid = 64 + buf[1] as u32 + (buf[2] as u32) * 256;
                Ok(Some((fmt, csid, 3)))
            }
            _ => {
                // 1-byte header: csid = 2-63
                Ok(Some((fmt, csid_low as u32, 1)))
            }
        }
    }

    /// Abort a message on a chunk stream (when receiving Abort message)
    pub fn abort(&mut self, csid: u32) {
        if let Some(state) = self.streams.get_mut(&csid) {
            state.partial_message.clear();
            state.expected_length = 0;
        }
    }
}

impl Default for ChunkDecoder {
    fn default() -> Self {
        Self::new()
    }
}

/// Chunk stream encoder
///
/// Encodes messages into chunks for transmission.
pub struct ChunkEncoder {
    /// Outgoing chunk size
    chunk_size: u32,
    /// Per-chunk-stream state for compression
    streams: HashMap<u32, ChunkStreamState>,
}

impl ChunkEncoder {
    /// Create a new encoder with default chunk size
    pub fn new() -> Self {
        Self {
            chunk_size: DEFAULT_CHUNK_SIZE,
            streams: HashMap::new(),
        }
    }

    /// Set the chunk size (call before encoding to use larger chunks)
    pub fn set_chunk_size(&mut self, size: u32) {
        self.chunk_size = size.min(MAX_CHUNK_SIZE);
    }

    /// Get current chunk size
    pub fn chunk_size(&self) -> u32 {
        self.chunk_size
    }

    /// Encode a message into chunks
    pub fn encode(&mut self, chunk: &RtmpChunk, buf: &mut BytesMut) {
        let csid = chunk.csid;
        let chunk_size = self.chunk_size;

        // Get or create state, and compute format based on current state
        let state = self.streams.entry(csid).or_default();

        // Compute format based on state comparison
        let fmt = select_format(chunk, state);

        // Determine if we need extended timestamp
        let needs_extended = chunk.timestamp >= EXTENDED_TIMESTAMP_THRESHOLD;
        let timestamp_field = if needs_extended {
            EXTENDED_TIMESTAMP_THRESHOLD
        } else {
            chunk.timestamp
        };

        let timestamp_delta = chunk.timestamp.wrapping_sub(state.timestamp);
        let delta_field = if needs_extended {
            EXTENDED_TIMESTAMP_THRESHOLD
        } else {
            timestamp_delta
        };

        let had_extended_timestamp = state.has_extended_timestamp;

        // Update state before encoding
        state.timestamp = chunk.timestamp;
        state.timestamp_delta = timestamp_delta;
        state.message_length = chunk.payload.len() as u32;
        state.message_type = chunk.message_type;
        state.stream_id = chunk.stream_id;
        state.has_extended_timestamp = needs_extended;

        // Encode chunks
        let mut offset = 0;
        let payload_len = chunk.payload.len();
        let mut first_chunk = true;

        while offset < payload_len {
            let chunk_data_len = (payload_len - offset).min(chunk_size as usize);

            // Write basic header
            write_basic_header(csid, if first_chunk { fmt } else { 3 }, buf);

            // Write message header based on format
            if first_chunk {
                match fmt {
                    0 => {
                        // Full header
                        write_u24(timestamp_field, buf);
                        write_u24(payload_len as u32, buf);
                        buf.put_u8(chunk.message_type);
                        buf.put_u32_le(chunk.stream_id);
                    }
                    1 => {
                        // No stream ID
                        write_u24(delta_field, buf);
                        write_u24(payload_len as u32, buf);
                        buf.put_u8(chunk.message_type);
                    }
                    2 => {
                        // Timestamp delta only
                        write_u24(delta_field, buf);
                    }
                    3 => {
                        // No header
                    }
                    _ => unreachable!(),
                }
            }

            // Write extended timestamp if needed
            if needs_extended && (first_chunk || had_extended_timestamp) {
                buf.put_u32(chunk.timestamp);
            }

            // Write chunk data
            buf.put_slice(&chunk.payload[offset..offset + chunk_data_len]);
            offset += chunk_data_len;
            first_chunk = false;
        }
    }

}

/// Select the best header format for compression
fn select_format(chunk: &RtmpChunk, state: &ChunkStreamState) -> u8 {
    // First message on this stream must use format 0
    if state.message_type == 0 && state.stream_id == 0 {
        return 0;
    }

    // If stream ID differs, must use format 0
    if chunk.stream_id != state.stream_id {
        return 0;
    }

    // If message type or length differs, use format 1
    if chunk.message_type != state.message_type
        || chunk.payload.len() as u32 != state.message_length
    {
        return 1;
    }

    // If timestamp delta matches previous, use format 3
    let delta = chunk.timestamp.wrapping_sub(state.timestamp);
    if delta == state.timestamp_delta {
        return 3;
    }

    // Otherwise use format 2 (timestamp delta only)
    2
}

/// Write basic header
fn write_basic_header(csid: u32, fmt: u8, buf: &mut BytesMut) {
    if csid >= 64 + 256 {
        // 3-byte header
        buf.put_u8((fmt << 6) | 1);
        let csid_offset = csid - 64;
        buf.put_u8((csid_offset & 0xFF) as u8);
        buf.put_u8(((csid_offset >> 8) & 0xFF) as u8);
    } else if csid >= 64 {
        // 2-byte header
        buf.put_u8((fmt << 6) | 0);
        buf.put_u8((csid - 64) as u8);
    } else {
        // 1-byte header
        buf.put_u8((fmt << 6) | (csid as u8));
    }
}

/// Write 24-bit big-endian value
fn write_u24(value: u32, buf: &mut BytesMut) {
    buf.put_u8(((value >> 16) & 0xFF) as u8);
    buf.put_u8(((value >> 8) & 0xFF) as u8);
    buf.put_u8((value & 0xFF) as u8);
}

impl Default for ChunkEncoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_header_parsing() {
        let decoder = ChunkDecoder::new();

        // 1-byte header (csid 2-63)
        let buf = [0x03]; // fmt=0, csid=3
        let result = decoder.parse_basic_header(&buf).unwrap().unwrap();
        assert_eq!(result, (0, 3, 1));

        // 2-byte header (csid 64-319)
        let buf = [0x00, 0x00]; // fmt=0, csid=64
        let result = decoder.parse_basic_header(&buf).unwrap().unwrap();
        assert_eq!(result, (0, 64, 2));

        // 3-byte header (csid 64-65599)
        let buf = [0x01, 0x00, 0x01]; // fmt=0, csid=64+256
        let result = decoder.parse_basic_header(&buf).unwrap().unwrap();
        assert_eq!(result, (0, 320, 3));
    }

    #[test]
    fn test_encode_decode_roundtrip() {
        let original = RtmpChunk {
            csid: CSID_COMMAND,
            timestamp: 1000,
            message_type: MSG_COMMAND_AMF0,
            stream_id: 0,
            payload: Bytes::from_static(b"test payload data"),
        };

        let mut encoder = ChunkEncoder::new();
        let mut decoder = ChunkDecoder::new();

        let mut encoded = BytesMut::new();
        encoder.encode(&original, &mut encoded);

        let decoded = decoder.decode(&mut encoded).unwrap().unwrap();

        assert_eq!(decoded.csid, original.csid);
        assert_eq!(decoded.timestamp, original.timestamp);
        assert_eq!(decoded.message_type, original.message_type);
        assert_eq!(decoded.stream_id, original.stream_id);
        assert_eq!(decoded.payload, original.payload);
    }

    #[test]
    fn test_large_message_chunking() {
        let large_payload = vec![0u8; 500]; // Larger than default chunk size

        let original = RtmpChunk {
            csid: CSID_VIDEO,
            timestamp: 0,
            message_type: MSG_VIDEO,
            stream_id: 1,
            payload: Bytes::from(large_payload.clone()),
        };

        let mut encoder = ChunkEncoder::new();
        let mut decoder = ChunkDecoder::new();

        let mut encoded = BytesMut::new();
        encoder.encode(&original, &mut encoded);

        // Should produce multiple chunks
        assert!(encoded.len() > 500);

        let decoded = decoder.decode(&mut encoded).unwrap().unwrap();
        assert_eq!(decoded.payload.len(), 500);
    }
}
