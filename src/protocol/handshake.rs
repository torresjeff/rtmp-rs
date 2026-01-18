//! RTMP handshake implementation
//!
//! The RTMP handshake consists of three phases:
//!
//! ```text
//! Client                                   Server
//!   |                                        |
//!   |------- C0 (1 byte: version) --------->|
//!   |------- C1 (1536 bytes: time+random) ->|
//!   |                                        |
//!   |<------ S0 (1 byte: version) ----------|
//!   |<------ S1 (1536 bytes: time+random) --|
//!   |<------ S2 (1536 bytes: echo C1) ------|
//!   |                                        |
//!   |------- C2 (1536 bytes: echo S1) ----->|
//!   |                                        |
//!   |          [Handshake Complete]          |
//! ```
//!
//! This implementation uses the "simple" handshake (no HMAC digest).
//! Complex handshake with HMAC-SHA256 is used by some servers but not required.
//!
//! Reference: RTMP Specification Section 5.2

use bytes::{Buf, BufMut, Bytes, BytesMut};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{HandshakeError, Result};
use crate::protocol::constants::{HANDSHAKE_SIZE, RTMP_VERSION};

/// Handshake role (client or server)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandshakeRole {
    Client,
    Server,
}

/// Handshake state machine
#[derive(Debug)]
pub struct Handshake {
    role: HandshakeRole,
    state: HandshakeState,
    /// Our C1/S1 packet (saved for verification)
    our_packet: Option<[u8; HANDSHAKE_SIZE]>,
    /// Peer's C1/S1 packet (saved for echo in C2/S2)
    peer_packet: Option<[u8; HANDSHAKE_SIZE]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HandshakeState {
    /// Initial state - need to send C0C1/S0S1
    Initial,
    /// Waiting for peer's C0C1/S0S1
    WaitingForPeerPacket,
    /// Received peer packet, need to send C2/S2
    NeedToSendResponse,
    /// Waiting for peer's C2/S2
    WaitingForPeerResponse,
    /// Handshake complete
    Done,
}

impl Handshake {
    /// Create a new handshake state machine
    pub fn new(role: HandshakeRole) -> Self {
        Self {
            role,
            state: HandshakeState::Initial,
            our_packet: None,
            peer_packet: None,
        }
    }

    /// Check if handshake is complete
    pub fn is_done(&self) -> bool {
        self.state == HandshakeState::Done
    }

    /// Get bytes needed before next state transition
    pub fn bytes_needed(&self) -> usize {
        match self.state {
            HandshakeState::Initial => 0,
            HandshakeState::WaitingForPeerPacket => 1 + HANDSHAKE_SIZE, // C0C1 or S0S1
            HandshakeState::NeedToSendResponse => 0,
            HandshakeState::WaitingForPeerResponse => {
                match self.role {
                    HandshakeRole::Client => HANDSHAKE_SIZE, // S2 only (S0S1 already received)
                    HandshakeRole::Server => HANDSHAKE_SIZE, // C2 only
                }
            }
            HandshakeState::Done => 0,
        }
    }

    /// Generate initial packet (C0C1 for client, nothing for server initially)
    ///
    /// For client: returns C0+C1 (1 + 1536 bytes)
    /// For server: returns None (server waits for C0C1 first)
    pub fn generate_initial(&mut self) -> Option<Bytes> {
        if self.state != HandshakeState::Initial {
            return None;
        }

        match self.role {
            HandshakeRole::Client => {
                let mut buf = BytesMut::with_capacity(1 + HANDSHAKE_SIZE);

                // C0: Version
                buf.put_u8(RTMP_VERSION);

                // C1: Time + Zero + Random
                let c1 = generate_packet();
                self.our_packet = Some(c1);
                buf.put_slice(&c1);

                self.state = HandshakeState::WaitingForPeerPacket;
                Some(buf.freeze())
            }
            HandshakeRole::Server => {
                // Server waits for client's C0C1 first
                self.state = HandshakeState::WaitingForPeerPacket;
                None
            }
        }
    }

    /// Process received data and return response if ready
    ///
    /// For server receiving C0C1: returns S0+S1+S2
    /// For client receiving S0S1S2: returns C2
    /// For server receiving C2: returns None (handshake done)
    pub fn process(&mut self, data: &mut Bytes) -> Result<Option<Bytes>> {
        match self.state {
            HandshakeState::WaitingForPeerPacket => {
                self.process_peer_packet(data)
            }
            HandshakeState::WaitingForPeerResponse => {
                self.process_peer_response(data)
            }
            _ => Ok(None),
        }
    }

