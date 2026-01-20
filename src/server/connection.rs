//! Per-connection RTMP handler
//!
//! Manages the lifecycle of a single RTMP connection:
//! 1. Handshake
//! 2. Connect command
//! 3. Stream commands (publish/play)
//! 4. Media handling
//! 5. Disconnect

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use bytes::{Bytes, BytesMut};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio::time::timeout;

use crate::registry::{BroadcastFrame, FrameType, StreamKey, StreamRegistry};

use crate::amf::AmfValue;
use crate::error::{Error, ProtocolError, Result};
use crate::media::flv::FlvTag;
use crate::media::{AacData, H264Data};
use crate::protocol::chunk::{ChunkDecoder, ChunkEncoder, RtmpChunk};
use crate::protocol::constants::*;
use crate::protocol::handshake::{Handshake, HandshakeRole};
use crate::protocol::message::{
    Command, ConnectParams, DataMessage, PlayParams, PublishParams, RtmpMessage, UserControlEvent,
};
use crate::protocol::quirks::EncoderType;
use crate::server::config::ServerConfig;
use crate::server::handler::{AuthResult, MediaDeliveryMode, RtmpHandler};
use crate::session::context::{SessionContext, StreamContext};
use crate::session::state::SessionState;

/// Subscriber state for backpressure handling
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubscriberState {
    /// Normal operation
    Normal,
    /// Skipping frames until next keyframe (due to lag)
    SkippingToKeyframe,
}

/// Per-connection handler
pub struct Connection<H: RtmpHandler> {
    /// Session state
    state: SessionState,

    /// Session context for callbacks
    context: SessionContext,

    /// TCP stream (buffered)
    reader: BufReader<tokio::io::ReadHalf<TcpStream>>,
    writer: BufWriter<tokio::io::WriteHalf<TcpStream>>,

    /// Read buffer
    read_buf: BytesMut,

    /// Chunk decoder/encoder
    chunk_decoder: ChunkDecoder,
    chunk_encoder: ChunkEncoder,

    /// Write buffer for outgoing chunks
    write_buf: BytesMut,

    /// Server configuration
    config: ServerConfig,

    /// Application handler
    handler: Arc<H>,

    /// Stream registry for pub/sub routing
    registry: Arc<StreamRegistry>,

    /// Pending FC commands (stream key -> transaction ID)
    pending_fc: HashMap<String, f64>,

    /// Stream key we are publishing to (if any)
    publishing_to: Option<StreamKey>,

    /// Stream key we are subscribed to (if any)
    subscribed_to: Option<StreamKey>,

    /// Broadcast receiver for subscriber mode
    frame_rx: Option<broadcast::Receiver<BroadcastFrame>>,

    /// Subscriber state for backpressure handling
    subscriber_state: SubscriberState,

    /// Count of consecutive lag events (for disconnection threshold)
    consecutive_lag_count: u32,

    /// RTMP stream ID we're using for playback (subscriber mode)
    playback_stream_id: Option<u32>,
}

impl<H: RtmpHandler> Connection<H> {
    /// Create a new connection handler
    pub fn new(
        session_id: u64,
        socket: TcpStream,
        peer_addr: SocketAddr,
        config: ServerConfig,
        handler: Arc<H>,
        registry: Arc<StreamRegistry>,
    ) -> Self {
        let (read_half, write_half) = tokio::io::split(socket);

        Self {
            state: SessionState::new(session_id, peer_addr),
            context: SessionContext::new(session_id, peer_addr),
            reader: BufReader::with_capacity(config.read_buffer_size, read_half),
            writer: BufWriter::with_capacity(config.write_buffer_size, write_half),
            read_buf: BytesMut::with_capacity(config.read_buffer_size),
            chunk_decoder: ChunkDecoder::new(),
            chunk_encoder: ChunkEncoder::new(),
            write_buf: BytesMut::with_capacity(config.write_buffer_size),
            config,
            handler,
            registry,
            pending_fc: HashMap::new(),
            publishing_to: None,
            subscribed_to: None,
            frame_rx: None,
            subscriber_state: SubscriberState::Normal,
            consecutive_lag_count: 0,
            playback_stream_id: None,
        }
    }

