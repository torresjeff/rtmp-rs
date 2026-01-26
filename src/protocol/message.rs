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
use crate::media::fourcc::{AudioFourCc, VideoFourCc};
use crate::protocol::chunk::RtmpChunk;
use crate::protocol::constants::*;
use crate::protocol::enhanced::{CapsEx, EnhancedCapabilities, FourCcCapability};

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

    // =========================================================================
    // Enhanced RTMP (E-RTMP) fields
    // =========================================================================
    /// List of supported FOURCC codec strings (e.g., ["avc1", "hvc1", "av01"])
    ///
    /// This is an alternative to the info maps; if present, all listed codecs
    /// are assumed to have full capability (decode + encode + forward).
    pub fourcc_list: Option<Vec<String>>,

    /// Video codec capabilities by FOURCC string.
    ///
    /// Maps FOURCC strings (e.g., "avc1", "hvc1") to capability bitmask:
    /// - 0x01: Can decode
    /// - 0x02: Can encode
    /// - 0x04: Can forward/relay
    pub video_fourcc_info_map: Option<HashMap<String, u32>>,

    /// Audio codec capabilities by FOURCC string.
    ///
    /// Maps FOURCC strings (e.g., "mp4a", "Opus") to capability bitmask.
    pub audio_fourcc_info_map: Option<HashMap<String, u32>>,

    /// Extended capabilities bitmask (E-RTMP capsEx field).
    ///
    /// - 0x01: Reconnect support
    /// - 0x02: Multitrack support
    /// - 0x04: ModEx signal parsing
    /// - 0x08: Nanosecond timestamp offset
    pub caps_ex: Option<u32>,
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
                    // E-RTMP fields
                    "fourCcList" => {
                        params.fourcc_list = Self::parse_fourcc_list(value);
                    }
                    "videoFourCcInfoMap" => {
                        params.video_fourcc_info_map = Self::parse_fourcc_info_map(value);
                    }
                    "audioFourCcInfoMap" => {
                        params.audio_fourcc_info_map = Self::parse_fourcc_info_map(value);
                    }
                    "capsEx" => {
                        params.caps_ex = value.as_number().map(|n| n as u32);
                    }
                    _ => {
                        params.extra.insert(key.clone(), value.clone());
                    }
                }
            }
        }

        params
    }

    /// Parse a fourCcList array from AMF value.
    fn parse_fourcc_list(value: &AmfValue) -> Option<Vec<String>> {
        if let AmfValue::Array(arr) = value {
            let list: Vec<String> = arr
                .iter()
                .filter_map(|v: &AmfValue| v.as_str().map(|s| s.to_string()))
                .collect();
            if list.is_empty() {
                None
            } else {
                Some(list)
            }
        } else {
            None
        }
    }

    /// Parse a FOURCC info map (videoFourCcInfoMap/audioFourCcInfoMap) from AMF value.
    fn parse_fourcc_info_map(value: &AmfValue) -> Option<HashMap<String, u32>> {
        if let Some(map) = value.as_object() {
            let info_map: HashMap<String, u32> = map
                .iter()
                .filter_map(|(k, v)| v.as_number().map(|n| (k.clone(), n as u32)))
                .collect();
            if info_map.is_empty() {
                None
            } else {
                Some(info_map)
            }
        } else {
            None
        }
    }

    /// Check if this connect request includes E-RTMP capabilities.
    ///
    /// Returns true if any E-RTMP fields are present (fourCcList, info maps, or capsEx).
    pub fn has_enhanced_rtmp(&self) -> bool {
        self.fourcc_list.is_some()
            || self.video_fourcc_info_map.is_some()
            || self.audio_fourcc_info_map.is_some()
            || self.caps_ex.is_some()
    }

    /// Get the CapsEx flags if present.
    pub fn caps_ex_flags(&self) -> CapsEx {
        CapsEx::from_bits(self.caps_ex.unwrap_or(0))
    }

    /// Convert E-RTMP fields to EnhancedCapabilities for negotiation.
    ///
    /// This extracts the client's declared capabilities from the connect params.
    pub fn to_enhanced_capabilities(&self) -> EnhancedCapabilities {
        if !self.has_enhanced_rtmp() {
            return EnhancedCapabilities::new();
        }

        let mut caps = EnhancedCapabilities {
            enabled: true,
            caps_ex: self.caps_ex_flags(),
            video_codecs: HashMap::new(),
            audio_codecs: HashMap::new(),
            ..Default::default()
        };

        // Parse video FOURCC info map
        if let Some(map) = &self.video_fourcc_info_map {
            for (fourcc_str, capability_bits) in map {
                if let Some(fourcc) = VideoFourCc::from_fourcc_str(fourcc_str) {
                    caps.video_codecs
                        .insert(fourcc, FourCcCapability::from_bits(*capability_bits));
                }
            }
        }

        // Parse audio FOURCC info map
        if let Some(map) = &self.audio_fourcc_info_map {
            for (fourcc_str, capability_bits) in map {
                if let Some(fourcc) = AudioFourCc::from_fourcc_str(fourcc_str) {
                    caps.audio_codecs
                        .insert(fourcc, FourCcCapability::from_bits(*capability_bits));
                }
            }
        }

        // If only fourCcList is provided (no info maps), treat all as full capability
        if let Some(list) = &self.fourcc_list {
            if self.video_fourcc_info_map.is_none() && self.audio_fourcc_info_map.is_none() {
                for fourcc_str in list {
                    // Try parsing as video codec
                    if let Some(fourcc) = VideoFourCc::from_fourcc_str(fourcc_str) {
                        caps.video_codecs
                            .entry(fourcc)
                            .or_insert(FourCcCapability::full());
                    }
                    // Try parsing as audio codec
                    if let Some(fourcc) = AudioFourCc::from_fourcc_str(fourcc_str) {
                        caps.audio_codecs
                            .entry(fourcc)
                            .or_insert(FourCcCapability::full());
                    }
                }
            }
        }

        caps
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
                Ok(RtmpMessage::Abort {
                    csid: payload.get_u32(),
                })
            }

            MSG_ACKNOWLEDGEMENT => {
                if payload.len() < 4 {
                    return Err(ProtocolError::InvalidChunkHeader.into());
                }
                Ok(RtmpMessage::Acknowledgement {
                    sequence: payload.get_u32(),
                })
            }

            MSG_USER_CONTROL => Self::parse_user_control(&mut payload),

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

            MSG_AUDIO => Ok(RtmpMessage::Audio {
                timestamp: chunk.timestamp,
                data: payload,
            }),

            MSG_VIDEO => Ok(RtmpMessage::Video {
                timestamp: chunk.timestamp,
                data: payload,
            }),

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

            MSG_AGGREGATE => Ok(RtmpMessage::Aggregate { data: payload }),

            _ => Ok(RtmpMessage::Unknown {
                type_id: chunk.message_type,
                data: payload,
            }),
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
                UserControlEvent::SetBufferLength {
                    stream_id,
                    buffer_ms,
                }
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

        Ok(DataMessage {
            name,
            values,
            stream_id,
        })
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
                    UserControlEvent::SetBufferLength {
                        stream_id,
                        buffer_ms,
                    } => {
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

            RtmpMessage::Audio { data, .. } => (MSG_AUDIO, data.clone()),

            RtmpMessage::Video { data, .. } => (MSG_VIDEO, data.clone()),

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

            RtmpMessage::Aggregate { data } => (MSG_AGGREGATE, data.clone()),

            RtmpMessage::Unknown { type_id, data } => (*type_id, data.clone()),
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
        info.insert(
            "description".to_string(),
            AmfValue::String(description.to_string()),
        );

        Command {
            name: CMD_ON_STATUS.to_string(),
            transaction_id: 0.0,
            command_object: AmfValue::Null,
            arguments: vec![AmfValue::Object(info)],
            stream_id,
        }
    }
}