    /// Process peer's initial packet (C0C1 or S0S1S2)
    fn process_peer_packet(&mut self, data: &mut Bytes) -> Result<Option<Bytes>> {
        match self.role {
            HandshakeRole::Server => {
                // Expecting C0 + C1
                if data.remaining() < 1 + HANDSHAKE_SIZE {
                    return Ok(None); // Need more data
                }

                // C0: Version check
                let version = data.get_u8();
                if version != RTMP_VERSION {
                    // Be lenient - accept version 3-31 (some encoders send different values)
                    if version < 3 {
                        return Err(HandshakeError::InvalidVersion(version).into());
                    }
                }

                // C1: Save peer packet
                let mut c1 = [0u8; HANDSHAKE_SIZE];
                data.copy_to_slice(&mut c1);
                self.peer_packet = Some(c1);

                // Generate S0 + S1 + S2
                let mut response = BytesMut::with_capacity(1 + HANDSHAKE_SIZE * 2);

                // S0: Version
                response.put_u8(RTMP_VERSION);

                // S1: Our packet
                let s1 = generate_packet();
                self.our_packet = Some(s1);
                response.put_slice(&s1);

                // S2: Echo C1 with our timestamp
                let s2 = generate_echo(&c1);
                response.put_slice(&s2);

                self.state = HandshakeState::WaitingForPeerResponse;
                Ok(Some(response.freeze()))
            }
            HandshakeRole::Client => {
                // Expecting S0 + S1 + S2
                if data.remaining() < 1 + HANDSHAKE_SIZE * 2 {
                    return Ok(None); // Need more data
                }

                // S0: Version check
                let version = data.get_u8();
                if version != RTMP_VERSION && version < 3 {
                    return Err(HandshakeError::InvalidVersion(version).into());
                }

                // S1: Save peer packet
                let mut s1 = [0u8; HANDSHAKE_SIZE];
                data.copy_to_slice(&mut s1);
                self.peer_packet = Some(s1);

                // S2: Verify echo of C1 (lenient - just consume)
                let mut s2 = [0u8; HANDSHAKE_SIZE];
                data.copy_to_slice(&mut s2);

                // In lenient mode, don't strictly verify S2 matches C1
                // Some servers don't echo correctly

                // Generate C2: Echo S1
                let c2 = generate_echo(&s1);

                self.state = HandshakeState::Done;
                Ok(Some(Bytes::copy_from_slice(&c2)))
            }
        }
    }

    /// Process peer's response (C2 for server)
    fn process_peer_response(&mut self, data: &mut Bytes) -> Result<Option<Bytes>> {
        match self.role {
            HandshakeRole::Server => {
                // Expecting C2
                if data.remaining() < HANDSHAKE_SIZE {
                    return Ok(None);
                }

                // C2: Verify echo of S1 (lenient)
                let mut c2 = [0u8; HANDSHAKE_SIZE];
                data.copy_to_slice(&mut c2);

                // Lenient: don't strictly verify C2 matches S1
                self.state = HandshakeState::Done;
                Ok(None)
            }
            HandshakeRole::Client => {
                // Client shouldn't be in this state
                self.state = HandshakeState::Done;
                Ok(None)
            }
        }
    }
}

/// Generate a handshake packet (C1 or S1)
///
/// Format (1536 bytes):
/// - Bytes 0-3: Timestamp (32-bit, big-endian)
/// - Bytes 4-7: Zero (for simple handshake) or version (for complex)
/// - Bytes 8-1535: Random data
fn generate_packet() -> [u8; HANDSHAKE_SIZE] {
    let mut packet = [0u8; HANDSHAKE_SIZE];

    // Timestamp: milliseconds since some epoch
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u32)
        .unwrap_or(0);

    packet[0..4].copy_from_slice(&timestamp.to_be_bytes());

    // Zero field (simple handshake)
    packet[4..8].copy_from_slice(&[0, 0, 0, 0]);

    // Random data - use simple PRNG seeded with timestamp
    // Not cryptographically secure, but RTMP handshake doesn't require it
    let mut seed = timestamp as u64;
    for chunk in packet[8..].chunks_mut(8) {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let bytes = seed.to_le_bytes();
        let len = chunk.len().min(8);
        chunk[..len].copy_from_slice(&bytes[..len]);
    }

    packet
}

/// Generate echo packet (C2 or S2)
///
/// Format:
/// - Bytes 0-3: Peer's timestamp (from their C1/S1)
/// - Bytes 4-7: Our timestamp
/// - Bytes 8-1535: Copy of peer's random data
fn generate_echo(peer_packet: &[u8; HANDSHAKE_SIZE]) -> [u8; HANDSHAKE_SIZE] {
    let mut echo = *peer_packet;

    // Bytes 4-7: Our receive timestamp
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u32)
        .unwrap_or(0);

    echo[4..8].copy_from_slice(&timestamp.to_be_bytes());

    echo
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_server_handshake() {
        let mut client = Handshake::new(HandshakeRole::Client);
        let mut server = Handshake::new(HandshakeRole::Server);

        // Client generates C0C1
        let c0c1 = client.generate_initial().expect("Client should generate C0C1");
        assert_eq!(c0c1.len(), 1 + HANDSHAKE_SIZE);

        // Server receives C0C1, generates S0S1S2
        let mut c0c1_buf = c0c1;
        server.generate_initial(); // Move server to waiting state
        let s0s1s2 = server.process(&mut c0c1_buf).unwrap().expect("Server should generate S0S1S2");
        assert_eq!(s0s1s2.len(), 1 + HANDSHAKE_SIZE * 2);

        // Client receives S0S1S2, generates C2
        let mut s0s1s2_buf = s0s1s2;
        let c2 = client.process(&mut s0s1s2_buf).unwrap().expect("Client should generate C2");
        assert_eq!(c2.len(), HANDSHAKE_SIZE);
        assert!(client.is_done());

        // Server receives C2
        let mut c2_buf = c2;
        let response = server.process(&mut c2_buf).unwrap();
        assert!(response.is_none());
        assert!(server.is_done());
    }

    #[test]
    fn test_packet_generation() {
        let packet = generate_packet();

        // Should have timestamp in first 4 bytes
        let timestamp = u32::from_be_bytes([packet[0], packet[1], packet[2], packet[3]]);
        assert!(timestamp > 0); // Should be non-zero for reasonable system time

        // Bytes 4-7 should be zero (simple handshake)
        assert_eq!(&packet[4..8], &[0, 0, 0, 0]);
    }
}