    /// Run the connection
    pub async fn run(&mut self) -> Result<()> {
        // Check if handler allows connection
        if !self.handler.on_connection(&self.context).await {
            return Err(Error::Rejected("Connection rejected by handler".into()));
        }

        // Perform handshake
        self.do_handshake().await?;
        self.handler.on_handshake_complete(&self.context).await;

        // Set our chunk size
        self.send_set_chunk_size(self.config.chunk_size).await?;
        self.chunk_encoder.set_chunk_size(self.config.chunk_size);

        // Main message loop
        let idle_timeout = self.config.idle_timeout;
        let result = loop {
            // Handle subscriber mode: take frame_rx out to avoid borrow conflicts
            let mut frame_rx = self.frame_rx.take();

            // Use select! to handle both TCP input and broadcast frames
            let loop_result = if let Some(ref mut rx) = frame_rx {
                // Subscriber mode: listen for both TCP and broadcast frames
                tokio::select! {
                    biased;

                    // Receive broadcast frames for subscribers (higher priority)
                    frame_result = rx.recv() => {
                        match frame_result {
                            Ok(frame) => {
                                // Put receiver back before processing
                                self.frame_rx = frame_rx;
                                // Reset lag count on successful receive
                                self.consecutive_lag_count = 0;
                                if let Err(e) = self.send_broadcast_frame(frame).await {
                                    tracing::debug!(error = %e, "Failed to send frame");
                                    Err(e)
                                } else {
                                    Ok(true)
                                }
                            }
                            Err(broadcast::error::RecvError::Lagged(n)) => {
                                self.frame_rx = frame_rx;
                                self.handle_lag(n).await.map(|_| true)
                            }
                            Err(broadcast::error::RecvError::Closed) => {
                                self.frame_rx = frame_rx;
                                // Publisher ended, notify subscriber
                                if let Err(e) = self.handle_stream_ended().await {
                                    tracing::debug!(error = %e, "Error handling stream end");
                                }
                                Ok(false) // Signal to exit loop
                            }
                        }
                    }

                    // Read from TCP
                    result = timeout(idle_timeout, self.read_and_process()) => {
                        self.frame_rx = frame_rx;
                        match result {
                            Ok(Ok(continue_loop)) => Ok(continue_loop),
                            Ok(Err(e)) => {
                                tracing::debug!(error = %e, "Processing error");
                                Err(e)
                            }
                            Err(_) => {
                                tracing::debug!("Idle timeout");
                                Ok(false)
                            }
                        }
                    }
                }
            } else {
                // Publisher mode: only listen for TCP
                self.frame_rx = frame_rx;
                match timeout(idle_timeout, self.read_and_process()).await {
                    Ok(Ok(continue_loop)) => Ok(continue_loop),
                    Ok(Err(e)) => {
                        tracing::debug!(error = %e, "Processing error");
                        Err(e)
                    }
                    Err(_) => {
                        tracing::debug!("Idle timeout");
                        Ok(false)
                    }
                }
            };

            match loop_result {
                Ok(true) => continue,
                Ok(false) => break Ok(()),
                Err(e) => break Err(e),
            }
        };

        // Cleanup: unregister publisher or unsubscribe
        self.cleanup_on_disconnect().await;

        // Notify handler of disconnect
        self.handler.on_disconnect(&self.context).await;

        result
    }

    /// Cleanup when connection disconnects
    async fn cleanup_on_disconnect(&mut self) {
        // Unregister as publisher if we were publishing
        if let Some(ref key) = self.publishing_to {
            self.registry.unregister_publisher(key, self.state.id).await;
            tracing::debug!(
                session_id = self.state.id,
                stream = %key,
                "Unregistered publisher on disconnect"
            );
        }

        // Unsubscribe if we were subscribed
        if let Some(ref key) = self.subscribed_to {
            self.registry.unsubscribe(key).await;
            tracing::debug!(
                session_id = self.state.id,
                stream = %key,
                "Unsubscribed on disconnect"
            );
        }
    }

    /// Handle lag event from broadcast channel
    async fn handle_lag(&mut self, skipped: u64) -> Result<()> {
        let config = self.registry.config();

        self.consecutive_lag_count += 1;

        if skipped < config.lag_threshold_low {
            // Minor lag, continue normally
            tracing::debug!(
                session_id = self.state.id,
                skipped = skipped,
                "Minor broadcast lag, continuing"
            );
            return Ok(());
        }

        // Significant lag - enter skip mode
        if self.subscriber_state != SubscriberState::SkippingToKeyframe {
            self.subscriber_state = SubscriberState::SkippingToKeyframe;
            tracing::warn!(
                session_id = self.state.id,
                skipped = skipped,
                "Subscriber lagging, skipping to next keyframe"
            );
        }

        // Check if we should disconnect slow subscriber
        if self.consecutive_lag_count >= config.max_consecutive_lag_events {
            tracing::warn!(
                session_id = self.state.id,
                consecutive_lags = self.consecutive_lag_count,
                "Disconnecting slow subscriber"
            );
            return Err(Error::Rejected("Subscriber too slow".into()));
        }

        Ok(())
    }

    /// Handle stream ended (publisher closed broadcast channel)
    async fn handle_stream_ended(&mut self) -> Result<()> {
        if let Some(stream_id) = self.playback_stream_id {
            // Send StreamEOF
            self.send_user_control(UserControlEvent::StreamEof(stream_id))
                .await?;

            // Send onStatus
            let status = Command::on_status(
                stream_id,
                "status",
                NS_PLAY_STOP,
                "Stream ended",
            );
            self.send_command(CSID_COMMAND, stream_id, &status).await?;

            tracing::info!(
                session_id = self.state.id,
                stream_id = stream_id,
                "Stream ended, notified subscriber"
            );
        }

        Ok(())
    }

