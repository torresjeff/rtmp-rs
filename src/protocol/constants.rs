//! RTMP protocol constants
//!
//! Reference: Adobe RTMP Specification (December 2012)
//! Reference: RFC 7425 - Adobe's RTMP (Informational)

/// RTMP version number (always 3 for standard RTMP)
pub const RTMP_VERSION: u8 = 3;

/// Default RTMP port
pub const RTMP_PORT: u16 = 1935;

/// Handshake packet sizes
pub const HANDSHAKE_SIZE: usize = 1536;

/// Default chunk size (per RTMP spec)
pub const DEFAULT_CHUNK_SIZE: u32 = 128;

/// Recommended chunk size for efficiency (reduces header overhead)
pub const RECOMMENDED_CHUNK_SIZE: u32 = 4096;

/// Maximum chunk size allowed
pub const MAX_CHUNK_SIZE: u32 = 0xFFFFFF; // 16MB

/// Maximum message size (sanity limit)
pub const MAX_MESSAGE_SIZE: u32 = 16 * 1024 * 1024; // 16MB

/// Extended timestamp threshold
/// Timestamps >= this value require extended timestamp field
pub const EXTENDED_TIMESTAMP_THRESHOLD: u32 = 0xFFFFFF;

// ============================================================================
// Chunk Stream IDs (CSID)
// RTMP spec section 5.3.1.1
// ============================================================================

/// Protocol control messages (Set Chunk Size, Abort, etc.)
pub const CSID_PROTOCOL_CONTROL: u32 = 2;

/// Command messages (connect, createStream, etc.)
pub const CSID_COMMAND: u32 = 3;

/// Audio data
pub const CSID_AUDIO: u32 = 4;

/// Video data
pub const CSID_VIDEO: u32 = 6;

// ============================================================================
// Message Type IDs
// RTMP spec section 5.4
// ============================================================================

/// Set Chunk Size (1) - protocol control
pub const MSG_SET_CHUNK_SIZE: u8 = 1;

/// Abort Message (2) - protocol control
pub const MSG_ABORT: u8 = 2;

/// Acknowledgement (3) - protocol control
pub const MSG_ACKNOWLEDGEMENT: u8 = 3;

/// User Control Message (4) - protocol control
pub const MSG_USER_CONTROL: u8 = 4;

/// Window Acknowledgement Size (5) - protocol control
pub const MSG_WINDOW_ACK_SIZE: u8 = 5;

/// Set Peer Bandwidth (6) - protocol control
pub const MSG_SET_PEER_BANDWIDTH: u8 = 6;

/// Audio Message (8)
pub const MSG_AUDIO: u8 = 8;

/// Video Message (9)
pub const MSG_VIDEO: u8 = 9;

/// AMF3 Data Message (15) - @setDataFrame with AMF3
pub const MSG_DATA_AMF3: u8 = 15;

/// AMF3 Shared Object (16)
pub const MSG_SHARED_OBJECT_AMF3: u8 = 16;

/// AMF3 Command Message (17)
pub const MSG_COMMAND_AMF3: u8 = 17;

/// AMF0 Data Message (18) - @setDataFrame, onMetaData
pub const MSG_DATA_AMF0: u8 = 18;

/// AMF0 Shared Object (19)
pub const MSG_SHARED_OBJECT_AMF0: u8 = 19;

/// AMF0 Command Message (20) - connect, play, publish, etc.
pub const MSG_COMMAND_AMF0: u8 = 20;

/// Aggregate Message (22)
pub const MSG_AGGREGATE: u8 = 22;

// ============================================================================
// User Control Event Types
// RTMP spec section 5.4.1
// ============================================================================

/// Stream Begin - server sends when stream becomes functional
pub const UC_STREAM_BEGIN: u16 = 0;

/// Stream EOF - server sends when playback ends
pub const UC_STREAM_EOF: u16 = 1;

/// Stream Dry - no more data available
pub const UC_STREAM_DRY: u16 = 2;

/// Set Buffer Length - client tells server buffer size
pub const UC_SET_BUFFER_LENGTH: u16 = 3;

/// Stream Is Recorded - stream is recorded
pub const UC_STREAM_IS_RECORDED: u16 = 4;

/// Ping Request - server pings client
pub const UC_PING_REQUEST: u16 = 6;

/// Ping Response - client responds to ping
pub const UC_PING_RESPONSE: u16 = 7;

