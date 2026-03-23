//! RTMP stream publisher
//!
//! High-level API for publishing audio (and optionally video) streams to RTMP servers.

use bytes::Bytes;
use tokio::sync::mpsc;

use crate::error::{Error, Result};

use super::config::ClientConfig;
use super::connector::RtmpConnector;

/// Events from the RTMP publisher
#[derive(Debug)]
pub enum PublishEvent {
    /// Connected and ready to publish
    Connected,

    /// Publishing started on the server
    Publishing,

    /// Error occurred
    Error(String),

    /// Disconnected
    Disconnected,
}

/// RTMP stream publisher
///
/// Publishes audio-only (or audio+video) streams to an RTMP server.
///
/// # Example
/// ```no_run
/// use rtmp_rs::client::{ClientConfig, RtmpPublisher};
///
/// # async fn example() -> rtmp_rs::error::Result<()> {
/// let config = ClientConfig::new("rtmp://localhost/live/stream_key");
/// let (mut publisher, mut events) = RtmpPublisher::new(config);
///
/// // Spawn event handler
/// tokio::spawn(async move {
///     while let Some(event) = events.recv().await {
///         println!("Event: {:?}", event);
///     }
/// });
///
/// // Connect and start publishing
/// publisher.connect().await?;
/// # Ok(())
/// # }
/// ```
pub struct RtmpPublisher {
    config: ClientConfig,
    event_tx: mpsc::Sender<PublishEvent>,
    connector: Option<RtmpConnector>,
}

impl RtmpPublisher {
    /// Create a new publisher.
    ///
    /// Returns the publisher and a receiver for events.
    pub fn new(config: ClientConfig) -> (Self, mpsc::Receiver<PublishEvent>) {
        let (tx, rx) = mpsc::channel(256);

        let publisher = Self {
            config,
            event_tx: tx,
            connector: None,
        };

        (publisher, rx)
    }

    /// Connect to the RTMP server and start publishing.
    ///
    /// After this returns successfully, you can call `send_audio()` to
    /// send audio frames.
    pub async fn connect(&mut self) -> Result<()> {
        let mut connector = RtmpConnector::connect(self.config.clone()).await?;
        let _ = self.event_tx.send(PublishEvent::Connected).await;

        let stream_name = self
            .config
            .parse_url()
            .and_then(|u| u.stream_key)
            .unwrap_or_default();

        connector.publish(&stream_name).await?;
        let _ = self.event_tx.send(PublishEvent::Publishing).await;

        self.connector = Some(connector);
        Ok(())
    }

    /// Send an audio frame.
    ///
    /// The `data` should be the complete FLV audio tag body, including any
    /// codec-specific headers. The caller is responsible for constructing
    /// the correct payload for their chosen codec.
    ///
    /// `timestamp` is in milliseconds.
    ///
    /// # AAC Example
    ///
    /// For AAC, the FLV audio tag body is structured as:
    /// - First byte: audio tag header (`0xAF` = AAC, 44100Hz, stereo, 16-bit)
    /// - Second byte: AAC packet type (`0x00` = sequence header, `0x01` = raw)
    /// - Remaining bytes: codec data
    ///
    /// ```no_run
    /// # use bytes::Bytes;
    /// # async fn example(publisher: &mut rtmp_rs::client::RtmpPublisher) -> rtmp_rs::error::Result<()> {
    /// // Send AAC sequence header (AudioSpecificConfig)
    /// let audio_specific_config: &[u8] = &[0x12, 0x10]; // example config
    /// let mut header = vec![0xAF, 0x00];
    /// header.extend_from_slice(audio_specific_config);
    /// publisher.send_audio(Bytes::from(header), 0).await?;
    ///
    /// // Send raw AAC frame
    /// let raw_aac: &[u8] = &[/* raw AAC data */];
    /// let mut frame = vec![0xAF, 0x01];
    /// frame.extend_from_slice(raw_aac);
    /// publisher.send_audio(Bytes::from(frame), timestamp_ms).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn send_audio(&mut self, data: Bytes, timestamp: u32) -> Result<()> {
        let connector = self
            .connector
            .as_mut()
            .ok_or_else(|| Error::Protocol(crate::error::ProtocolError::UnexpectedMessage(
                "Not connected".into(),
            )))?;

        connector.send_audio_data(data, timestamp).await
    }

    /// Disconnect from the server.
    pub async fn disconnect(&mut self) {
        self.connector.take();
        let _ = self.event_tx.send(PublishEvent::Disconnected).await;
    }

    /// Check if currently connected and publishing.
    pub fn is_connected(&self) -> bool {
        self.connector.is_some()
    }
}