    /// Perform RTMP handshake
    async fn do_handshake(&mut self) -> Result<()> {
        let mut handshake = Handshake::new(HandshakeRole::Server);

        // Move to waiting state
        handshake.generate_initial();
        self.state.start_handshake();

        // Wait for C0C1
        let connection_timeout = self.config.connection_timeout;
        timeout(connection_timeout, async {
            loop {
                // Read more data
                let bytes_needed = handshake.bytes_needed();
                if bytes_needed > 0 && self.read_buf.len() < bytes_needed {
                    let n = self.reader.read_buf(&mut self.read_buf).await?;
                    if n == 0 {
                        return Err(Error::ConnectionClosed);
                    }
                }

                // Process handshake
                let mut buf = Bytes::copy_from_slice(&self.read_buf);
                if let Some(response) = handshake.process(&mut buf)? {
                    // Consume processed bytes
                    let consumed = self.read_buf.len() - buf.len();
                    self.read_buf.advance(consumed);

                    // Send response (S0S1S2 or nothing)
                    self.writer.write_all(&response).await?;
                    self.writer.flush().await?;
                }

                if handshake.is_done() {
                    break;
                }
            }

            Ok::<_, Error>(())
        })
        .await
        .map_err(|_| Error::Timeout)??;

        self.state.complete_handshake();
        tracing::debug!(session_id = self.state.id, "Handshake complete");

        Ok(())
    }

    /// Read data and process messages
    async fn read_and_process(&mut self) -> Result<bool> {
        // First, try to decode any complete chunks already in buffer
        // This handles data that arrived during handshake (e.g., connect command)
        let mut processed = false;
        while let Some(chunk) = self.chunk_decoder.decode(&mut self.read_buf)? {
            self.handle_chunk(chunk).await?;
            processed = true;
        }

        // If we processed something, return immediately (don't block waiting for more)
        if processed {
            return Ok(true);
        }

        // No complete chunks in buffer - wait for more data
        let n = self.reader.read_buf(&mut self.read_buf).await?;
        if n == 0 {
            return Ok(false); // Connection closed
        }

        let needs_ack = self.state.add_bytes_received(n as u64);

        // Try to decode chunks with the new data
        while let Some(chunk) = self.chunk_decoder.decode(&mut self.read_buf)? {
            self.handle_chunk(chunk).await?;
        }

        // Send acknowledgement if needed
        if needs_ack {
            self.send_acknowledgement().await?;
        }

        Ok(true)
    }

    /// Handle a decoded chunk
    async fn handle_chunk(&mut self, chunk: RtmpChunk) -> Result<()> {
        let message = RtmpMessage::from_chunk(&chunk)?;

        match message {
            RtmpMessage::SetChunkSize(size) => {
                tracing::debug!(size = size, "Peer set chunk size");
                self.chunk_decoder.set_chunk_size(size);
                self.state.in_chunk_size = size;
            }

            RtmpMessage::Abort { csid } => {
                self.chunk_decoder.abort(csid);
            }

            RtmpMessage::Acknowledgement { sequence: _ } => {
                // Peer acknowledged bytes - we can track this for flow control
            }

            RtmpMessage::WindowAckSize(size) => {
                self.state.window_ack_size = size;
            }

            RtmpMessage::SetPeerBandwidth {
                size,
                limit_type: _,
            } => {
                // Send window ack size back
                self.send_window_ack_size(size).await?;
            }

            RtmpMessage::UserControl(event) => {
                self.handle_user_control(event).await?;
            }

            RtmpMessage::Command(cmd) | RtmpMessage::CommandAmf3(cmd) => {
                self.handle_command(cmd).await?;
            }

            RtmpMessage::Data(data) | RtmpMessage::DataAmf3(data) => {
                self.handle_data(data).await?;
            }

            RtmpMessage::Audio { timestamp, data } => {
                self.handle_audio(timestamp, data).await?;
            }

            RtmpMessage::Video { timestamp, data } => {
                self.handle_video(timestamp, data).await?;
            }

            _ => {
                tracing::trace!(message = ?message, "Unhandled message");
            }
        }

        Ok(())
    }

    /// Handle user control event
    async fn handle_user_control(&mut self, event: UserControlEvent) -> Result<()> {
        match event {
            UserControlEvent::PingRequest(timestamp) => {
                self.send_ping_response(timestamp).await?;
            }
            UserControlEvent::SetBufferLength {
                stream_id: _,
                buffer_ms: _,
            } => {
                // Client's buffer length - we can use this for flow control
            }
            _ => {}
        }
        Ok(())
    }

    /// Handle command message
    async fn handle_command(&mut self, cmd: Command) -> Result<()> {
        match cmd.name.as_str() {
            CMD_CONNECT => self.handle_connect(cmd).await?,
            CMD_CREATE_STREAM => self.handle_create_stream(cmd).await?,
            CMD_DELETE_STREAM => self.handle_delete_stream(cmd).await?,
            CMD_PUBLISH => self.handle_publish(cmd).await?,
            CMD_PLAY => self.handle_play(cmd).await?,
            CMD_FC_PUBLISH => self.handle_fc_publish(cmd).await?,
            CMD_FC_UNPUBLISH => self.handle_fc_unpublish(cmd).await?,
            CMD_RELEASE_STREAM => self.handle_release_stream(cmd).await?,
            CMD_CLOSE | "closeStream" => self.handle_close_stream(cmd).await?,
            _ => {
                tracing::trace!(command = cmd.name, "Unknown command");
            }
        }
        Ok(())
    }

