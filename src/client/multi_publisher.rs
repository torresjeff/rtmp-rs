//! Multi-platform RTMP publisher
//!
//! Publishes the same stream to several RTMP endpoints at once (e.g. multiple
//! platforms). Each endpoint runs in its own tokio task with a bounded mpsc
//! buffer, so a slow or failing endpoint does not block the others.
//!
//! Connections are established concurrently and reported through the event
//! channel (`PlatformConnected` / `PlatformConnectFailed`), so callers can
//! start pushing as soon as their preferred endpoint is ready without waiting
//! for all endpoints. This provides concurrency isolation and disconnect
//! events, but does **not** implement reconnection/backoff or frame-dropping
//! policies. Callers receive [`MultiPublishEvent`]s and can call
//! [`reconnect`](MultiPublisher::reconnect) to recover an endpoint with their
//! own strategy.

use std::sync::{Arc, Mutex};

use bytes::Bytes;
use tokio::sync::mpsc;

use crate::error::{Error, Result};
use crate::media::aac::AacData;
use crate::media::h264::H264Data;

use super::config::ClientConfig;
use super::publisher::RtmpPublisher;

/// Bounded channel capacity per endpoint (matches `RtmpPublisher` event channel).
const BUFFER_CAPACITY: usize = 256;

/// Commands sent to a per-endpoint task.
enum FrameCommand {
    Video {
        data: H264Data,
        timestamp: u32,
    },
    Audio {
        data: AacData,
        timestamp: u32,
    },
    AudioRaw {
        data: Bytes,
        timestamp: u32,
    },
}

/// Events from the multi-platform publisher.
#[derive(Debug)]
pub enum MultiPublishEvent {
    /// An endpoint's connection attempt succeeded; its task is ready to
    /// accept frames.
    PlatformConnected {
        id: usize,
    },
    /// An endpoint's connection attempt failed or timed out.
    PlatformConnectFailed {
        id: usize,
        err: String,
    },
    /// An endpoint's send failed (non-fatal to other endpoints).
    PlatformError {
        id: usize,
        err: String,
    },
    /// An endpoint disconnected (send failed or task exited).
    PlatformDisconnected {
        id: usize,
    },
}

/// Per-endpoint sender slots, shared with the spawned connect tasks so they
/// can register their channel once connected.
type SharedSenders = Arc<Mutex<Vec<Option<mpsc::Sender<FrameCommand>>>>>;

/// Latest AVC/AAC sequence headers, replayed to every endpoint on connect so
/// raw frames never precede SPS/PPS + AAC config.
#[derive(Default)]
struct SequenceHeaders {
    video: Option<H264Data>,
    audio: Option<AacData>,
}

/// Publishes one stream to multiple RTMP endpoints simultaneously.
///
/// Each endpoint runs in its own tokio task with a bounded mpsc buffer,
/// providing concurrency isolation: a slow endpoint does not block others.
/// Connection outcomes and send failures are reported via
/// [`MultiPublishEvent`]; callers can [`reconnect`](Self::reconnect) to
/// recover an endpoint.
pub struct MultiPublisher {
    configs: Vec<ClientConfig>,
    event_tx: mpsc::Sender<MultiPublishEvent>,
    senders: SharedSenders,
    sequence_headers: Arc<Mutex<SequenceHeaders>>,
}

impl MultiPublisher {
    /// Create a publisher fanning out to the given RTMP URLs.
    ///
    /// Returns the publisher and a receiver for [`MultiPublishEvent`]s.
    pub fn new(urls: Vec<String>) -> (Self, mpsc::Receiver<MultiPublishEvent>) {
        let (event_tx, event_rx) = mpsc::channel(BUFFER_CAPACITY);
        let configs: Vec<ClientConfig> = urls.into_iter().map(ClientConfig::new).collect();
        let senders = Arc::new(Mutex::new(configs.iter().map(|_| None).collect()));
        (
            Self {
                configs,
                event_tx,
                senders,
                sequence_headers: Arc::new(Mutex::new(SequenceHeaders::default())),
            },
            event_rx,
        )
    }

