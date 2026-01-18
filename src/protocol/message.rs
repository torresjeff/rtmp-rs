//! RTMP message types and parsing
//!
//! RTMP messages are classified into:
//! - Protocol Control Messages (types 1-6): Chunk/flow control
//! - Command Messages (types 17, 20): AMF-encoded commands
//! - Data Messages (types 15, 18): Metadata
//! - Audio/Video Messages (types 8, 9): Media data
//!
//! Reference: RTMP Specification Section 5.4

use bytes::{Buf, BufMut, Bytes, BytesMut};
use std::collections::HashMap;

use crate::amf::{Amf0Decoder, Amf0Encoder, AmfValue};
use crate::error::{AmfError, ProtocolError, Result};
use crate::protocol::chunk::RtmpChunk;
use crate::protocol::constants::*;

/// Parsed RTMP message
#[derive(Debug, Clone)]
pub enum RtmpMessage {
    /// Set Chunk Size (type 1)
    SetChunkSize(u32),

    /// Abort Message (type 2)
    Abort { csid: u32 },

    /// Acknowledgement (type 3)
    Acknowledgement { sequence: u32 },

    /// User Control Message (type 4)
    UserControl(UserControlEvent),

    /// Window Acknowledgement Size (type 5)
    WindowAckSize(u32),

    /// Set Peer Bandwidth (type 6)
    SetPeerBandwidth { size: u32, limit_type: u8 },

    /// Audio data (type 8)
    Audio { timestamp: u32, data: Bytes },

    /// Video data (type 9)
    Video { timestamp: u32, data: Bytes },

    /// AMF0 Command (type 20)
    Command(Command),

    /// AMF0 Data message (type 18) - metadata, etc.
    Data(DataMessage),

    /// AMF3 Command (type 17)
    CommandAmf3(Command),

    /// AMF3 Data message (type 15)
    DataAmf3(DataMessage),

    /// Aggregate message (type 22)
    Aggregate { data: Bytes },

    /// Unknown message type
    Unknown { type_id: u8, data: Bytes },
}

/// User Control Event
#[derive(Debug, Clone)]
pub enum UserControlEvent {
    StreamBegin(u32),
    StreamEof(u32),
    StreamDry(u32),
    SetBufferLength { stream_id: u32, buffer_ms: u32 },
    StreamIsRecorded(u32),
    PingRequest(u32),
    PingResponse(u32),
    Unknown { event_type: u16, data: Bytes },
}

/// RTMP command (connect, publish, play, etc.)
#[derive(Debug, Clone)]
pub struct Command {
    /// Command name
    pub name: String,
    /// Transaction ID
    pub transaction_id: f64,
    /// Command object (often null for responses)
    pub command_object: AmfValue,
    /// Additional arguments
    pub arguments: Vec<AmfValue>,
    /// Message stream ID (from chunk)
    pub stream_id: u32,
}

/// Data message (@setDataFrame, onMetaData, etc.)
#[derive(Debug, Clone)]
pub struct DataMessage {
    /// Handler name (e.g., "@setDataFrame", "onMetaData")
    pub name: String,
    /// Data values
    pub values: Vec<AmfValue>,
    /// Message stream ID
    pub stream_id: u32,
}

/// Connect command parameters
#[derive(Debug, Clone, Default)]
pub struct ConnectParams {
    /// Application name
    pub app: String,
    /// Flash version
    pub flash_ver: Option<String>,
    /// SWF URL
    pub swf_url: Option<String>,
    /// TC URL (full RTMP URL)
    pub tc_url: Option<String>,
    /// Is FPAD
    pub fpad: bool,
    /// Audio codecs
    pub audio_codecs: u32,
    /// Video codecs
    pub video_codecs: u32,
    /// Video function
    pub video_function: u32,
    /// Page URL
    pub page_url: Option<String>,
    /// Object encoding (AMF version)
    pub object_encoding: f64,
    /// Extra properties from connect object
    pub extra: HashMap<String, AmfValue>,
}