    /// Handle connect command
    async fn handle_connect(&mut self, cmd: Command) -> Result<()> {
        let params = ConnectParams::from_amf(&cmd.command_object);
        let encoder_type = params
            .flash_ver
            .as_deref()
            .map(EncoderType::from_flash_ver)
            .unwrap_or(EncoderType::Unknown);

        // Call handler
        let result = self.handler.on_connect(&self.context, &params).await;

        match result {
            AuthResult::Accept => {
                self.state.on_connect(params.clone(), encoder_type);
                self.context.with_connect(params, encoder_type);

                // Send window ack size
                self.send_window_ack_size(self.config.window_ack_size)
                    .await?;

                // Send peer bandwidth
                self.send_peer_bandwidth(self.config.peer_bandwidth).await?;

                // Send stream begin
                self.send_user_control(UserControlEvent::StreamBegin(0))
                    .await?;

                // Send connect result
                self.send_connect_result(cmd.transaction_id).await?;

                tracing::info!(
                    session_id = self.state.id,
                    app = self.context.app,
                    "Connected"
                );
            }
            AuthResult::Reject(reason) => {
                self.send_connect_error(cmd.transaction_id, &reason).await?;
                return Err(Error::Rejected(reason));
            }
            AuthResult::Redirect { url } => {
                self.send_connect_redirect(cmd.transaction_id, &url).await?;
                return Err(Error::Rejected(format!("Redirected to {}", url)));
            }
        }

        Ok(())
    }

    /// Handle createStream command
    async fn handle_create_stream(&mut self, cmd: Command) -> Result<()> {
        let stream_id = self.state.allocate_stream_id();

        let result = Command::result(
            cmd.transaction_id,
            AmfValue::Null,
            AmfValue::Number(stream_id as f64),
        );

        self.send_command(CSID_COMMAND, 0, &result).await?;

        tracing::debug!(stream_id = stream_id, "Stream created");
        Ok(())
    }

    /// Handle deleteStream command
    async fn handle_delete_stream(&mut self, cmd: Command) -> Result<()> {
        let stream_id = cmd
            .arguments
            .first()
            .and_then(|v| v.as_number())
            .unwrap_or(0.0) as u32;

        if let Some(stream) = self.state.remove_stream(stream_id) {
            if stream.is_publishing() {
                let stream_ctx = StreamContext::new(
                    self.context.clone(),
                    stream_id,
                    stream.stream_key.unwrap_or_default(),
                    true,
                );
                self.handler.on_publish_stop(&stream_ctx).await;
            }
        }

        Ok(())
    }

    /// Handle FCPublish command (OBS/Twitch compatibility)
    async fn handle_fc_publish(&mut self, cmd: Command) -> Result<()> {
        let stream_key = cmd
            .arguments
            .first()
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let result = self.handler.on_fc_publish(&self.context, &stream_key).await;

        match result {
            AuthResult::Accept => {
                // Store for later publish command
                self.pending_fc
                    .insert(stream_key.clone(), cmd.transaction_id);

                // Send onFCPublish response
                let response = Command {
                    name: CMD_ON_FC_PUBLISH.to_string(),
                    transaction_id: 0.0,
                    command_object: AmfValue::Null,
                    arguments: vec![],
                    stream_id: 0,
                };
                self.send_command(CSID_COMMAND, 0, &response).await?;
            }
            AuthResult::Reject(reason) => {
                return Err(Error::Rejected(reason));
            }
            AuthResult::Redirect { url } => {
                return Err(Error::Rejected(format!("Redirected to {}", url)));
            }
        }

        Ok(())
    }

    /// Handle FCUnpublish command
    async fn handle_fc_unpublish(&mut self, cmd: Command) -> Result<()> {
        let stream_key = cmd.arguments.first().and_then(|v| v.as_str()).unwrap_or("");

        self.pending_fc.remove(stream_key);

        // Send onFCUnpublish response
        let response = Command {
            name: CMD_ON_FC_UNPUBLISH.to_string(),
            transaction_id: 0.0,
            command_object: AmfValue::Null,
            arguments: vec![],
            stream_id: 0,
        };
        self.send_command(CSID_COMMAND, 0, &response).await?;

        Ok(())
    }

    /// Handle releaseStream command
    async fn handle_release_stream(&mut self, _cmd: Command) -> Result<()> {
        // No response needed, this is just cleanup notification
        Ok(())
    }