/// Builder for connect response with E-RTMP capability negotiation.
///
/// Use this to construct a proper `_result` response to a connect command,
/// optionally including E-RTMP capability fields.
///
/// # Example
///
/// ```ignore
/// use rtmp_rs::protocol::message::ConnectResponseBuilder;
/// use rtmp_rs::protocol::enhanced::EnhancedCapabilities;
///
/// let caps = EnhancedCapabilities::with_defaults();
/// let response = ConnectResponseBuilder::new()
///     .fms_ver("rtmp-rs/0.5.0")
///     .capabilities(31)
///     .enhanced_capabilities(&caps)
///     .build(1.0);
/// ```
#[derive(Debug, Clone, Default)]
pub struct ConnectResponseBuilder {
    fms_ver: String,
    capabilities: u32,
    enhanced_caps: Option<EnhancedCapabilities>,
}

impl ConnectResponseBuilder {
    /// Create a new builder with default values.
    pub fn new() -> Self {
        Self {
            fms_ver: "FMS/3,5,7,7009".to_string(),
            capabilities: 31,
            enhanced_caps: None,
        }
    }

    /// Set the FMS version string.
    pub fn fms_ver(mut self, ver: impl Into<String>) -> Self {
        self.fms_ver = ver.into();
        self
    }

    /// Set the capabilities bitmask.
    pub fn capabilities(mut self, caps: u32) -> Self {
        self.capabilities = caps;
        self
    }

    /// Set E-RTMP enhanced capabilities.
    ///
    /// If the capabilities are enabled, the response will include
    /// E-RTMP fields (capsEx, videoFourCcInfoMap, audioFourCcInfoMap).
    pub fn enhanced_capabilities(mut self, caps: &EnhancedCapabilities) -> Self {
        if caps.enabled {
            self.enhanced_caps = Some(caps.clone());
        }
        self
    }

