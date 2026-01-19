//! RTMP stream puller
//!
//! High-level API for pulling streams from RTMP servers.

use tokio::sync::mpsc;

use crate::error::Result;
use crate::media::{AacData, FlvTag, H264Data};
use crate::protocol::message::RtmpMessage;

use super::config::ClientConfig;
use super::connector::RtmpConnector;

/// Events from the RTMP puller
#[derive(Debug)]
pub enum ClientEvent {
    /// Connected to server
    Connected,

    /// Stream metadata received
    Metadata(std::collections::HashMap<String, crate::amf::AmfValue>),

    /// Video frame received
    VideoFrame {
        timestamp: u32,
        data: H264Data,
    },

    /// Audio frame received
    AudioFrame {
        timestamp: u32,
        data: AacData,
    },

    /// Raw video tag (if configured)
    VideoTag(FlvTag),

    /// Raw audio tag (if configured)
    AudioTag(FlvTag),

    /// Stream ended
    StreamEnd,

    /// Error occurred
    Error(String),

    /// Disconnected
    Disconnected,
}

/// RTMP stream puller
pub struct RtmpPuller {
    config: ClientConfig,
    event_tx: mpsc::Sender<ClientEvent>,
    event_rx: Option<mpsc::Receiver<ClientEvent>>,
}

impl RtmpPuller {
    /// Create a new puller
    ///
    /// Returns the puller and a receiver for events.
    pub fn new(config: ClientConfig) -> (Self, mpsc::Receiver<ClientEvent>) {
        let (tx, rx) = mpsc::channel(1024);

        let puller = Self {
            config,
            event_tx: tx,
            event_rx: None, // Receiver is returned to caller
        };

        (puller, rx)
    }

    /// Take the event receiver (if not already taken)
    pub fn take_events(&mut self) -> Option<mpsc::Receiver<ClientEvent>> {
        self.event_rx.take()
    }

    /// Start pulling the stream
    pub async fn start(&self) -> Result<()> {
        let tx = self.event_tx.clone();

        // Connect
        let mut connector = RtmpConnector::connect(self.config.clone()).await?;
        let _ = tx.send(ClientEvent::Connected).await;

        // Get stream name from URL
        let stream_name = self.config.parse_url()
            .and_then(|u| u.stream_key)
            .unwrap_or_default();

        // Start playing
        connector.play(&stream_name).await?;

        // Read messages
        loop {
            match connector.read_message().await {
                Ok(msg) => {
                    if !self.handle_message(msg, &tx).await {
                        break;
                    }
                }
                Err(e) => {
                    let _ = tx.send(ClientEvent::Error(e.to_string())).await;
                    break;
                }
            }
        }

        let _ = tx.send(ClientEvent::Disconnected).await;
        Ok(())
    }

    /// Handle a received message
    async fn handle_message(&self, msg: RtmpMessage, tx: &mpsc::Sender<ClientEvent>) -> bool {
        match msg {
            RtmpMessage::Video { timestamp, data } => {
                // Send raw tag
                let tag = FlvTag::video(timestamp, data.clone());
                let _ = tx.send(ClientEvent::VideoTag(tag)).await;

                // Parse and send frame
                if data.len() >= 2 && (data[0] & 0x0F) == 7 {
                    if let Ok(h264) = H264Data::parse(data.slice(1..)) {
                        let _ = tx.send(ClientEvent::VideoFrame {
                            timestamp,
                            data: h264,
                        }).await;
                    }
                }
            }

            RtmpMessage::Audio { timestamp, data } => {
                // Send raw tag
                let tag = FlvTag::audio(timestamp, data.clone());
                let _ = tx.send(ClientEvent::AudioTag(tag)).await;

                // Parse and send frame
                if data.len() >= 2 && (data[0] >> 4) == 10 {
                    if let Ok(aac) = AacData::parse(data.slice(1..)) {
                        let _ = tx.send(ClientEvent::AudioFrame {
                            timestamp,
                            data: aac,
                        }).await;
                    }
                }
            }

            RtmpMessage::Data(data) | RtmpMessage::DataAmf3(data) => {
                if data.name == "onMetaData" || data.name == "@setDataFrame" {
                    if let Some(metadata) = data.values.first().and_then(|v| v.as_object()) {
                        let _ = tx.send(ClientEvent::Metadata(metadata.clone())).await;
                    }
                }
            }

            RtmpMessage::Command(cmd) if cmd.name == "onStatus" => {
                if let Some(info) = cmd.arguments.first().and_then(|v| v.as_object()) {
                    if let Some(code) = info.get("code").and_then(|v| v.as_str()) {
                        if code == crate::protocol::constants::NS_PLAY_STOP {
                            let _ = tx.send(ClientEvent::StreamEnd).await;
                            return false;
                        }
                    }
                }
            }

            RtmpMessage::UserControl(crate::protocol::message::UserControlEvent::StreamEof(_)) => {
                let _ = tx.send(ClientEvent::StreamEnd).await;
                return false;
            }

            _ => {}
        }

        true
    }
}

use bytes::Buf;
