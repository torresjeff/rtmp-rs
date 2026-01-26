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
    /// Invalid enhanced video packet (E-RTMP)
    InvalidEnhancedVideoPacket,
    /// Invalid enhanced audio packet (E-RTMP)
    InvalidEnhancedAudioPacket,
    /// Unsupported video codec FOURCC
    UnsupportedVideoCodec,
    /// Unsupported audio codec FOURCC
    UnsupportedAudioCodec,
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
            MediaError::InvalidEnhancedVideoPacket => write!(f, "Invalid enhanced video packet"),
            MediaError::InvalidEnhancedAudioPacket => write!(f, "Invalid enhanced audio packet"),
            MediaError::UnsupportedVideoCodec => write!(f, "Unsupported video codec FOURCC"),
            MediaError::UnsupportedAudioCodec => write!(f, "Unsupported audio codec FOURCC"),
        }
    }
}

impl std::error::Error for MediaError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as StdError;
    use std::io;

    #[test]
    fn test_error_display() {
        // Test Error::Io display
        let io_err = io::Error::new(io::ErrorKind::ConnectionReset, "connection reset");
        let err = Error::Io(io_err);
        assert!(err.to_string().contains("I/O error"));

        // Test Error::Protocol display
        let err = Error::Protocol(ProtocolError::InvalidChunkHeader);
        assert!(err.to_string().contains("Protocol error"));
        assert!(err.to_string().contains("Invalid chunk header"));

        // Test Error::Amf display
        let err = Error::Amf(AmfError::UnknownMarker(0xFF));
        assert!(err.to_string().contains("AMF error"));
        assert!(err.to_string().contains("0xff"));

        // Test Error::Handshake display
        let err = Error::Handshake(HandshakeError::InvalidVersion(5));
        assert!(err.to_string().contains("Handshake error"));
        assert!(err.to_string().contains("5"));

        // Test Error::Media display
        let err = Error::Media(MediaError::UnsupportedCodec("VP9".into()));
        assert!(err.to_string().contains("Media error"));
        assert!(err.to_string().contains("VP9"));

        // Test Error::Rejected display
        let err = Error::Rejected("stream key invalid".into());
        assert!(err.to_string().contains("Connection rejected"));
        assert!(err.to_string().contains("stream key invalid"));

        // Test Error::Timeout display
        let err = Error::Timeout;
        assert!(err.to_string().contains("timed out"));

        // Test Error::ConnectionClosed display
        let err = Error::ConnectionClosed;
        assert!(err.to_string().contains("closed"));

        // Test Error::Config display
        let err = Error::Config("invalid port".into());
        assert!(err.to_string().contains("Configuration error"));
    }

    #[test]
    fn test_error_source() {
        // Only Io error should have a source
        let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let err = Error::Io(io_err);
        assert!(StdError::source(&err).is_some());

        // Other errors should not have a source
        let err = Error::Protocol(ProtocolError::InvalidChunkHeader);
        assert!(StdError::source(&err).is_none());

        let err = Error::Timeout;
        assert!(StdError::source(&err).is_none());
    }

    #[test]
    fn test_from_conversions() {
        // Test From<io::Error>
        let io_err = io::Error::new(io::ErrorKind::TimedOut, "timeout");
        let err: Error = io_err.into();
        assert!(matches!(err, Error::Io(_)));

        // Test From<ProtocolError>
        let proto_err = ProtocolError::MessageTooLarge { size: 100, max: 50 };
        let err: Error = proto_err.into();
        assert!(matches!(err, Error::Protocol(_)));

        // Test From<AmfError>
        let amf_err = AmfError::UnexpectedEof;
        let err: Error = amf_err.into();
        assert!(matches!(err, Error::Amf(_)));

        // Test From<HandshakeError>
        let hs_err = HandshakeError::DigestMismatch;
        let err: Error = hs_err.into();
        assert!(matches!(err, Error::Handshake(_)));

        // Test From<MediaError>
        let media_err = MediaError::InvalidFlvTag;
        let err: Error = media_err.into();
        assert!(matches!(err, Error::Media(_)));
    }

    #[test]
    fn test_protocol_error_display() {
        assert!(ProtocolError::InvalidChunkHeader
            .to_string()
            .contains("Invalid chunk header"));

        assert!(ProtocolError::UnknownMessageType(99)
            .to_string()
            .contains("99"));

        let err = ProtocolError::MessageTooLarge {
            size: 1000,
            max: 500,
        };
        assert!(err.to_string().contains("1000"));
        assert!(err.to_string().contains("500"));

        assert!(ProtocolError::InvalidChunkStreamId(123)
            .to_string()
            .contains("123"));

        assert!(ProtocolError::UnexpectedMessage("test".into())
            .to_string()
            .contains("test"));

        assert!(ProtocolError::MissingField("app".into())
            .to_string()
            .contains("app"));

        assert!(ProtocolError::InvalidCommand("bad".into())
            .to_string()
            .contains("bad"));

        assert!(ProtocolError::StreamNotFound(5).to_string().contains("5"));
    }

    #[test]
    fn test_amf_error_display() {
        assert!(AmfError::UnknownMarker(0xAB).to_string().contains("0xab"));

        assert!(AmfError::UnexpectedEof.to_string().contains("end of AMF"));

        assert!(AmfError::InvalidUtf8.to_string().contains("UTF-8"));

        assert!(AmfError::InvalidReference(42).to_string().contains("42"));

        assert!(AmfError::NestingTooDeep.to_string().contains("deep"));

        assert!(AmfError::InvalidObjectEnd.to_string().contains("end"));
    }

    #[test]
    fn test_handshake_error_display() {
        assert!(HandshakeError::InvalidVersion(10)
            .to_string()
            .contains("10"));

        assert!(HandshakeError::DigestMismatch
            .to_string()
            .contains("digest"));

        assert!(HandshakeError::InvalidState.to_string().contains("state"));

        assert!(HandshakeError::ResponseMismatch
            .to_string()
            .contains("response"));
    }

    #[test]
    fn test_media_error_display() {
        assert!(MediaError::InvalidFlvTag.to_string().contains("FLV"));
        assert!(MediaError::InvalidAvcPacket.to_string().contains("AVC"));
        assert!(MediaError::InvalidAacPacket.to_string().contains("AAC"));
        assert!(MediaError::UnsupportedCodec("HEVC".into())
            .to_string()
            .contains("HEVC"));
        assert!(MediaError::InvalidNalu.to_string().contains("NAL"));
        assert!(MediaError::MissingSequenceHeader
            .to_string()
            .contains("sequence"));
        assert!(MediaError::InvalidEnhancedVideoPacket
            .to_string()
            .contains("enhanced video"));
        assert!(MediaError::InvalidEnhancedAudioPacket
            .to_string()
            .contains("enhanced audio"));
        assert!(MediaError::UnsupportedVideoCodec
            .to_string()
            .contains("video codec"));
        assert!(MediaError::UnsupportedAudioCodec
            .to_string()
            .contains("audio codec"));
    }
}