impl ConnectParams {
    /// Parse from AMF command object
    pub fn from_amf(obj: &AmfValue) -> Self {
        let mut params = ConnectParams::default();

        if let Some(map) = obj.as_object() {
            for (key, value) in map {
                match key.as_str() {
                    "app" => {
                        if let Some(s) = value.as_str() {
                            params.app = s.to_string();
                        }
                    }
                    "flashVer" | "flashver" => {
                        params.flash_ver = value.as_str().map(|s| s.to_string());
                    }
                    "swfUrl" | "swfurl" => {
                        params.swf_url = value.as_str().map(|s| s.to_string());
                    }
                    "tcUrl" | "tcurl" => {
                        params.tc_url = value.as_str().map(|s| s.to_string());
                    }
                    "fpad" => {
                        params.fpad = value.as_bool().unwrap_or(false);
                    }
                    "audioCodecs" | "audiocodecs" => {
                        params.audio_codecs = value.as_number().unwrap_or(0.0) as u32;
                    }
                    "videoCodecs" | "videocodecs" => {
                        params.video_codecs = value.as_number().unwrap_or(0.0) as u32;
                    }
                    "videoFunction" | "videofunction" => {
                        params.video_function = value.as_number().unwrap_or(0.0) as u32;
                    }
                    "pageUrl" | "pageurl" => {
                        params.page_url = value.as_str().map(|s| s.to_string());
                    }
                    "objectEncoding" | "objectencoding" => {
                        params.object_encoding = value.as_number().unwrap_or(0.0);
                    }
                    _ => {
                        params.extra.insert(key.clone(), value.clone());
                    }
                }
            }
        }

        params
    }
}

/// Publish command parameters
#[derive(Debug, Clone)]
pub struct PublishParams {
    /// Stream key (name)
    pub stream_key: String,
    /// Publish type: "live", "record", "append"
    pub publish_type: String,
    /// Message stream ID
    pub stream_id: u32,
}

/// Play command parameters
#[derive(Debug, Clone)]
pub struct PlayParams {
    /// Stream name
    pub stream_name: String,
    /// Start time (-2 = live, -1 = live or recorded, >= 0 = specific time)
    pub start: f64,
    /// Duration (-1 = until end)
    pub duration: f64,
    /// Reset flag
    pub reset: bool,
    /// Message stream ID
    pub stream_id: u32,
}

impl RtmpMessage {
    /// Parse a message from a chunk
    pub fn from_chunk(chunk: &RtmpChunk) -> Result<Self> {
        let mut payload = chunk.payload.clone();

        match chunk.message_type {
            MSG_SET_CHUNK_SIZE => {
                if payload.len() < 4 {
                    return Err(ProtocolError::InvalidChunkHeader.into());
                }
                let size = payload.get_u32() & 0x7FFFFFFF; // Ignore MSB
                Ok(RtmpMessage::SetChunkSize(size))
            }

            MSG_ABORT => {
                if payload.len() < 4 {
                    return Err(ProtocolError::InvalidChunkHeader.into());
                }
                Ok(RtmpMessage::Abort { csid: payload.get_u32() })
            }

            MSG_ACKNOWLEDGEMENT => {
                if payload.len() < 4 {
                    return Err(ProtocolError::InvalidChunkHeader.into());
                }
                Ok(RtmpMessage::Acknowledgement { sequence: payload.get_u32() })
            }

            MSG_USER_CONTROL => {
                Self::parse_user_control(&mut payload)
            }

            MSG_WINDOW_ACK_SIZE => {
                if payload.len() < 4 {
                    return Err(ProtocolError::InvalidChunkHeader.into());
                }
                Ok(RtmpMessage::WindowAckSize(payload.get_u32()))
            }

            MSG_SET_PEER_BANDWIDTH => {
                if payload.len() < 5 {
                    return Err(ProtocolError::InvalidChunkHeader.into());
                }
                let size = payload.get_u32();
                let limit_type = payload.get_u8();
                Ok(RtmpMessage::SetPeerBandwidth { size, limit_type })
            }

            MSG_AUDIO => {
                Ok(RtmpMessage::Audio {
                    timestamp: chunk.timestamp,
                    data: payload,
                })
            }

            MSG_VIDEO => {
                Ok(RtmpMessage::Video {
                    timestamp: chunk.timestamp,
                    data: payload,
                })
            }

            MSG_COMMAND_AMF0 => {
                let cmd = Self::parse_command(&mut payload, chunk.stream_id)?;
                Ok(RtmpMessage::Command(cmd))
            }

            MSG_COMMAND_AMF3 => {
                // Skip AMF3 marker byte if present
                if !payload.is_empty() && payload[0] == 0x00 {
                    payload.advance(1);
                }
                let cmd = Self::parse_command(&mut payload, chunk.stream_id)?;
                Ok(RtmpMessage::CommandAmf3(cmd))
            }

            MSG_DATA_AMF0 => {
                let data = Self::parse_data(&mut payload, chunk.stream_id)?;
                Ok(RtmpMessage::Data(data))
            }

            MSG_DATA_AMF3 => {
                if !payload.is_empty() && payload[0] == 0x00 {
                    payload.advance(1);
                }
                let data = Self::parse_data(&mut payload, chunk.stream_id)?;
                Ok(RtmpMessage::DataAmf3(data))
            }

            MSG_AGGREGATE => {
                Ok(RtmpMessage::Aggregate { data: payload })
            }

            _ => {
                Ok(RtmpMessage::Unknown {
                    type_id: chunk.message_type,
                    data: payload,
                })
            }
        }
    }

