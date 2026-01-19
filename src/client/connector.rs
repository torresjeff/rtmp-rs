//! RTMP client connector
//!
//! Low-level client for connecting to RTMP servers.

use std::collections::HashMap;

use bytes::{Buf, Bytes, BytesMut};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::net::TcpStream;
use tokio::time::timeout;

use crate::amf::AmfValue;
use crate::error::{Error, Result};
use crate::protocol::chunk::{ChunkDecoder, ChunkEncoder, RtmpChunk};
use crate::protocol::constants::*;
use crate::protocol::handshake::{Handshake, HandshakeRole};
use crate::protocol::message::{Command, RtmpMessage};

use super::config::{ClientConfig, ParsedUrl};

/// RTMP client connector
pub struct RtmpConnector {
    config: ClientConfig,
    parsed_url: ParsedUrl,
    reader: BufReader<tokio::io::ReadHalf<TcpStream>>,
    writer: BufWriter<tokio::io::WriteHalf<TcpStream>>,
    read_buf: BytesMut,
    write_buf: BytesMut,
    chunk_decoder: ChunkDecoder,
    chunk_encoder: ChunkEncoder,
    stream_id: u32,
}

impl RtmpConnector {
    /// Connect to an RTMP server
    pub async fn connect(config: ClientConfig) -> Result<Self> {
        let parsed_url = config.parse_url()
            .ok_or_else(|| Error::Config("Invalid RTMP URL".into()))?;

        let addr = format!("{}:{}", parsed_url.host, parsed_url.port);

        let socket = timeout(config.connect_timeout, TcpStream::connect(&addr))
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(Error::Io)?;

        if config.tcp_nodelay {
            socket.set_nodelay(true)?;
        }

        let (read_half, write_half) = tokio::io::split(socket);

        let mut connector = Self {
            config,
            parsed_url,
            reader: BufReader::with_capacity(64 * 1024, read_half),
            writer: BufWriter::with_capacity(64 * 1024, write_half),
            read_buf: BytesMut::with_capacity(64 * 1024),
            write_buf: BytesMut::with_capacity(64 * 1024),
            chunk_decoder: ChunkDecoder::new(),
            chunk_encoder: ChunkEncoder::new(),
            stream_id: 0,
        };

        connector.do_handshake().await?;
        connector.do_connect().await?;

        Ok(connector)
    }

    /// Perform handshake
    async fn do_handshake(&mut self) -> Result<()> {
        let mut handshake = Handshake::new(HandshakeRole::Client);

        // Send C0C1
        let c0c1 = handshake.generate_initial()
            .ok_or(Error::Protocol(crate::error::ProtocolError::InvalidChunkHeader))?;
        self.writer.write_all(&c0c1).await?;
        self.writer.flush().await?;

        // Wait for S0S1S2 and send C2
        let timeout_duration = self.config.connect_timeout;
        timeout(timeout_duration, async {
            loop {
                let n = self.reader.read_buf(&mut self.read_buf).await?;
                if n == 0 {
                    return Err(Error::ConnectionClosed);
                }

                let mut buf = Bytes::copy_from_slice(&self.read_buf);
                if let Some(response) = handshake.process(&mut buf)? {
                    let consumed = self.read_buf.len() - buf.len();
                    self.read_buf.advance(consumed);

                    self.writer.write_all(&response).await?;
                    self.writer.flush().await?;
                }

                if handshake.is_done() {
                    break;
                }
            }
            Ok::<_, Error>(())
        }).await.map_err(|_| Error::Timeout)??;

        Ok(())
    }

    /// Send connect command
    async fn do_connect(&mut self) -> Result<()> {
        let mut obj = HashMap::new();
        obj.insert("app".to_string(), AmfValue::String(self.parsed_url.app.clone()));
        obj.insert("type".to_string(), AmfValue::String("nonprivate".into()));
        obj.insert("flashVer".to_string(), AmfValue::String(self.config.flash_ver.clone()));
        obj.insert("tcUrl".to_string(), AmfValue::String(self.config.url.clone()));
        obj.insert("fpad".to_string(), AmfValue::Boolean(false));
        obj.insert("capabilities".to_string(), AmfValue::Number(15.0));
        obj.insert("audioCodecs".to_string(), AmfValue::Number(3191.0));
        obj.insert("videoCodecs".to_string(), AmfValue::Number(252.0));
        obj.insert("videoFunction".to_string(), AmfValue::Number(1.0));

        let cmd = Command {
            name: CMD_CONNECT.to_string(),
            transaction_id: 1.0,
            command_object: AmfValue::Object(obj),
            arguments: vec![],
            stream_id: 0,
        };

        self.send_command(&cmd).await?;

        // Wait for connect result
        loop {
            let msg = self.read_message().await?;
            match msg {
                RtmpMessage::Command(cmd) if cmd.name == CMD_RESULT => {
                    // Connect successful
                    break;
                }
                RtmpMessage::Command(cmd) if cmd.name == CMD_ERROR => {
                    return Err(Error::Rejected("Connect rejected".into()));
                }
                RtmpMessage::SetChunkSize(size) => {
                    self.chunk_decoder.set_chunk_size(size);
                }
                RtmpMessage::WindowAckSize(_) | RtmpMessage::SetPeerBandwidth { .. } => {
                    // Ignore these during connect
                }
                _ => {}
            }
        }

        // Set our chunk size
        self.chunk_encoder.set_chunk_size(RECOMMENDED_CHUNK_SIZE);
        self.send_message(&RtmpMessage::SetChunkSize(RECOMMENDED_CHUNK_SIZE)).await?;

        Ok(())
    }