    /// Handle publish command
    async fn handle_publish(&mut self, cmd: Command) -> Result<()> {
        let stream_key = cmd
            .arguments
            .first()
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let publish_type = cmd
            .arguments
            .get(1)
            .and_then(|v| v.as_str())
            .unwrap_or("live")
            .to_string();

        let params = PublishParams {
            stream_key: stream_key.clone(),
            publish_type: publish_type.clone(),
            stream_id: cmd.stream_id,
        };

        let result = self.handler.on_publish(&self.context, &params).await;

        match result {
            AuthResult::Accept => {
                // Create stream key for registry
                let app = self.context.app.clone();
                let registry_key = StreamKey::new(&app, &stream_key);

                // Register as publisher in the registry
                if let Err(e) = self.registry.register_publisher(&registry_key, self.state.id).await {
                    tracing::warn!(
                        session_id = self.state.id,
                        stream = %registry_key,
                        error = %e,
                        "Failed to register publisher"
                    );
                    let status = Command::on_status(
                        cmd.stream_id,
                        "error",
                        NS_PUBLISH_BAD_NAME,
                        &format!("Stream already publishing: {}", e),
                    );
                    self.send_command(CSID_COMMAND, cmd.stream_id, &status).await?;
                    return Err(Error::Rejected(format!("Stream already publishing: {}", e)));
                }

                // Track that we're publishing to this stream
                self.publishing_to = Some(registry_key);

                // Update stream state
                if let Some(stream) = self.state.get_stream_mut(cmd.stream_id) {
                    stream.start_publish(stream_key.clone(), publish_type);
                }

                // Send StreamBegin
                self.send_user_control(UserControlEvent::StreamBegin(cmd.stream_id))
                    .await?;

                // Send onStatus
                let status = Command::on_status(
                    cmd.stream_id,
                    "status",
                    NS_PUBLISH_START,
                    &format!("{} is now published", stream_key),
                );
                self.send_command(CSID_COMMAND, cmd.stream_id, &status)
                    .await?;

                tracing::info!(
                    session_id = self.state.id,
                    stream_key = stream_key,
                    "Publishing started"
                );
            }
            AuthResult::Reject(reason) => {
                let status =
                    Command::on_status(cmd.stream_id, "error", NS_PUBLISH_BAD_NAME, &reason);
                self.send_command(CSID_COMMAND, cmd.stream_id, &status)
                    .await?;
                return Err(Error::Rejected(reason));
            }
            AuthResult::Redirect { url } => {
                return Err(Error::Rejected(format!("Redirected to {}", url)));
            }
        }

        Ok(())
    }

    /// Handle play command
    async fn handle_play(&mut self, cmd: Command) -> Result<()> {
        let stream_name = cmd
            .arguments
            .first()
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let start = cmd
            .arguments
            .get(1)
            .and_then(|v| v.as_number())
            .unwrap_or(-2.0);

        let duration = cmd
            .arguments
            .get(2)
            .and_then(|v| v.as_number())
            .unwrap_or(-1.0);

        let reset = cmd
            .arguments
            .get(3)
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let params = PlayParams {
            stream_name: stream_name.clone(),
            start,
            duration,
            reset,
            stream_id: cmd.stream_id,
        };

        let result = self.handler.on_play(&self.context, &params).await;

        match result {
            AuthResult::Accept => {
                // Create stream key for registry
                let app = self.context.app.clone();
                let registry_key = StreamKey::new(&app, &stream_name);

                // Subscribe to the stream in registry
                let (rx, catchup_frames) = match self.registry.subscribe(&registry_key).await {
                    Ok(result) => result,
                    Err(e) => {
                        tracing::debug!(
                            session_id = self.state.id,
                            stream = %registry_key,
                            error = %e,
                            "Stream not found for play"
                        );
                        let status = Command::on_status(
                            cmd.stream_id,
                            "error",
                            NS_PLAY_STREAM_NOT_FOUND,
                            &format!("Stream not found: {}", stream_name),
                        );
                        self.send_command(CSID_COMMAND, cmd.stream_id, &status).await?;
                        return Ok(());
                    }
                };

                // Store subscription info
                self.subscribed_to = Some(registry_key.clone());
                self.frame_rx = Some(rx);
                self.playback_stream_id = Some(cmd.stream_id);

                if let Some(stream) = self.state.get_stream_mut(cmd.stream_id) {
                    stream.start_play(stream_name.clone());
                }

                // Send StreamBegin
                self.send_user_control(UserControlEvent::StreamBegin(cmd.stream_id))
                    .await?;

                // Send onStatus Reset
                if reset {
                    let status = Command::on_status(
                        cmd.stream_id,
                        "status",
                        NS_PLAY_RESET,
                        "Playing and resetting",
                    );
                    self.send_command(CSID_COMMAND, cmd.stream_id, &status)
                        .await?;
                }

                // Send onStatus Start
                let status = Command::on_status(
                    cmd.stream_id,
                    "status",
                    NS_PLAY_START,
                    &format!("Started playing {}", stream_name),
                );
                self.send_command(CSID_COMMAND, cmd.stream_id, &status)
                    .await?;

                // Send catchup frames (sequence headers + GOP)
                tracing::debug!(
                    session_id = self.state.id,
                    stream = %registry_key,
                    catchup_frames = catchup_frames.len(),
                    "Sending catchup frames"
                );

                for frame in catchup_frames {
                    self.send_broadcast_frame(frame).await?;
                }

                tracing::info!(
                    session_id = self.state.id,
                    stream_name = stream_name,
                    "Playing started"
                );
            }
            AuthResult::Reject(reason) => {
                let status =
                    Command::on_status(cmd.stream_id, "error", NS_PLAY_STREAM_NOT_FOUND, &reason);
                self.send_command(CSID_COMMAND, cmd.stream_id, &status)
                    .await?;
            }
            AuthResult::Redirect { url: _ } => {
                // Handle redirect
            }
        }

        Ok(())
    }