    /// Parse User Control message
    fn parse_user_control(payload: &mut Bytes) -> Result<Self> {
        if payload.len() < 6 {
            return Err(ProtocolError::InvalidChunkHeader.into());
        }

        let event_type = payload.get_u16();
        let event = match event_type {
            UC_STREAM_BEGIN => UserControlEvent::StreamBegin(payload.get_u32()),
            UC_STREAM_EOF => UserControlEvent::StreamEof(payload.get_u32()),
            UC_STREAM_DRY => UserControlEvent::StreamDry(payload.get_u32()),
            UC_SET_BUFFER_LENGTH => {
                if payload.len() < 8 {
                    return Err(ProtocolError::InvalidChunkHeader.into());
                }
                let stream_id = payload.get_u32();
                let buffer_ms = payload.get_u32();
                UserControlEvent::SetBufferLength { stream_id, buffer_ms }
            }
            UC_STREAM_IS_RECORDED => UserControlEvent::StreamIsRecorded(payload.get_u32()),
            UC_PING_REQUEST => UserControlEvent::PingRequest(payload.get_u32()),
            UC_PING_RESPONSE => UserControlEvent::PingResponse(payload.get_u32()),
            _ => UserControlEvent::Unknown {
                event_type,
                data: payload.clone(),
            },
        };

        Ok(RtmpMessage::UserControl(event))
    }

    /// Parse AMF0 command
    fn parse_command(payload: &mut Bytes, stream_id: u32) -> Result<Command> {
        let mut decoder = Amf0Decoder::new();

        // Command name
        let name = match decoder.decode(payload)? {
            AmfValue::String(s) => s,
            _ => return Err(ProtocolError::InvalidCommand("Expected command name".into()).into()),
        };

        // Transaction ID
        let transaction_id = match decoder.decode(payload)? {
            AmfValue::Number(n) => n,
            _ => 0.0, // Lenient: default to 0
        };

        // Command object (can be null)
        let command_object = if payload.has_remaining() {
            decoder.decode(payload)?
        } else {
            AmfValue::Null
        };

        // Additional arguments
        let mut arguments = Vec::new();
        while payload.has_remaining() {
            match decoder.decode(payload) {
                Ok(v) => arguments.push(v),
                Err(AmfError::UnexpectedEof) => break,
                Err(e) => return Err(e.into()),
            }
        }

        Ok(Command {
            name,
            transaction_id,
            command_object,
            arguments,
            stream_id,
        })
    }