    /// Number of configured endpoints.
    pub fn len(&self) -> usize {
        self.configs.len()
    }

    /// Whether no endpoints are configured.
    pub fn is_empty(&self) -> bool {
        self.configs.is_empty()
    }

    /// Start connecting to all endpoints concurrently.
    ///
    /// Returns immediately; each endpoint reports `PlatformConnected` or
    /// `PlatformConnectFailed` on the event channel. Connects run in
    /// parallel and are individually bounded by
    /// [`ClientConfig::connect_timeout`], so one slow or unreachable endpoint
    /// does not delay the others.
    ///
    /// Frames may be sent before an endpoint is ready — they are dropped
    /// (`Err(ConnectionClosed)`) until that endpoint's `PlatformConnected`
    /// event arrives. Sequence headers set via
    /// [`set_sequence_headers`](Self::set_sequence_headers) are replayed to
    /// an endpoint before its first raw frame, so a late-joining or
    /// reconnected endpoint always has SPS/PPS + AAC config.
    pub fn connect(&self) {
        for id in 0..self.configs.len() {
            let senders = lock_senders(&self.senders);
            let alive = senders[id].as_ref().is_some_and(|tx| !tx.is_closed());
            drop(senders);
            if !alive {
                self.connect_one(id);
            }
        }
    }

    /// Remember the current AVC/AAC sequence headers.
    ///
    /// Every endpoint receives them replayed right after it connects, before
    /// any raw frame, so a late-joining or reconnected endpoint has
    /// SPS/PPS + AAC config without the caller racing the connection. Call
    /// this whenever a new sequence header appears (e.g. stream restart).
    pub fn set_sequence_headers(&self, video: Option<&H264Data>, audio: Option<&AacData>) {
        let mut seq = self.sequence_headers.lock().unwrap_or_else(|e| e.into_inner());
        seq.video = video.cloned();
        seq.audio = audio.cloned();
    }

    /// Reconnect to a previously failed or disconnected endpoint.
    ///
    /// Returns an error if the endpoint is still connected or `id` is
    /// invalid. Like [`connect`](Self::connect), the attempt is concurrent
    /// and its outcome is reported via `PlatformConnected` /
    /// `PlatformConnectFailed`. The caller is responsible for any
    /// backoff/retry strategy; the cached sequence headers are replayed on
    /// `PlatformConnected` (see [`set_sequence_headers`](Self::set_sequence_headers)),
    /// so no manual re-send is needed.
    pub fn reconnect(&self, id: usize) -> Result<()> {
        let mut senders = lock_senders(&self.senders);
        let slot = senders
            .get_mut(id)
            .ok_or_else(|| Error::Config("invalid platform id".into()))?;
        if slot.as_ref().is_some_and(|tx| !tx.is_closed()) {
            return Err(Error::Config(
                "endpoint still connected; disconnect first".into(),
            ));
        }
        drop(senders);
        self.connect_one(id);
        Ok(())
    }

    /// Send a video frame to all endpoints.
    ///
    /// Returns one result per endpoint. `Ok(())` means the frame was accepted
    /// into the endpoint's buffer. `Err(BufferFull)` means the endpoint is
    /// alive but its buffer is full — the caller may drop the frame or retry.
    /// `Err(ConnectionClosed)` means the endpoint is not (yet) connected or
    /// its task has exited.
    pub fn send_video(&self, data: &H264Data, timestamp: u32) -> Vec<Result<()>> {
        let mut senders = lock_senders(&self.senders);
        let mut results = Vec::with_capacity(senders.len());
        for slot in senders.iter_mut() {
            results.push(dispatch(slot, FrameCommand::Video {
                data: data.clone(),
                timestamp,
            }));
        }
        results
    }