    /// Handle closeStream command
    async fn handle_close_stream(&mut self, cmd: Command) -> Result<()> {
        if let Some(stream) = self.state.get_stream_mut(cmd.stream_id) {
            stream.stop();
        }
        Ok(())
    }

    /// Handle data message
    async fn handle_data(&mut self, data: DataMessage) -> Result<()> {
        match data.name.as_str() {
            CMD_SET_DATA_FRAME => {
                // @setDataFrame usually has "onMetaData" as first value
                if let Some(AmfValue::String(name)) = data.values.first() {
                    if name == CMD_ON_METADATA {
                        self.handle_metadata(data.stream_id, &data.values[1..])
                            .await?;
                    }
                }
            }
            CMD_ON_METADATA => {
                self.handle_metadata(data.stream_id, &data.values).await?;
            }
            _ => {
                tracing::trace!(name = data.name, "Unknown data message");
            }
        }
        Ok(())
    }

    /// Handle metadata
    async fn handle_metadata(&mut self, stream_id: u32, values: &[AmfValue]) -> Result<()> {
        // Extract metadata object
        let metadata: HashMap<String, AmfValue> = values
            .first()
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();

        if let Some(stream) = self.state.get_stream_mut(stream_id) {
            stream.on_metadata();
        }

        if let Some(stream) = self.state.get_stream(stream_id) {
            if stream.is_publishing() {
                let stream_ctx = StreamContext::new(
                    self.context.clone(),
                    stream_id,
                    stream.stream_key.clone().unwrap_or_default(),
                    true,
                );
                self.handler.on_metadata(&stream_ctx, &metadata).await;
            }
        }

        Ok(())
    }

    /// Handle audio message
    async fn handle_audio(&mut self, timestamp: u32, data: Bytes) -> Result<()> {
        if data.is_empty() {
            return Ok(());
        }

        // Find publishing stream
        let stream_id = self.find_publishing_stream()?;
        let stream = self
            .state
            .get_stream_mut(stream_id)
            .ok_or_else(|| ProtocolError::StreamNotFound(stream_id))?;

        let is_header = data.len() >= 2 && (data[0] >> 4) == 10 && data[1] == 0;
        stream.on_audio(timestamp, is_header, data.len());

        // Store sequence header
        if is_header {
            let tag = FlvTag::audio(timestamp, data.clone());
            stream.gop_buffer.set_audio_header(tag);
        }

        // Create stream context for callbacks
        let stream_key = stream.stream_key.clone().unwrap_or_default();
        let stream_ctx = StreamContext::new(self.context.clone(), stream_id, stream_key, true);

        // Deliver based on mode
        let mode = self.handler.media_delivery_mode();

        if matches!(mode, MediaDeliveryMode::RawFlv | MediaDeliveryMode::Both) {
            let tag = FlvTag::audio(timestamp, data.clone());
            self.handler.on_media_tag(&stream_ctx, &tag).await;
        }

        if matches!(
            mode,
            MediaDeliveryMode::ParsedFrames | MediaDeliveryMode::Both
        ) {
            if data.len() >= 2 && (data[0] >> 4) == 10 {
                // AAC
                if let Ok(aac_data) = AacData::parse(data.slice(1..)) {
                    self.handler
                        .on_audio_frame(&stream_ctx, &aac_data, timestamp)
                        .await;
                }
            }
        }

        // Broadcast to subscribers via registry
        if let Some(ref key) = self.publishing_to {
            let frame = BroadcastFrame::audio(timestamp, data, is_header);
            self.registry.broadcast(key, frame).await;
        }

        Ok(())
    }

    /// Handle video message
    async fn handle_video(&mut self, timestamp: u32, data: Bytes) -> Result<()> {
        if data.is_empty() {
            return Ok(());
        }

        // Find publishing stream
        let stream_id = self.find_publishing_stream()?;
        let stream = self
            .state
            .get_stream_mut(stream_id)
            .ok_or_else(|| ProtocolError::StreamNotFound(stream_id))?;

        let is_keyframe = (data[0] >> 4) == 1;
        let is_header = data.len() >= 2 && (data[0] & 0x0F) == 7 && data[1] == 0;
        stream.on_video(timestamp, is_keyframe, is_header, data.len());

        // Create FLV tag
        let tag = FlvTag::video(timestamp, data.clone());

        // Store sequence header
        if is_header {
            stream.gop_buffer.set_video_header(tag.clone());
        } else {
            // Add to GOP buffer
            stream.gop_buffer.push(tag.clone());
        }

        // Create stream context for callbacks
        let stream_key = stream.stream_key.clone().unwrap_or_default();
        let stream_ctx = StreamContext::new(self.context.clone(), stream_id, stream_key, true);

        // Notify keyframe
        if is_keyframe && !is_header {
            self.handler.on_keyframe(&stream_ctx, timestamp).await;
        }

        // Deliver based on mode
        let mode = self.handler.media_delivery_mode();

        if matches!(mode, MediaDeliveryMode::RawFlv | MediaDeliveryMode::Both) {
            self.handler.on_media_tag(&stream_ctx, &tag).await;
        }

        if matches!(
            mode,
            MediaDeliveryMode::ParsedFrames | MediaDeliveryMode::Both
        ) {
            if data.len() >= 2 && (data[0] & 0x0F) == 7 {
                // AVC/H.264
                if let Ok(h264_data) = H264Data::parse(data.slice(1..)) {
                    self.handler
                        .on_video_frame(&stream_ctx, &h264_data, timestamp)
                        .await;
                }
            }
        }

        // Broadcast to subscribers via registry
        if let Some(ref key) = self.publishing_to {
            let frame = BroadcastFrame::video(timestamp, data, is_keyframe, is_header);
            self.registry.broadcast(key, frame).await;
        }

        Ok(())
    }