    /// Parse AMF0 data message
    fn parse_data(payload: &mut Bytes, stream_id: u32) -> Result<DataMessage> {
        let mut decoder = Amf0Decoder::new();

        // Handler name
        let name = match decoder.decode(payload)? {
            AmfValue::String(s) => s,
            _ => String::new(), // Lenient
        };

        // Data values
        let mut values = Vec::new();
        while payload.has_remaining() {
            match decoder.decode(payload) {
                Ok(v) => values.push(v),
                Err(AmfError::UnexpectedEof) => break,
                Err(e) => return Err(e.into()),
            }
        }

        Ok(DataMessage { name, values, stream_id })
    }

    /// Encode message to chunk payload
    pub fn encode(&self) -> (u8, Bytes) {
        match self {
            RtmpMessage::SetChunkSize(size) => {
                let mut buf = BytesMut::with_capacity(4);
                buf.put_u32(*size);
                (MSG_SET_CHUNK_SIZE, buf.freeze())
            }

            RtmpMessage::Abort { csid } => {
                let mut buf = BytesMut::with_capacity(4);
                buf.put_u32(*csid);
                (MSG_ABORT, buf.freeze())
            }

            RtmpMessage::Acknowledgement { sequence } => {
                let mut buf = BytesMut::with_capacity(4);
                buf.put_u32(*sequence);
                (MSG_ACKNOWLEDGEMENT, buf.freeze())
            }

            RtmpMessage::WindowAckSize(size) => {
                let mut buf = BytesMut::with_capacity(4);
                buf.put_u32(*size);
                (MSG_WINDOW_ACK_SIZE, buf.freeze())
            }

            RtmpMessage::SetPeerBandwidth { size, limit_type } => {
                let mut buf = BytesMut::with_capacity(5);
                buf.put_u32(*size);
                buf.put_u8(*limit_type);
                (MSG_SET_PEER_BANDWIDTH, buf.freeze())
            }

            RtmpMessage::UserControl(event) => {
                let mut buf = BytesMut::with_capacity(10);
                match event {
                    UserControlEvent::StreamBegin(id) => {
                        buf.put_u16(UC_STREAM_BEGIN);
                        buf.put_u32(*id);
                    }
                    UserControlEvent::StreamEof(id) => {
                        buf.put_u16(UC_STREAM_EOF);
                        buf.put_u32(*id);
                    }
                    UserControlEvent::StreamDry(id) => {
                        buf.put_u16(UC_STREAM_DRY);
                        buf.put_u32(*id);
                    }
                    UserControlEvent::SetBufferLength { stream_id, buffer_ms } => {
                        buf.put_u16(UC_SET_BUFFER_LENGTH);
                        buf.put_u32(*stream_id);
                        buf.put_u32(*buffer_ms);
                    }
                    UserControlEvent::StreamIsRecorded(id) => {
                        buf.put_u16(UC_STREAM_IS_RECORDED);
                        buf.put_u32(*id);
                    }
                    UserControlEvent::PingRequest(ts) => {
                        buf.put_u16(UC_PING_REQUEST);
                        buf.put_u32(*ts);
                    }
                    UserControlEvent::PingResponse(ts) => {
                        buf.put_u16(UC_PING_RESPONSE);
                        buf.put_u32(*ts);
                    }
                    UserControlEvent::Unknown { event_type, data } => {
                        buf.put_u16(*event_type);
                        buf.put_slice(data);
                    }
                }
                (MSG_USER_CONTROL, buf.freeze())
            }

            RtmpMessage::Audio { data, .. } => {
                (MSG_AUDIO, data.clone())
            }

            RtmpMessage::Video { data, .. } => {
                (MSG_VIDEO, data.clone())
            }

            RtmpMessage::Command(cmd) => {
                let payload = encode_command(cmd);
                (MSG_COMMAND_AMF0, payload)
            }

            RtmpMessage::CommandAmf3(cmd) => {
                let mut buf = BytesMut::new();
                buf.put_u8(0x00); // AMF3 marker
                buf.put_slice(&encode_command(cmd));
                (MSG_COMMAND_AMF3, buf.freeze())
            }

            RtmpMessage::Data(data) => {
                let payload = encode_data(data);
                (MSG_DATA_AMF0, payload)
            }

            RtmpMessage::DataAmf3(data) => {
                let mut buf = BytesMut::new();
                buf.put_u8(0x00);
                buf.put_slice(&encode_data(data));
                (MSG_DATA_AMF3, buf.freeze())
            }

            RtmpMessage::Aggregate { data } => {
                (MSG_AGGREGATE, data.clone())
            }

            RtmpMessage::Unknown { type_id, data } => {
                (*type_id, data.clone())
            }
        }
    }
}