    /// Send an audio frame to all endpoints.
    ///
    /// Returns one result per endpoint (see [`send_video`](Self::send_video)).
    /// Each endpoint derives and caches its own `aac_format_byte` from the
    /// sequence header.
    pub fn send_audio(&self, data: &AacData, timestamp: u32) -> Vec<Result<()>> {
        let mut senders = lock_senders(&self.senders);
        let mut results = Vec::with_capacity(senders.len());
        for slot in senders.iter_mut() {
            results.push(dispatch(slot, FrameCommand::Audio {
                data: data.clone(),
                timestamp,
            }));
        }
        results
    }

    /// Send a pre-built FLV audio tag body to all endpoints.
    ///
    /// Returns one result per endpoint (see [`send_video`](Self::send_video)).
    /// Does not update the cached `aac_format_byte` on any endpoint.
    pub fn send_audio_raw(&self, data: Bytes, timestamp: u32) -> Vec<Result<()>> {
        let mut senders = lock_senders(&self.senders);
        let mut results = Vec::with_capacity(senders.len());
        for slot in senders.iter_mut() {
            results.push(dispatch(slot, FrameCommand::AudioRaw {
                data: data.clone(),
                timestamp,
            }));
        }
        results
    }

    /// Disconnect from all endpoints.
    ///
    /// Drops each endpoint's send channel; its task notices the closed channel
    /// and disconnects asynchronously. Frames sent before the tasks exit are
    /// dropped (`Err(ConnectionClosed)`). A caller that immediately
    /// [`connect`](Self::connect)s again should be aware that a previous task
    /// may still be winding down, and its `PlatformDisconnected` event can
    /// arrive after the new connection is established.
    pub fn disconnect(&self) {
        let mut senders = lock_senders(&self.senders);
        for slot in senders.iter_mut() {
            *slot = None;
        }
    }

    /// Spawn one endpoint's connection task.
    ///
    /// On success it spawns the endpoint's frame task, registers its channel
    /// and emits `PlatformConnected`; on failure or timeout it emits
    /// `PlatformConnectFailed`.
    ///
    /// The sender slot is re-checked under the lock before connecting and
    /// again before registering, so a concurrent `connect`/`reconnect` cannot
    /// double-register an endpoint or clobber a live sender: the loser
    /// disconnects and exits.
    fn connect_one(&self, id: usize) {
        let config = self.configs[id].clone();
        let event_tx = self.event_tx.clone();
        let senders = self.senders.clone();
        let sequence_headers = self.sequence_headers.clone();
        tokio::spawn(async move {
            // A concurrent `connect`/`reconnect` may already have this
            // endpoint live — bail instead of connecting a second time.
            if is_alive(&senders, id) {
                return;
            }

            let timeout = config.connect_timeout;
            let (mut publisher, _events) = RtmpPublisher::new(config);
            match tokio::time::timeout(timeout, publisher.connect()).await {
                Ok(Ok(())) => {
                    let (tx, rx) = mpsc::channel(BUFFER_CAPACITY);
                    // Replay the cached sequence headers into the fresh
                    // channel BEFORE the slot goes live, so raw frames can
                    // never precede SPS/PPS + AAC config in this endpoint's
                    // buffer.
                    {
                        let seq = sequence_headers.lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(v) = &seq.video {
                            let _ = tx.try_send(FrameCommand::Video {
                                data: v.clone(),
                                timestamp: 0,
                            });
                        }
                        if let Some(a) = &seq.audio {
                            let _ = tx.try_send(FrameCommand::Audio {
                                data: a.clone(),
                                timestamp: 0,
                            });
                        }
                    }
                    // A newer connect won the race — don't clobber its live
                    // sender; abandon this connection instead.
                    if !try_register(&senders, id, tx) {
                        publisher.disconnect().await;
                        return;
                    }
                    tokio::spawn(platform_task(id, publisher, rx, event_tx.clone()));
                    let _ = event_tx
                        .send(MultiPublishEvent::PlatformConnected { id })
                        .await;
                }
                Ok(Err(e)) => {
                    let _ = event_tx
                        .send(MultiPublishEvent::PlatformConnectFailed {
                            id,
                            err: e.to_string(),
                        })
                        .await;
                }
                Err(_) => {
                    let _ = event_tx
                        .send(MultiPublishEvent::PlatformConnectFailed {
                            id,
                            err: "connect timed out".into(),
                        })
                        .await;
                }
            }
        });
    }
}