    /// Build the connect response command.
    pub fn build(self, transaction_id: f64) -> Command {
        let properties = self.build_properties();

        let mut info = HashMap::new();
        info.insert("level".to_string(), AmfValue::String("status".to_string()));
        info.insert(
            "code".to_string(),
            AmfValue::String(NC_CONNECT_SUCCESS.to_string()),
        );
        info.insert(
            "description".to_string(),
            AmfValue::String("Connection succeeded.".to_string()),
        );
        info.insert(
            "objectEncoding".to_string(),
            AmfValue::Number(0.0), // AMF0
        );

        Command::result(transaction_id, properties, AmfValue::Object(info))
    }

    /// Build the properties object for the connect response.
    fn build_properties(&self) -> AmfValue {
        let mut props = HashMap::new();
        props.insert("fmsVer".to_string(), AmfValue::String(self.fms_ver.clone()));
        props.insert(
            "capabilities".to_string(),
            AmfValue::Number(self.capabilities as f64),
        );

        // Add E-RTMP fields if enabled
        if let Some(caps) = &self.enhanced_caps {
            // capsEx
            props.insert(
                "capsEx".to_string(),
                AmfValue::Number(caps.caps_ex.bits() as f64),
            );

            // videoFourCcInfoMap
            if !caps.video_codecs.is_empty() {
                let video_map: HashMap<String, AmfValue> = caps
                    .video_codecs
                    .iter()
                    .map(|(fourcc, cap)| {
                        (
                            fourcc.as_fourcc_str().to_string(),
                            AmfValue::Number(cap.bits() as f64),
                        )
                    })
                    .collect();
                props.insert(
                    "videoFourCcInfoMap".to_string(),
                    AmfValue::Object(video_map),
                );
            }

            // audioFourCcInfoMap
            if !caps.audio_codecs.is_empty() {
                let audio_map: HashMap<String, AmfValue> = caps
                    .audio_codecs
                    .iter()
                    .map(|(fourcc, cap)| {
                        (
                            fourcc.as_fourcc_str().to_string(),
                            AmfValue::Number(cap.bits() as f64),
                        )
                    })
                    .collect();
                props.insert(
                    "audioFourCcInfoMap".to_string(),
                    AmfValue::Object(audio_map),
                );
            }
        }

        AmfValue::Object(props)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connect_params_parsing() {
        let mut obj = HashMap::new();
        obj.insert("app".to_string(), AmfValue::String("live".into()));
        obj.insert(
            "tcUrl".to_string(),
            AmfValue::String("rtmp://localhost/live".into()),
        );
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

    #[test]
    fn test_set_chunk_size_message() {
        let chunk = RtmpChunk {
            csid: CSID_PROTOCOL_CONTROL,
            timestamp: 0,
            message_type: MSG_SET_CHUNK_SIZE,
            stream_id: 0,
            payload: Bytes::from_static(&[0x00, 0x00, 0x10, 0x00]), // 4096
        };

        let msg = RtmpMessage::from_chunk(&chunk).unwrap();
        assert!(matches!(msg, RtmpMessage::SetChunkSize(4096)));

        // Test encoding
        let (msg_type, payload) = msg.encode();
        assert_eq!(msg_type, MSG_SET_CHUNK_SIZE);
        assert_eq!(&payload[..], &[0x00, 0x00, 0x10, 0x00]);
    }

    #[test]
    fn test_abort_message() {
        let chunk = RtmpChunk {
            csid: CSID_PROTOCOL_CONTROL,
            timestamp: 0,
            message_type: MSG_ABORT,
            stream_id: 0,
            payload: Bytes::from_static(&[0x00, 0x00, 0x00, 0x05]), // CSID 5
        };

        let msg = RtmpMessage::from_chunk(&chunk).unwrap();
        if let RtmpMessage::Abort { csid } = msg {
            assert_eq!(csid, 5);
        } else {
            panic!("Expected Abort message");
        }
    }

    #[test]
    fn test_acknowledgement_message() {
        let chunk = RtmpChunk {
            csid: CSID_PROTOCOL_CONTROL,
            timestamp: 0,
            message_type: MSG_ACKNOWLEDGEMENT,
            stream_id: 0,
            payload: Bytes::from_static(&[0x00, 0x10, 0x00, 0x00]), // 1048576
        };

        let msg = RtmpMessage::from_chunk(&chunk).unwrap();
        if let RtmpMessage::Acknowledgement { sequence } = msg {
            assert_eq!(sequence, 1048576);
        } else {
            panic!("Expected Acknowledgement message");
        }
    }

    #[test]
    fn test_window_ack_size_message() {
        let chunk = RtmpChunk {
            csid: CSID_PROTOCOL_CONTROL,
            timestamp: 0,
            message_type: MSG_WINDOW_ACK_SIZE,
            stream_id: 0,
            payload: Bytes::from_static(&[0x00, 0x26, 0x25, 0xA0]), // 2500000
        };

        let msg = RtmpMessage::from_chunk(&chunk).unwrap();
        if let RtmpMessage::WindowAckSize(size) = msg {
            assert_eq!(size, 2500000);
        } else {
            panic!("Expected WindowAckSize message");
        }
    }

    #[test]
    fn test_set_peer_bandwidth_message() {
        let chunk = RtmpChunk {
            csid: CSID_PROTOCOL_CONTROL,
            timestamp: 0,
            message_type: MSG_SET_PEER_BANDWIDTH,
            stream_id: 0,
            payload: Bytes::from_static(&[0x00, 0x26, 0x25, 0xA0, 0x02]), // 2500000, dynamic
        };

        let msg = RtmpMessage::from_chunk(&chunk).unwrap();
        if let RtmpMessage::SetPeerBandwidth { size, limit_type } = msg {
            assert_eq!(size, 2500000);
            assert_eq!(limit_type, BANDWIDTH_LIMIT_DYNAMIC);
        } else {
            panic!("Expected SetPeerBandwidth message");
        }
    }

    #[test]
    fn test_user_control_stream_begin() {
        let chunk = RtmpChunk {
            csid: CSID_PROTOCOL_CONTROL,
            timestamp: 0,
            message_type: MSG_USER_CONTROL,
            stream_id: 0,
            payload: Bytes::from_static(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x01]), // StreamBegin, stream 1
        };

        let msg = RtmpMessage::from_chunk(&chunk).unwrap();
        if let RtmpMessage::UserControl(UserControlEvent::StreamBegin(id)) = msg {
            assert_eq!(id, 1);
        } else {
            panic!("Expected StreamBegin user control");
        }
    }

    #[test]
    fn test_user_control_stream_eof() {
        let chunk = RtmpChunk {
            csid: CSID_PROTOCOL_CONTROL,
            timestamp: 0,
            message_type: MSG_USER_CONTROL,
            stream_id: 0,
            payload: Bytes::from_static(&[0x00, 0x01, 0x00, 0x00, 0x00, 0x02]), // StreamEof, stream 2
        };

        let msg = RtmpMessage::from_chunk(&chunk).unwrap();
        if let RtmpMessage::UserControl(UserControlEvent::StreamEof(id)) = msg {
            assert_eq!(id, 2);
        } else {
            panic!("Expected StreamEof user control");
        }
    }

    #[test]
    fn test_user_control_set_buffer_length() {
        let chunk = RtmpChunk {
            csid: CSID_PROTOCOL_CONTROL,
            timestamp: 0,
            message_type: MSG_USER_CONTROL,
            stream_id: 0,
            payload: Bytes::from_static(&[
                0x00, 0x03, // SetBufferLength
                0x00, 0x00, 0x00, 0x01, // stream_id 1
                0x00, 0x00, 0x03, 0xE8, // 1000ms
            ]),
        };

        let msg = RtmpMessage::from_chunk(&chunk).unwrap();
        if let RtmpMessage::UserControl(UserControlEvent::SetBufferLength {
            stream_id,
            buffer_ms,
        }) = msg
        {
            assert_eq!(stream_id, 1);
            assert_eq!(buffer_ms, 1000);
        } else {
            panic!("Expected SetBufferLength user control");
        }
    }

    #[test]
    fn test_user_control_ping_request() {
        let chunk = RtmpChunk {
            csid: CSID_PROTOCOL_CONTROL,
            timestamp: 0,
            message_type: MSG_USER_CONTROL,
            stream_id: 0,
            payload: Bytes::from_static(&[0x00, 0x06, 0x00, 0x01, 0x00, 0x00]), // PingRequest
        };

        let msg = RtmpMessage::from_chunk(&chunk).unwrap();
        if let RtmpMessage::UserControl(UserControlEvent::PingRequest(ts)) = msg {
            assert_eq!(ts, 0x00010000);
        } else {
            panic!("Expected PingRequest user control");
        }
    }

    #[test]
    fn test_user_control_ping_response() {
        let chunk = RtmpChunk {
            csid: CSID_PROTOCOL_CONTROL,
            timestamp: 0,
            message_type: MSG_USER_CONTROL,
            stream_id: 0,
            payload: Bytes::from_static(&[0x00, 0x07, 0x00, 0x00, 0x00, 0x64]), // PingResponse
        };

        let msg = RtmpMessage::from_chunk(&chunk).unwrap();
        if let RtmpMessage::UserControl(UserControlEvent::PingResponse(ts)) = msg {
            assert_eq!(ts, 100);
        } else {
            panic!("Expected PingResponse user control");
        }
    }

    #[test]
    fn test_audio_message() {
        let audio_data = Bytes::from_static(&[0xAF, 0x01, 0x21, 0x00, 0x00]);

        let chunk = RtmpChunk {
            csid: CSID_AUDIO,
            timestamp: 1000,
            message_type: MSG_AUDIO,
            stream_id: 1,
            payload: audio_data.clone(),
        };

        let msg = RtmpMessage::from_chunk(&chunk).unwrap();
        if let RtmpMessage::Audio { timestamp, data } = msg {
            assert_eq!(timestamp, 1000);
            assert_eq!(data, audio_data);
        } else {
            panic!("Expected Audio message");
        }
    }

    #[test]
    fn test_video_message() {
        let video_data = Bytes::from_static(&[0x17, 0x01, 0x00, 0x00, 0x00, 0x00]);

        let chunk = RtmpChunk {
            csid: CSID_VIDEO,
            timestamp: 2000,
            message_type: MSG_VIDEO,
            stream_id: 1,
            payload: video_data.clone(),
        };

        let msg = RtmpMessage::from_chunk(&chunk).unwrap();
        if let RtmpMessage::Video { timestamp, data } = msg {
            assert_eq!(timestamp, 2000);
            assert_eq!(data, video_data);
        } else {
            panic!("Expected Video message");
        }
    }

    #[test]
    fn test_data_message() {
        let mut encoder = Amf0Encoder::new();
        encoder.encode(&AmfValue::String("@setDataFrame".into()));
        encoder.encode(&AmfValue::String("onMetaData".into()));
        let mut metadata = HashMap::new();
        metadata.insert("width".to_string(), AmfValue::Number(1920.0));
        encoder.encode(&AmfValue::Object(metadata));

        let chunk = RtmpChunk {
            csid: CSID_COMMAND,
            timestamp: 0,
            message_type: MSG_DATA_AMF0,
            stream_id: 1,
            payload: encoder.finish(),
        };

        let msg = RtmpMessage::from_chunk(&chunk).unwrap();
        if let RtmpMessage::Data(data) = msg {
            assert_eq!(data.name, "@setDataFrame");
            assert_eq!(data.stream_id, 1);
            assert_eq!(data.values.len(), 2);
        } else {
            panic!("Expected Data message");
        }
    }

    #[test]
    fn test_unknown_message_type() {
        let chunk = RtmpChunk {
            csid: CSID_COMMAND,
            timestamp: 0,
            message_type: 99, // Unknown type
            stream_id: 0,
            payload: Bytes::from_static(b"unknown"),
        };

        let msg = RtmpMessage::from_chunk(&chunk).unwrap();
        if let RtmpMessage::Unknown { type_id, data } = msg {
            assert_eq!(type_id, 99);
            assert_eq!(data.as_ref(), b"unknown");
        } else {
            panic!("Expected Unknown message");
        }
    }

    #[test]
    fn test_command_result() {
        let mut props = HashMap::new();
        props.insert(
            "fmsVer".to_string(),
            AmfValue::String("FMS/3,5,7,7009".into()),
        );
        props.insert("capabilities".to_string(), AmfValue::Number(31.0));

        let result = Command::result(1.0, AmfValue::Object(props), AmfValue::Null);

        assert_eq!(result.name, "_result");
        assert_eq!(result.transaction_id, 1.0);
    }

    #[test]
    fn test_command_error() {
        let error = Command::error(1.0, AmfValue::Null, AmfValue::String("error".into()));

        assert_eq!(error.name, "_error");
        assert_eq!(error.transaction_id, 1.0);
    }

    #[test]
    fn test_command_on_status() {
        let status = Command::on_status(1, "status", NS_PUBLISH_START, "Publishing started");

        assert_eq!(status.name, "onStatus");
        assert_eq!(status.transaction_id, 0.0);
        assert_eq!(status.stream_id, 1);

        if let Some(info) = status.arguments.first() {
            if let AmfValue::Object(props) = info {
                assert_eq!(props.get("level").unwrap().as_str(), Some("status"));
                assert_eq!(props.get("code").unwrap().as_str(), Some(NS_PUBLISH_START));
            } else {
                panic!("Expected Object in arguments");
            }
        } else {
            panic!("Expected arguments");
        }
    }

    #[test]
    fn test_connect_params_all_fields() {
        let mut obj = HashMap::new();
        obj.insert("app".to_string(), AmfValue::String("live".into()));
        obj.insert(
            "flashVer".to_string(),
            AmfValue::String("OBS-Studio/29.0".into()),
        );
        obj.insert(
            "swfUrl".to_string(),
            AmfValue::String("rtmp://example.com/app".into()),
        );
        obj.insert(
            "tcUrl".to_string(),
            AmfValue::String("rtmp://example.com/live".into()),
        );
        obj.insert("fpad".to_string(), AmfValue::Boolean(false));
        obj.insert("audioCodecs".to_string(), AmfValue::Number(3575.0));
        obj.insert("videoCodecs".to_string(), AmfValue::Number(252.0));
        obj.insert("videoFunction".to_string(), AmfValue::Number(1.0));
        obj.insert(
            "pageUrl".to_string(),
            AmfValue::String("http://twitch.tv".into()),
        );
        obj.insert("objectEncoding".to_string(), AmfValue::Number(0.0));
        obj.insert("custom".to_string(), AmfValue::String("value".into()));

        let params = ConnectParams::from_amf(&AmfValue::Object(obj));

        assert_eq!(params.app, "live");
        assert_eq!(params.flash_ver, Some("OBS-Studio/29.0".into()));
        assert_eq!(params.swf_url, Some("rtmp://example.com/app".into()));
        assert_eq!(params.tc_url, Some("rtmp://example.com/live".into()));
        assert!(!params.fpad);
        assert_eq!(params.audio_codecs, 3575);
        assert_eq!(params.video_codecs, 252);
        assert_eq!(params.video_function, 1);
        assert_eq!(params.page_url, Some("http://twitch.tv".into()));
        assert_eq!(params.object_encoding, 0.0);
        assert!(params.extra.contains_key("custom"));
    }

    #[test]
    fn test_connect_params_case_insensitive() {
        // Test lowercase variants
        let mut obj = HashMap::new();
        obj.insert("flashver".to_string(), AmfValue::String("test".into()));
        obj.insert("tcurl".to_string(), AmfValue::String("url".into()));
        obj.insert("pageurl".to_string(), AmfValue::String("page".into()));
        obj.insert("swfurl".to_string(), AmfValue::String("swf".into()));

        let params = ConnectParams::from_amf(&AmfValue::Object(obj));

        assert_eq!(params.flash_ver, Some("test".into()));
        assert_eq!(params.tc_url, Some("url".into()));
        assert_eq!(params.page_url, Some("page".into()));
        assert_eq!(params.swf_url, Some("swf".into()));
    }

    #[test]
    fn test_connect_params_from_non_object() {
        // Should handle non-object gracefully
        let params = ConnectParams::from_amf(&AmfValue::Null);
        assert_eq!(params.app, "");
        assert!(params.flash_ver.is_none());
    }

    #[test]
    fn test_message_encode_roundtrip() {
        // Test various messages encode/decode roundtrip

        // SetChunkSize
        let msg = RtmpMessage::SetChunkSize(4096);
        let (msg_type, payload) = msg.encode();
        let chunk = RtmpChunk {
            csid: CSID_PROTOCOL_CONTROL,
            timestamp: 0,
            message_type: msg_type,
            stream_id: 0,
            payload,
        };
        let decoded = RtmpMessage::from_chunk(&chunk).unwrap();
        assert!(matches!(decoded, RtmpMessage::SetChunkSize(4096)));

        // WindowAckSize
        let msg = RtmpMessage::WindowAckSize(2500000);
        let (msg_type, payload) = msg.encode();
        let chunk = RtmpChunk {
            csid: CSID_PROTOCOL_CONTROL,
            timestamp: 0,
            message_type: msg_type,
            stream_id: 0,
            payload,
        };
        let decoded = RtmpMessage::from_chunk(&chunk).unwrap();
        assert!(matches!(decoded, RtmpMessage::WindowAckSize(2500000)));
    }

    #[test]
    fn test_user_control_event_encode() {
        // Test encoding of user control events
        let events = vec![
            RtmpMessage::UserControl(UserControlEvent::StreamBegin(1)),
            RtmpMessage::UserControl(UserControlEvent::StreamEof(2)),
            RtmpMessage::UserControl(UserControlEvent::StreamDry(3)),
            RtmpMessage::UserControl(UserControlEvent::StreamIsRecorded(4)),
            RtmpMessage::UserControl(UserControlEvent::PingRequest(5)),
            RtmpMessage::UserControl(UserControlEvent::PingResponse(6)),
            RtmpMessage::UserControl(UserControlEvent::SetBufferLength {
                stream_id: 1,
                buffer_ms: 1000,
            }),
        ];

        for msg in events {
            let (msg_type, payload) = msg.encode();
            assert_eq!(msg_type, MSG_USER_CONTROL);
            assert!(!payload.is_empty());
        }
    }

    #[test]
    fn test_aggregate_message() {
        let chunk = RtmpChunk {
            csid: CSID_VIDEO,
            timestamp: 0,
            message_type: MSG_AGGREGATE,
            stream_id: 1,
            payload: Bytes::from_static(b"aggregate data"),
        };

        let msg = RtmpMessage::from_chunk(&chunk).unwrap();
        if let RtmpMessage::Aggregate { data } = msg {
            assert_eq!(data.as_ref(), b"aggregate data");
        } else {
            panic!("Expected Aggregate message");
        }
    }

    #[test]
    fn test_truncated_protocol_control_messages() {
        // SetChunkSize with less than 4 bytes
        let chunk = RtmpChunk {
            csid: CSID_PROTOCOL_CONTROL,
            timestamp: 0,
            message_type: MSG_SET_CHUNK_SIZE,
            stream_id: 0,
            payload: Bytes::from_static(&[0x00, 0x00]), // Only 2 bytes
        };

        let result = RtmpMessage::from_chunk(&chunk);
        assert!(result.is_err());

        // WindowAckSize with less than 4 bytes
        let chunk = RtmpChunk {
            csid: CSID_PROTOCOL_CONTROL,
            timestamp: 0,
            message_type: MSG_WINDOW_ACK_SIZE,
            stream_id: 0,
            payload: Bytes::from_static(&[0x00]),
        };

        let result = RtmpMessage::from_chunk(&chunk);
        assert!(result.is_err());
    }

    // =========================================================================
    // E-RTMP Connect Tests
    // =========================================================================

    #[test]
    fn test_connect_params_ertmp_fields() {
        let mut obj = HashMap::new();
        obj.insert("app".to_string(), AmfValue::String("live".into()));

        // E-RTMP fields
        obj.insert("capsEx".to_string(), AmfValue::Number(3.0)); // RECONNECT | MULTITRACK

        let mut video_map = HashMap::new();
        video_map.insert("avc1".to_string(), AmfValue::Number(7.0)); // Full capability
        video_map.insert("hvc1".to_string(), AmfValue::Number(4.0)); // Forward only
        obj.insert(
            "videoFourCcInfoMap".to_string(),
            AmfValue::Object(video_map),
        );

        let mut audio_map = HashMap::new();
        audio_map.insert("mp4a".to_string(), AmfValue::Number(7.0));
        audio_map.insert("Opus".to_string(), AmfValue::Number(4.0));
        obj.insert(
            "audioFourCcInfoMap".to_string(),
            AmfValue::Object(audio_map),
        );

        let params = ConnectParams::from_amf(&AmfValue::Object(obj));

        assert!(params.has_enhanced_rtmp());
        assert_eq!(params.caps_ex, Some(3));
        assert!(params.video_fourcc_info_map.is_some());
        assert!(params.audio_fourcc_info_map.is_some());

        let video_map = params.video_fourcc_info_map.unwrap();
        assert_eq!(video_map.get("avc1"), Some(&7));
        assert_eq!(video_map.get("hvc1"), Some(&4));

        let audio_map = params.audio_fourcc_info_map.unwrap();
        assert_eq!(audio_map.get("mp4a"), Some(&7));
        assert_eq!(audio_map.get("Opus"), Some(&4));
    }

    #[test]
    fn test_connect_params_fourcc_list() {
        let mut obj = HashMap::new();
        obj.insert("app".to_string(), AmfValue::String("live".into()));

        // fourCcList as alternative to info maps
        obj.insert(
            "fourCcList".to_string(),
            AmfValue::Array(vec![
                AmfValue::String("avc1".into()),
                AmfValue::String("hvc1".into()),
                AmfValue::String("mp4a".into()),
            ]),
        );

        let params = ConnectParams::from_amf(&AmfValue::Object(obj));

        assert!(params.has_enhanced_rtmp());
        let list = params.fourcc_list.unwrap();
        assert_eq!(list.len(), 3);
        assert!(list.contains(&"avc1".to_string()));
        assert!(list.contains(&"hvc1".to_string()));
        assert!(list.contains(&"mp4a".to_string()));
    }

    #[test]
    fn test_connect_params_no_ertmp() {
        let mut obj = HashMap::new();
        obj.insert("app".to_string(), AmfValue::String("live".into()));

        let params = ConnectParams::from_amf(&AmfValue::Object(obj));

        assert!(!params.has_enhanced_rtmp());
        assert!(params.fourcc_list.is_none());
        assert!(params.video_fourcc_info_map.is_none());
        assert!(params.audio_fourcc_info_map.is_none());
        assert!(params.caps_ex.is_none());
    }

    #[test]
    fn test_connect_params_caps_ex_flags() {
        let mut obj = HashMap::new();
        obj.insert("app".to_string(), AmfValue::String("live".into()));
        obj.insert("capsEx".to_string(), AmfValue::Number(15.0)); // All flags

        let params = ConnectParams::from_amf(&AmfValue::Object(obj));
        let caps = params.caps_ex_flags();

        assert!(caps.supports_reconnect());
        assert!(caps.supports_multitrack());
        assert!(caps.supports_modex());
        assert!(caps.supports_timestamp_nano_offset());
    }

    #[test]
    fn test_connect_params_to_enhanced_capabilities() {
        let mut obj = HashMap::new();
        obj.insert("app".to_string(), AmfValue::String("live".into()));
        obj.insert("capsEx".to_string(), AmfValue::Number(6.0)); // MULTITRACK | MODEX

        let mut video_map = HashMap::new();
        video_map.insert("avc1".to_string(), AmfValue::Number(7.0));
        video_map.insert("av01".to_string(), AmfValue::Number(4.0));
        obj.insert(
            "videoFourCcInfoMap".to_string(),
            AmfValue::Object(video_map),
        );

        let mut audio_map = HashMap::new();
        audio_map.insert("Opus".to_string(), AmfValue::Number(7.0));
        obj.insert(
            "audioFourCcInfoMap".to_string(),
            AmfValue::Object(audio_map),
        );

        let params = ConnectParams::from_amf(&AmfValue::Object(obj));
        let caps = params.to_enhanced_capabilities();

        assert!(caps.enabled);
        assert!(caps.supports_multitrack());
        assert!(caps.caps_ex.supports_modex());
        assert!(!caps.caps_ex.supports_reconnect());

        assert!(caps.supports_video_codec(VideoFourCc::Avc));
        assert!(caps.supports_video_codec(VideoFourCc::Av1));
        assert!(!caps.supports_video_codec(VideoFourCc::Hevc));

        assert!(caps.supports_audio_codec(AudioFourCc::Opus));
        assert!(!caps.supports_audio_codec(AudioFourCc::Aac));
    }

    #[test]
    fn test_connect_params_fourcc_list_to_capabilities() {
        let mut obj = HashMap::new();
        obj.insert("app".to_string(), AmfValue::String("live".into()));

        // Only fourCcList (no info maps) -> full capability assumed
        obj.insert(
            "fourCcList".to_string(),
            AmfValue::Array(vec![
                AmfValue::String("avc1".into()),
                AmfValue::String("Opus".into()),
            ]),
        );

        let params = ConnectParams::from_amf(&AmfValue::Object(obj));
        let caps = params.to_enhanced_capabilities();

        assert!(caps.enabled);

        // Both should have full capability
        let avc_cap = caps.video_codec_capability(VideoFourCc::Avc).unwrap();
        assert!(avc_cap.can_decode());
        assert!(avc_cap.can_encode());
        assert!(avc_cap.can_forward());

        let opus_cap = caps.audio_codec_capability(AudioFourCc::Opus).unwrap();
        assert!(opus_cap.can_decode());
        assert!(opus_cap.can_encode());
        assert!(opus_cap.can_forward());
    }

    #[test]
    fn test_connect_response_builder_basic() {
        let response = ConnectResponseBuilder::new()
            .fms_ver("rtmp-rs/0.5.0")
            .capabilities(31)
            .build(1.0);

        assert_eq!(response.name, "_result");
        assert_eq!(response.transaction_id, 1.0);

        // Check properties
        if let AmfValue::Object(props) = &response.command_object {
            assert_eq!(props.get("fmsVer").unwrap().as_str(), Some("rtmp-rs/0.5.0"));
            assert_eq!(props.get("capabilities").unwrap().as_number(), Some(31.0));
            // No E-RTMP fields
            assert!(!props.contains_key("capsEx"));
            assert!(!props.contains_key("videoFourCcInfoMap"));
            assert!(!props.contains_key("audioFourCcInfoMap"));
        } else {
            panic!("Expected Object in command_object");
        }
    }

    #[test]
    fn test_connect_response_builder_with_ertmp() {
        let caps = EnhancedCapabilities::with_defaults();

        let response = ConnectResponseBuilder::new()
            .fms_ver("rtmp-rs/0.5.0")
            .capabilities(31)
            .enhanced_capabilities(&caps)
            .build(1.0);

        // Check properties include E-RTMP fields
        if let AmfValue::Object(props) = &response.command_object {
            assert!(props.contains_key("capsEx"));
            assert!(props.contains_key("videoFourCcInfoMap"));
            assert!(props.contains_key("audioFourCcInfoMap"));

            // Check video info map
            if let AmfValue::Object(video_map) = props.get("videoFourCcInfoMap").unwrap() {
                assert!(video_map.contains_key("avc1"));
                assert!(video_map.contains_key("hvc1"));
                assert!(video_map.contains_key("av01"));
            } else {
                panic!("Expected Object for videoFourCcInfoMap");
            }

            // Check audio info map
            if let AmfValue::Object(audio_map) = props.get("audioFourCcInfoMap").unwrap() {
                assert!(audio_map.contains_key("mp4a"));
                assert!(audio_map.contains_key("Opus"));
            } else {
                panic!("Expected Object for audioFourCcInfoMap");
            }
        } else {
            panic!("Expected Object in command_object");
        }
    }

    #[test]
    fn test_connect_response_builder_disabled_ertmp() {
        let caps = EnhancedCapabilities::new(); // disabled

        let response = ConnectResponseBuilder::new()
            .enhanced_capabilities(&caps)
            .build(1.0);

        // Should not include E-RTMP fields when disabled
        if let AmfValue::Object(props) = &response.command_object {
            assert!(!props.contains_key("capsEx"));
            assert!(!props.contains_key("videoFourCcInfoMap"));
            assert!(!props.contains_key("audioFourCcInfoMap"));
        } else {
            panic!("Expected Object in command_object");
        }
    }
}