/// Encode a command to AMF0 bytes
fn encode_command(cmd: &Command) -> Bytes {
    let mut encoder = Amf0Encoder::new();
    encoder.encode(&AmfValue::String(cmd.name.clone()));
    encoder.encode(&AmfValue::Number(cmd.transaction_id));
    encoder.encode(&cmd.command_object);
    for arg in &cmd.arguments {
        encoder.encode(arg);
    }
    encoder.finish()
}

/// Encode a data message to AMF0 bytes
fn encode_data(data: &DataMessage) -> Bytes {
    let mut encoder = Amf0Encoder::new();
    encoder.encode(&AmfValue::String(data.name.clone()));
    for value in &data.values {
        encoder.encode(value);
    }
    encoder.finish()
}

/// Build common response messages
impl Command {
    /// Create a _result response
    pub fn result(transaction_id: f64, properties: AmfValue, info: AmfValue) -> Self {
        Command {
            name: CMD_RESULT.to_string(),
            transaction_id,
            command_object: properties,
            arguments: vec![info],
            stream_id: 0,
        }
    }

    /// Create an _error response
    pub fn error(transaction_id: f64, properties: AmfValue, info: AmfValue) -> Self {
        Command {
            name: CMD_ERROR.to_string(),
            transaction_id,
            command_object: properties,
            arguments: vec![info],
            stream_id: 0,
        }
    }

    /// Create an onStatus response
    pub fn on_status(stream_id: u32, level: &str, code: &str, description: &str) -> Self {
        let mut info = HashMap::new();
        info.insert("level".to_string(), AmfValue::String(level.to_string()));
        info.insert("code".to_string(), AmfValue::String(code.to_string()));
        info.insert("description".to_string(), AmfValue::String(description.to_string()));

        Command {
            name: CMD_ON_STATUS.to_string(),
            transaction_id: 0.0,
            command_object: AmfValue::Null,
            arguments: vec![AmfValue::Object(info)],
            stream_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connect_params_parsing() {
        let mut obj = HashMap::new();
        obj.insert("app".to_string(), AmfValue::String("live".into()));
        obj.insert("tcUrl".to_string(), AmfValue::String("rtmp://localhost/live".into()));
        obj.insert("objectEncoding".to_string(), AmfValue::Number(0.0));

        let params = ConnectParams::from_amf(&AmfValue::Object(obj));
        assert_eq!(params.app, "live");
        assert_eq!(params.tc_url, Some("rtmp://localhost/live".into()));
        assert_eq!(params.object_encoding, 0.0);
    }

    #[test]
    fn test_command_roundtrip() {
        let cmd = Command {
            name: "connect".to_string(),
            transaction_id: 1.0,
            command_object: AmfValue::Null,
            arguments: vec![AmfValue::String("test".into())],
            stream_id: 0,
        };

        let payload = encode_command(&cmd);
        let chunk = RtmpChunk {
            csid: CSID_COMMAND,
            timestamp: 0,
            message_type: MSG_COMMAND_AMF0,
            stream_id: 0,
            payload,
        };

        let parsed = RtmpMessage::from_chunk(&chunk).unwrap();
        if let RtmpMessage::Command(parsed_cmd) = parsed {
            assert_eq!(parsed_cmd.name, "connect");
            assert_eq!(parsed_cmd.transaction_id, 1.0);
        } else {
            panic!("Expected Command message");
        }
    }
}