fn buffer_err() -> Result<()> {
    Err(Error::BufferFull)
}

/// Lock the shared senders, recovering from a poisoned mutex so a panic in
/// another task can never wedge an endpoint in the "connecting" limbo. The
/// poisoned data is still valid — each slot is a plain `Option` swap.
fn lock_senders(
    senders: &SharedSenders,
) -> std::sync::MutexGuard<'_, Vec<Option<mpsc::Sender<FrameCommand>>>> {
    senders.lock().unwrap_or_else(|e| e.into_inner())
}

/// Whether `id` currently has a live sender (its frame task is running).
///
/// Lock is acquired and released inside this sync helper so a non-`Send`
/// `MutexGuard` never crosses into the spawned connect task.
fn is_alive(senders: &SharedSenders, id: usize) -> bool {
    let senders = senders.lock().unwrap_or_else(|e| e.into_inner());
    senders[id].as_ref().is_some_and(|tx| !tx.is_closed())
}

/// Register `tx` for `id` unless a live sender already exists.
///
/// Returns `true` when registered; `false` when a concurrent connect won the
/// race and this attempt should be abandoned.
fn try_register(
    senders: &SharedSenders,
    id: usize,
    tx: mpsc::Sender<FrameCommand>,
) -> bool {
    let mut senders = senders.lock().unwrap_or_else(|e| e.into_inner());
    if senders[id].as_ref().is_some_and(|t| !t.is_closed()) {
        false
    } else {
        senders[id] = Some(tx);
        true
    }
}

fn not_connected_err() -> Result<()> {
    Err(Error::ConnectionClosed)
}

/// Try to enqueue a command to an endpoint's task.
///
/// - `Ok(())`: enqueued.
/// - `Err(buffer full)`: endpoint alive but its mpsc is full (caller may drop
///   frame or retry).
/// - `Err(not connected)`: endpoint was never connected, or its task exited
///   (channel closed). On `Closed`, the slot is cleared so the caller can
///   [`reconnect`](MultiPublisher::reconnect) without hitting "still connected".
fn dispatch(slot: &mut Option<mpsc::Sender<FrameCommand>>, cmd: FrameCommand) -> Result<()> {
    let tx = match slot.take() {
        Some(tx) => tx,
        None => return not_connected_err(),
    };
    match tx.try_send(cmd) {
        Ok(()) => {
            *slot = Some(tx);
            Ok(())
        }
        Err(mpsc::error::TrySendError::Full(_)) => {
            *slot = Some(tx);
            buffer_err()
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            // Task exited; leave the slot cleared so reconnect works.
            not_connected_err()
        }
    }
}

/// Per-endpoint task: drains the command channel and forwards to the publisher.
///
/// On send failure, emits `PlatformError` + `PlatformDisconnected` and exits.
/// On channel close (all senders dropped), disconnects silently and exits.
async fn platform_task(
    id: usize,
    mut publisher: RtmpPublisher,
    mut rx: mpsc::Receiver<FrameCommand>,
    event_tx: mpsc::Sender<MultiPublishEvent>,
) {
    while let Some(cmd) = rx.recv().await {
        let result = match cmd {
            FrameCommand::Video { data, timestamp } => publisher.send_video(&data, timestamp).await,
            FrameCommand::Audio { data, timestamp } => publisher.send_audio(&data, timestamp).await,
            FrameCommand::AudioRaw { data, timestamp } => {
                publisher.send_audio_raw(data, timestamp).await
            }
        };
        if let Err(e) = result {
            let _ = event_tx
                .send(MultiPublishEvent::PlatformError {
                    id,
                    err: e.to_string(),
                })
                .await;
            publisher.disconnect().await;
            let _ = event_tx
                .send(MultiPublishEvent::PlatformDisconnected { id })
                .await;
            return;
        }
    }
    // Channel closed (tx dropped) — disconnect silently.
    publisher.disconnect().await;
}
