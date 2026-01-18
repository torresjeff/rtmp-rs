//! Unified error types for rtmp-rs

use std::fmt;
use std::io;

/// Result type alias using the library's Error type
pub type Result<T> = std::result::Result<T, Error>;

/// Unified error type for all RTMP operations
#[derive(Debug)]
pub enum Error {
    /// I/O error during network operations
    Io(io::Error),
    /// RTMP protocol violation
    Protocol(ProtocolError),
    /// AMF encoding/decoding error
    Amf(AmfError),
    /// Handshake failure
    Handshake(HandshakeError),
    /// Media parsing error
    Media(MediaError),
    /// Connection rejected by peer or handler
    Rejected(String),
    /// Operation timed out
    Timeout,
    /// Connection was closed
    ConnectionClosed,
    /// Invalid configuration
    Config(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "I/O error: {}", e),
            Error::Protocol(e) => write!(f, "Protocol error: {}", e),
            Error::Amf(e) => write!(f, "AMF error: {}", e),
            Error::Handshake(e) => write!(f, "Handshake error: {}", e),
            Error::Media(e) => write!(f, "Media error: {}", e),
            Error::Rejected(msg) => write!(f, "Connection rejected: {}", msg),
            Error::Timeout => write!(f, "Operation timed out"),
            Error::ConnectionClosed => write!(f, "Connection closed"),
            Error::Config(msg) => write!(f, "Configuration error: {}", msg),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::Io(err)
    }
}

impl From<ProtocolError> for Error {
    fn from(err: ProtocolError) -> Self {
        Error::Protocol(err)
    }
}

impl From<AmfError> for Error {
    fn from(err: AmfError) -> Self {
        Error::Amf(err)
    }
}

impl From<HandshakeError> for Error {
    fn from(err: HandshakeError) -> Self {
        Error::Handshake(err)
    }
}

impl From<MediaError> for Error {
    fn from(err: MediaError) -> Self {
        Error::Media(err)
    }
}

/// Protocol-level errors
#[derive(Debug)]
pub enum ProtocolError {
    InvalidChunkHeader,
    UnknownMessageType(u8),
    MessageTooLarge { size: u32, max: u32 },
    InvalidChunkStreamId(u32),
    UnexpectedMessage(String),
    MissingField(String),
    InvalidCommand(String),
    StreamNotFound(u32),
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolError::InvalidChunkHeader => write!(f, "Invalid chunk header"),
            ProtocolError::UnknownMessageType(t) => write!(f, "Unknown message type: {}", t),
            ProtocolError::MessageTooLarge { size, max } => {
                write!(f, "Message too large: {} bytes (max {})", size, max)
            }
            ProtocolError::InvalidChunkStreamId(id) => write!(f, "Invalid chunk stream ID: {}", id),
            ProtocolError::UnexpectedMessage(msg) => write!(f, "Unexpected message: {}", msg),
            ProtocolError::MissingField(field) => write!(f, "Missing required field: {}", field),
            ProtocolError::InvalidCommand(cmd) => write!(f, "Invalid command: {}", cmd),
            ProtocolError::StreamNotFound(id) => write!(f, "Stream not found: {}", id),
        }
    }
}

impl std::error::Error for ProtocolError {}

/// AMF encoding/decoding errors
#[derive(Debug)]
pub enum AmfError {
    UnknownMarker(u8),
    UnexpectedEof,
    InvalidUtf8,
    InvalidReference(u16),
    NestingTooDeep,
    InvalidObjectEnd,
}

impl fmt::Display for AmfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AmfError::UnknownMarker(m) => write!(f, "Unknown AMF marker: 0x{:02x}", m),
            AmfError::UnexpectedEof => write!(f, "Unexpected end of AMF data"),
            AmfError::InvalidUtf8 => write!(f, "Invalid UTF-8 in AMF string"),
            AmfError::InvalidReference(idx) => write!(f, "Invalid AMF reference: {}", idx),
            AmfError::NestingTooDeep => write!(f, "AMF nesting too deep"),
            AmfError::InvalidObjectEnd => write!(f, "Invalid object end marker"),
        }
    }
}

impl std::error::Error for AmfError {}

/// Handshake-specific errors
#[derive(Debug)]
pub enum HandshakeError {
    InvalidVersion(u8),
    DigestMismatch,
    InvalidState,
    ResponseMismatch,
}

impl fmt::Display for HandshakeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HandshakeError::InvalidVersion(v) => write!(f, "Invalid RTMP version: {}", v),
            HandshakeError::DigestMismatch => write!(f, "Handshake digest mismatch"),
            HandshakeError::InvalidState => write!(f, "Invalid handshake state"),
            HandshakeError::ResponseMismatch => write!(f, "Handshake response mismatch"),
        }
    }
}

impl std::error::Error for HandshakeError {}

/// Media parsing errors
#[derive(Debug)]
pub enum MediaError {
    InvalidFlvTag,
    InvalidAvcPacket,
    InvalidAacPacket,
    UnsupportedCodec(String),
    InvalidNalu,
    MissingSequenceHeader,
}

impl fmt::Display for MediaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MediaError::InvalidFlvTag => write!(f, "Invalid FLV tag"),
            MediaError::InvalidAvcPacket => write!(f, "Invalid AVC packet"),
            MediaError::InvalidAacPacket => write!(f, "Invalid AAC packet"),
            MediaError::UnsupportedCodec(c) => write!(f, "Unsupported codec: {}", c),
            MediaError::InvalidNalu => write!(f, "Invalid NAL unit"),
            MediaError::MissingSequenceHeader => write!(f, "Missing sequence header"),
        }
    }
}

impl std::error::Error for MediaError {}