    /// Find the publishing stream (assumes single publish per connection)
    fn find_publishing_stream(&self) -> Result<u32> {
        for (id, stream) in &self.state.streams {
            if stream.is_publishing() {
                return Ok(*id);
            }
        }
        Err(ProtocolError::StreamNotFound(0).into())
    }

    // === Message sending helpers ===

    async fn send_command(&mut self, csid: u32, stream_id: u32, cmd: &Command) -> Result<()> {
        let (msg_type, payload) = RtmpMessage::Command(cmd.clone()).encode();

        let chunk = RtmpChunk {
            csid,
            timestamp: 0,
            message_type: msg_type,
            stream_id,
            payload,
        };

        self.write_buf.clear();
        self.chunk_encoder.encode(&chunk, &mut self.write_buf);
        self.writer.write_all(&self.write_buf).await?;
        self.writer.flush().await?;

        Ok(())
    }

    async fn send_connect_result(&mut self, transaction_id: f64) -> Result<()> {
        let mut props = HashMap::new();
        props.insert(
            "fmsVer".to_string(),
            AmfValue::String("FMS/3,5,7,7009".into()),
        );
        props.insert("capabilities".to_string(), AmfValue::Number(31.0));
        props.insert("mode".to_string(), AmfValue::Number(1.0));

        let mut info = HashMap::new();
        info.insert("level".to_string(), AmfValue::String("status".into()));
        info.insert(
            "code".to_string(),
            AmfValue::String(NC_CONNECT_SUCCESS.into()),
        );
        info.insert(
            "description".to_string(),
            AmfValue::String("Connection succeeded".into()),
        );
        info.insert("objectEncoding".to_string(), AmfValue::Number(0.0));

        let result = Command::result(
            transaction_id,
            AmfValue::Object(props),
            AmfValue::Object(info),
        );

        self.send_command(CSID_COMMAND, 0, &result).await
    }

    async fn send_connect_error(&mut self, transaction_id: f64, reason: &str) -> Result<()> {
        let mut info = HashMap::new();
        info.insert("level".to_string(), AmfValue::String("error".into()));
        info.insert(
            "code".to_string(),
            AmfValue::String(NC_CONNECT_REJECTED.into()),
        );
        info.insert("description".to_string(), AmfValue::String(reason.into()));

        let error = Command::error(transaction_id, AmfValue::Null, AmfValue::Object(info));

        self.send_command(CSID_COMMAND, 0, &error).await
    }

    async fn send_connect_redirect(&mut self, transaction_id: f64, url: &str) -> Result<()> {
        let mut info = HashMap::new();
        info.insert("level".to_string(), AmfValue::String("error".into()));
        info.insert(
            "code".to_string(),
            AmfValue::String(NC_CONNECT_REJECTED.into()),
        );
        info.insert(
            "description".to_string(),
            AmfValue::String("Redirect".into()),
        );
        info.insert("ex.redirect".to_string(), AmfValue::String(url.into()));

        let error = Command::error(transaction_id, AmfValue::Null, AmfValue::Object(info));

        self.send_command(CSID_COMMAND, 0, &error).await
    }

