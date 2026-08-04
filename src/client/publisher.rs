//! RTMP stream publisher
//!
//! High-level API for publishing audio (and optionally video) streams to RTMP servers.

use bytes::Bytes;
use tokio::sync::mpsc;

use crate::error::{Error, MediaError, Result};
use crate::media::aac::AacData;
use crate::media::h264::H264Data;

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
    /// Cached FLV audio format byte derived from the AAC sequence header;
    /// used by `send_audio(&AacData)` to build tag bodies for raw frames.
    aac_format_byte: Option<u8>,
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
            aac_format_byte: None,
        };

        (publisher, rx)
    }

    /// Connect to the RTMP server and start publishing.
    ///
    /// After this returns successfully, you can call `send_video()` and
    /// `send_audio()` to send media frames.
    pub async fn connect(&mut self) -> Result<()> {
        let mut connector = RtmpConnector::connect(self.config.clone()).await?;
        // Reset cached state from any previous session.
        self.aac_format_byte = None;
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

    /// Send an audio frame parsed from RTMP data.
    ///
    /// `data` is parsed AAC data; its FLV tag body is built internally and
    /// sent on the published stream. `timestamp` is in milliseconds.
    ///
    /// For `AacData::SequenceHeader`, the FLV audio format byte is derived
    /// from the `AudioSpecificConfig` and cached for subsequent raw frames.
    /// For `AacData::Frame`, the cached format byte is used; calling this
    /// with a `Frame` before any `SequenceHeader` returns an error.
    ///
    /// # AAC Example
    ///
    /// ```no_run
    /// # use bytes::Bytes;
    /// # use rtmp_rs::media::{AacData, AudioSpecificConfig};
    /// # async fn example(publisher: &mut rtmp_rs::client::RtmpPublisher) -> rtmp_rs::error::Result<()> {
    /// // Send AAC sequence header (AudioSpecificConfig: AAC-LC, 44.1kHz, stereo)
    /// let asc = AudioSpecificConfig::parse(Bytes::from_static(&[0x12, 0x10])).unwrap();
    /// let audio_seq = AacData::SequenceHeader(asc);
    /// publisher.send_audio(&audio_seq, 0).await?;
    ///
    /// // Send raw AAC frame
    /// let audio_frame = AacData::Frame { data: Bytes::from_static(&[/* raw AAC data */]) };
    /// publisher.send_audio(&audio_frame, 1024).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn send_audio(&mut self, data: &AacData, timestamp: u32) -> Result<()> {
        let connector = self.connector.as_mut().ok_or(Error::ConnectionClosed)?;

        let format_byte = match data {
            AacData::SequenceHeader(cfg) => cfg.flv_format_byte(),
            AacData::Frame { .. } => self
                .aac_format_byte
                .ok_or(Error::Media(MediaError::MissingSequenceHeader))?,
        };

        let body = data.to_flv_tag_body(format_byte);
        connector.send_audio_data(body, timestamp).await?;

        if let AacData::SequenceHeader(_) = data {
            self.aac_format_byte = Some(format_byte);
        }
        Ok(())
    }

    /// Send a pre-built FLV audio tag body.
    ///
    /// `data` should be the complete FLV audio tag body, including the
    /// format byte and AAC packet type. The caller is responsible for
    /// constructing the correct payload. `timestamp` is in milliseconds.
    ///
    /// This is the low-level counterpart to [`send_audio`](Self::send_audio).
    /// It does **not** update the cached `aac_format_byte`; mixing
    /// `send_audio_raw` for a sequence header with `send_audio` for raw
    /// frames will leave the cache unset and cause `send_audio` to error.
    pub async fn send_audio_raw(&mut self, data: Bytes, timestamp: u32) -> Result<()> {
        let connector = self.connector.as_mut().ok_or(Error::ConnectionClosed)?;

        connector.send_audio_data(data, timestamp).await
    }

    /// Send a video frame parsed from RTMP data.
    ///
    /// `data` is parsed AVC data; its FLV tag body is built internally and
    /// sent on the published stream. `timestamp` is in milliseconds.
    ///
    /// # AVC Example
    ///
    /// For AVC, the FLV video tag body is structured as:
    /// - First byte: frame type (4 bits) + codec ID (4 bits, `0x07` = AVC)
    /// - Second byte: AVC packet type (`0x00` = sequence header, `0x01` = NALU)
    /// - Bytes 3-5: composition time (SI24, signed)
    /// - Remaining bytes: codec data
    ///
    /// ```no_run
    /// # use bytes::Bytes;
    /// # use rtmp_rs::media::h264::{AvcConfig, H264Data};
    /// # async fn example(publisher: &mut rtmp_rs::client::RtmpPublisher) -> rtmp_rs::error::Result<()> {
    /// // Send AVC sequence header (AVCDecoderConfigurationRecord)
    /// let avc_raw = Bytes::from_static(&[
    ///     0x01, 0x64, 0x00, 0x1F, 0xFF, 0xE1, 0x00, 0x04, 0x67, 0x64, 0x00, 0x1F,
    ///     0x01, 0x00, 0x03, 0x68, 0xEF, 0x38,
    /// ]);
    /// let video_seq = H264Data::SequenceHeader(AvcConfig::parse(avc_raw).unwrap());
    /// publisher.send_video(&video_seq, 0).await?;
    ///
    /// // Send keyframe (IDR)
    /// let nalus = Bytes::from_static(&[0x00, 0x00, 0x00, 0x05, 0x65, 0x88, 0x84, 0x00, 0x00]);
    /// let keyframe = H264Data::Frame { keyframe: true, composition_time: 0, nalus };
    /// publisher.send_video(&keyframe, 33).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn send_video(&mut self, data: &H264Data, timestamp: u32) -> Result<()> {
        let connector = self.connector.as_mut().ok_or(Error::ConnectionClosed)?;

        let body = data.to_flv_tag_body();
        connector.send_video_data(body, timestamp).await
    }

    /// Disconnect from the server.
    ///
    /// Resets the cached `aac_format_byte`; after reconnect, a fresh AAC
    /// sequence header must be sent before raw audio frames.
    pub async fn disconnect(&mut self) {
        self.connector.take();
        self.aac_format_byte = None;
        let _ = self.event_tx.send(PublishEvent::Disconnected).await;
    }

    /// Check if currently connected and publishing.
    pub fn is_connected(&self) -> bool {
        self.connector.is_some()
    }
}
