//! Multi-platform RTMP publisher
//!
//! Publishes the same stream to several RTMP endpoints at once (e.g. multiple
//! platforms). Each endpoint uses its own `RtmpPublisher`; a failure on one
//! endpoint is isolated and reported without affecting the others.

use bytes::Bytes;

use crate::error::Result;
use crate::media::h264::H264Data;

use super::config::ClientConfig;
use super::publisher::RtmpPublisher;

/// Publishes one stream to multiple RTMP endpoints simultaneously.
pub struct MultiPublisher {
    publishers: Vec<RtmpPublisher>,
}

impl MultiPublisher {
    /// Create a publisher fanning out to the given RTMP URLs.
    pub fn new(urls: Vec<String>) -> Self {
        let publishers = urls
            .into_iter()
            .map(|url| RtmpPublisher::new(ClientConfig::new(url)).0)
            .collect();
        Self { publishers }
    }

    /// Number of configured endpoints.
    pub fn len(&self) -> usize {
        self.publishers.len()
    }

    /// Whether no endpoints are configured.
    pub fn is_empty(&self) -> bool {
        self.publishers.is_empty()
    }

    /// Connect to all endpoints.
    ///
    /// Returns one result per endpoint, in the same order as the URLs passed
    /// to `new`. A failed connection is reported here but does not stop the
    /// others from connecting.
    pub async fn connect(&mut self) -> Vec<Result<()>> {
        let mut results = Vec::with_capacity(self.publishers.len());
        for publisher in &mut self.publishers {
            results.push(publisher.connect().await);
        }
        results
    }

    /// Send a video frame to all endpoints.
    ///
    /// Returns one result per endpoint. Endpoints that fail are skipped on
    /// subsequent calls; healthy endpoints keep receiving the stream.
    pub async fn send_video(&mut self, data: &H264Data, timestamp: u32) -> Vec<Result<()>> {
        let mut results = Vec::with_capacity(self.publishers.len());
        for publisher in &mut self.publishers {
            results.push(publisher.send_video(data, timestamp).await);
        }
        results
    }

    /// Send an audio frame to all endpoints.
    ///
    /// Returns one result per endpoint (see `send_video`).
    pub async fn send_audio(&mut self, data: Bytes, timestamp: u32) -> Vec<Result<()>> {
        let mut results = Vec::with_capacity(self.publishers.len());
        for publisher in &mut self.publishers {
            results.push(publisher.send_audio(data.clone(), timestamp).await);
        }
        results
    }

    /// Disconnect from all endpoints.
    pub async fn disconnect(&mut self) {
        for publisher in &mut self.publishers {
            publisher.disconnect().await;
        }
    }
}