    async fn send_set_chunk_size(&mut self, size: u32) -> Result<()> {
        let (msg_type, payload) = RtmpMessage::SetChunkSize(size).encode();

        let chunk = RtmpChunk {
            csid: CSID_PROTOCOL_CONTROL,
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

    async fn send_window_ack_size(&mut self, size: u32) -> Result<()> {
        let (msg_type, payload) = RtmpMessage::WindowAckSize(size).encode();

        let chunk = RtmpChunk {
            csid: CSID_PROTOCOL_CONTROL,
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

    async fn send_peer_bandwidth(&mut self, size: u32) -> Result<()> {
        let (msg_type, payload) = RtmpMessage::SetPeerBandwidth {
            size,
            limit_type: BANDWIDTH_LIMIT_DYNAMIC,
        }
        .encode();

        let chunk = RtmpChunk {
            csid: CSID_PROTOCOL_CONTROL,
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

    async fn send_user_control(&mut self, event: UserControlEvent) -> Result<()> {
        let (msg_type, payload) = RtmpMessage::UserControl(event).encode();

        let chunk = RtmpChunk {
            csid: CSID_PROTOCOL_CONTROL,
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

    async fn send_acknowledgement(&mut self) -> Result<()> {
        let sequence = self.state.bytes_received as u32;

        let (msg_type, payload) = RtmpMessage::Acknowledgement { sequence }.encode();

        let chunk = RtmpChunk {
            csid: CSID_PROTOCOL_CONTROL,
            timestamp: 0,
            message_type: msg_type,
            stream_id: 0,
            payload,
        };

        self.write_buf.clear();
        self.chunk_encoder.encode(&chunk, &mut self.write_buf);
        self.writer.write_all(&self.write_buf).await?;
        self.writer.flush().await?;

        self.state.mark_ack_sent();
        Ok(())
    }

    async fn send_ping_response(&mut self, timestamp: u32) -> Result<()> {
        self.send_user_control(UserControlEvent::PingResponse(timestamp))
            .await
    }

    // === Media sending methods for subscriber mode ===

    /// Send a video message to the client
    async fn send_video(&mut self, stream_id: u32, timestamp: u32, data: Bytes) -> Result<()> {
        let (msg_type, payload) = RtmpMessage::Video {
            timestamp,
            data: data.clone(),
        }
        .encode();

        let chunk = RtmpChunk {
            csid: CSID_VIDEO,
            timestamp,
            message_type: msg_type,
            stream_id,
            payload,
        };

        self.write_buf.clear();
        self.chunk_encoder.encode(&chunk, &mut self.write_buf);
        self.writer.write_all(&self.write_buf).await?;
        // Don't flush after every frame - batch writes for efficiency
        Ok(())
    }

    /// Send an audio message to the client
    async fn send_audio(&mut self, stream_id: u32, timestamp: u32, data: Bytes) -> Result<()> {
        let (msg_type, payload) = RtmpMessage::Audio {
            timestamp,
            data: data.clone(),
        }
        .encode();

        let chunk = RtmpChunk {
            csid: CSID_AUDIO,
            timestamp,
            message_type: msg_type,
            stream_id,
            payload,
        };

        self.write_buf.clear();
        self.chunk_encoder.encode(&chunk, &mut self.write_buf);
        self.writer.write_all(&self.write_buf).await?;
        // Don't flush after every frame - batch writes for efficiency
        Ok(())
    }

    /// Send a broadcast frame to the subscriber client
    ///
    /// Handles backpressure by skipping non-keyframes when in skip mode.
    async fn send_broadcast_frame(&mut self, frame: BroadcastFrame) -> Result<()> {
        let stream_id = self.playback_stream_id.unwrap_or(1);

        // Backpressure handling: skip non-keyframes if we're lagging
        if self.subscriber_state == SubscriberState::SkippingToKeyframe {
            match frame.frame_type {
                FrameType::Video => {
                    if frame.is_keyframe || frame.is_header {
                        // Got a keyframe or header, resume normal operation
                        self.subscriber_state = SubscriberState::Normal;
                        tracing::debug!(
                            session_id = self.state.id,
                            "Received keyframe, resuming normal playback"
                        );
                    } else {
                        // Skip non-keyframe video
                        return Ok(());
                    }
                }
                FrameType::Audio => {
                    // Always forward audio (glitches are worse than skipping audio)
                    // Audio frames are small, so this doesn't add much load
                }
                FrameType::Metadata => {
                    // Always forward metadata
                }
            }
        }

        // Send the frame based on type
        match frame.frame_type {
            FrameType::Video => {
                self.send_video(stream_id, frame.timestamp, frame.data).await?;
            }
            FrameType::Audio => {
                self.send_audio(stream_id, frame.timestamp, frame.data).await?;
            }
            FrameType::Metadata => {
                // Send metadata as data message
                self.send_metadata_frame(stream_id, frame.data).await?;
            }
        }

        // Periodically flush to ensure data is sent
        // The flush happens less frequently when sending many frames
        self.writer.flush().await?;

        Ok(())
    }

    /// Send metadata frame to subscriber
    async fn send_metadata_frame(&mut self, stream_id: u32, data: Bytes) -> Result<()> {
        // Metadata is sent as a data message with onMetaData
        let data_msg = DataMessage {
            name: CMD_ON_METADATA.to_string(),
            values: vec![AmfValue::String("onMetaData".to_string())],
            stream_id,
        };

        let (msg_type, payload) = RtmpMessage::Data(data_msg).encode();

        // If the original data is available, use it directly
        // Otherwise, encode the metadata
        let chunk = RtmpChunk {
            csid: CSID_COMMAND,
            timestamp: 0,
            message_type: msg_type,
            stream_id,
            payload: if data.is_empty() { payload } else { data },
        };

        self.write_buf.clear();
        self.chunk_encoder.encode(&chunk, &mut self.write_buf);
        self.writer.write_all(&self.write_buf).await?;

        Ok(())
    }
}

use bytes::Buf;