    /// Create a stream for publishing or playing
    pub async fn create_stream(&mut self) -> Result<u32> {
        let cmd = Command {
            name: CMD_CREATE_STREAM.to_string(),
            transaction_id: 2.0,
            command_object: AmfValue::Null,
            arguments: vec![],
            stream_id: 0,
        };

        self.send_command(&cmd).await?;

        // Wait for result
        loop {
            let msg = self.read_message().await?;
            if let RtmpMessage::Command(result) = msg {
                if result.name == CMD_RESULT && result.transaction_id == 2.0 {
                    if let Some(id) = result.arguments.first().and_then(|v| v.as_number()) {
                        self.stream_id = id as u32;
                        return Ok(self.stream_id);
                    }
                }
            }
        }
    }

    /// Start playing a stream
    pub async fn play(&mut self, stream_name: &str) -> Result<()> {
        if self.stream_id == 0 {
            self.create_stream().await?;
        }

        // Set buffer length
        self.send_message(&RtmpMessage::UserControl(
            crate::protocol::message::UserControlEvent::SetBufferLength {
                stream_id: self.stream_id,
                buffer_ms: self.config.buffer_length,
            }
        )).await?;

        // Send play command
        let cmd = Command {
            name: CMD_PLAY.to_string(),
            transaction_id: 0.0,
            command_object: AmfValue::Null,
            arguments: vec![
                AmfValue::String(stream_name.to_string()),
                AmfValue::Number(-2.0), // Start: live or recorded
                AmfValue::Number(-1.0), // Duration: play until end
                AmfValue::Boolean(true), // Reset
            ],
            stream_id: self.stream_id,
        };

        self.send_command(&cmd).await?;

        // Wait for onStatus
        loop {
            let msg = self.read_message().await?;
            if let RtmpMessage::Command(status) = msg {
                if status.name == CMD_ON_STATUS {
                    if let Some(info) = status.arguments.first().and_then(|v| v.as_object()) {
                        if let Some(code) = info.get("code").and_then(|v| v.as_str()) {
                            if code == NS_PLAY_START {
                                return Ok(());
                            } else if code.contains("Failed") || code.contains("Error") {
                                return Err(Error::Rejected(code.to_string()));
                            }
                        }
                    }
                }
            }
        }
    }

    /// Read the next RTMP message
    pub async fn read_message(&mut self) -> Result<RtmpMessage> {
        loop {
            // Try to decode from buffer
            while let Some(chunk) = self.chunk_decoder.decode(&mut self.read_buf)? {
                return RtmpMessage::from_chunk(&chunk);
            }

            // Need more data
            let n = self.reader.read_buf(&mut self.read_buf).await?;
            if n == 0 {
                return Err(Error::ConnectionClosed);
            }
        }
    }

    /// Send an RTMP message
    async fn send_message(&mut self, msg: &RtmpMessage) -> Result<()> {
        let (msg_type, payload) = msg.encode();

        let csid = match msg {
            RtmpMessage::SetChunkSize(_)
            | RtmpMessage::WindowAckSize(_)
            | RtmpMessage::SetPeerBandwidth { .. }
            | RtmpMessage::UserControl(_) => CSID_PROTOCOL_CONTROL,
            RtmpMessage::Command(_) | RtmpMessage::CommandAmf3(_) => CSID_COMMAND,
            _ => CSID_COMMAND,
        };

        let chunk = RtmpChunk {
            csid,
            timestamp: 0,
            message_type: msg_type,
            stream_id: 0,
            payload,
        };

        self.write_buf.clear();
        self.chunk_encoder.encode(&chunk, &mut self.write_buf);
        self.writer.write_all(&self.write_buf).await?;
        self.writer.flush().await?;

        Ok(())
    }

    /// Send a command
    async fn send_command(&mut self, cmd: &Command) -> Result<()> {
        self.send_message(&RtmpMessage::Command(cmd.clone())).await
    }

    /// Get the stream ID
    pub fn stream_id(&self) -> u32 {
        self.stream_id
    }
}