// ============================================================================
// Peer Bandwidth Limit Types
// RTMP spec section 5.4.5
// ============================================================================

/// Hard limit - peer should limit output to this bandwidth
pub const BANDWIDTH_LIMIT_HARD: u8 = 0;

/// Soft limit - peer can exceed if it has excess bandwidth
pub const BANDWIDTH_LIMIT_SOFT: u8 = 1;

/// Dynamic - can be hard or soft depending on prior state
pub const BANDWIDTH_LIMIT_DYNAMIC: u8 = 2;

// ============================================================================
// Common Command Names
// ============================================================================

pub const CMD_CONNECT: &str = "connect";
pub const CMD_CALL: &str = "call";
pub const CMD_CLOSE: &str = "close";
pub const CMD_CREATE_STREAM: &str = "createStream";
pub const CMD_DELETE_STREAM: &str = "deleteStream";
pub const CMD_PLAY: &str = "play";
pub const CMD_PLAY2: &str = "play2";
pub const CMD_PUBLISH: &str = "publish";
pub const CMD_PAUSE: &str = "pause";
pub const CMD_SEEK: &str = "seek";
pub const CMD_RECEIVE_AUDIO: &str = "receiveAudio";
pub const CMD_RECEIVE_VIDEO: &str = "receiveVideo";

/// Internal response commands
pub const CMD_RESULT: &str = "_result";
pub const CMD_ERROR: &str = "_error";

/// Status notification
pub const CMD_ON_STATUS: &str = "onStatus";

// OBS/Twitch extended commands
pub const CMD_FC_PUBLISH: &str = "FCPublish";
pub const CMD_FC_UNPUBLISH: &str = "FCUnpublish";
pub const CMD_RELEASE_STREAM: &str = "releaseStream";
pub const CMD_ON_FC_PUBLISH: &str = "onFCPublish";
pub const CMD_ON_FC_UNPUBLISH: &str = "onFCUnpublish";

// Data commands
pub const CMD_SET_DATA_FRAME: &str = "@setDataFrame";
pub const CMD_ON_METADATA: &str = "onMetaData";

// ============================================================================
// NetConnection Status Codes
// ============================================================================

pub const NC_CONNECT_SUCCESS: &str = "NetConnection.Connect.Success";
pub const NC_CONNECT_REJECTED: &str = "NetConnection.Connect.Rejected";
pub const NC_CONNECT_FAILED: &str = "NetConnection.Connect.Failed";
pub const NC_CONNECT_CLOSED: &str = "NetConnection.Connect.Closed";

// ============================================================================
// NetStream Status Codes
// ============================================================================

pub const NS_PUBLISH_START: &str = "NetStream.Publish.Start";
pub const NS_PUBLISH_BAD_NAME: &str = "NetStream.Publish.BadName";
pub const NS_PLAY_START: &str = "NetStream.Play.Start";
pub const NS_PLAY_RESET: &str = "NetStream.Play.Reset";
pub const NS_PLAY_STOP: &str = "NetStream.Play.Stop";
pub const NS_PLAY_STREAM_NOT_FOUND: &str = "NetStream.Play.StreamNotFound";
pub const NS_PAUSE_NOTIFY: &str = "NetStream.Pause.Notify";
pub const NS_UNPAUSE_NOTIFY: &str = "NetStream.Unpause.Notify";

// ============================================================================
// Default Server Settings
// ============================================================================

/// Default window acknowledgement size (2.5 MB)
pub const DEFAULT_WINDOW_ACK_SIZE: u32 = 2_500_000;

/// Default peer bandwidth (2.5 MB)
pub const DEFAULT_PEER_BANDWIDTH: u32 = 2_500_000;

/// Default buffer length in milliseconds
pub const DEFAULT_BUFFER_LENGTH: u32 = 1000;

// ============================================================================
// Chunk Header Format Types (fmt field)
// RTMP spec section 5.3.1.2
// ============================================================================

/// Type 0: Full header (11 bytes) - timestamp, length, type, stream ID
pub const CHUNK_FMT_0: u8 = 0;

/// Type 1: No stream ID (7 bytes) - timestamp delta, length, type
pub const CHUNK_FMT_1: u8 = 1;

/// Type 2: No stream ID, length, type (3 bytes) - timestamp delta only
pub const CHUNK_FMT_2: u8 = 2;

/// Type 3: No header (0 bytes) - use previous chunk's values
pub const CHUNK_FMT_3: u8 = 3;
