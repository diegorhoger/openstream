//! Codec layer — OSCP message parse, encode, validate (`OSCP_MESSAGES.md` §2–§9).
//!
//! Status: M2 — hand-authored codec matching ADR-0005; generated artifacts
//! remain pending (`tools/codegen.json`). No transport, pairing, or platform
//! policy embedded.



/// Protocol major must always equal 1 (`PROTOCOL.md`).
pub const PROTOCOL_MAJOR: u32 = 1;
/// Effective minor for M2 codec; additive only within major.
pub const PROTOCOL_MINOR: u32 = 2;

/// Canonical UUIDv7 lowercase-hyphen form (`DOMAIN_MODEL.md` §2).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UuidV7(String);

impl UuidV7 {
    pub fn new(id: impl Into<String>) -> Self {
        let s = id.into();
        assert!(
            s.len() == 36 && !s.contains(' ') && s.chars().all(|c| c.is_ascii_hexdigit() || c == '-'),
            "invalid UUIDv7 format: {}",
            s
        );
        Self(s.to_ascii_lowercase())
    }
    pub fn as_str(&self) -> &str { &self.0 }
}

/// Envelope body kinds (§2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BodyKind {
    Hello,
    CapabilitySnapshot,
    DeckSnapshot,
    DeckPatch,
    ControlEvent,
    ExecuteRequest,
    ExecutionUpdate,
    AssetRequest,
    AssetChunk,
    Ack,
    OscpError,
    Heartbeat,
}

/// The canonical envelope (§2, `PROTOCOL.md`).
#[derive(Clone, Debug, PartialEq)]
pub struct Envelope {
    pub protocol_major: u32,
    pub protocol_minor: u32,
    pub session_id: UuidV7,
    pub sequence: u64,
    pub message_id: UuidV7,
    pub correlation_id: UuidV7,
    pub sent_at: i64, // UTC ms since epoch (fake-clock compatible)
    pub expires_at: Option<i64>,
    pub body_kind: BodyKind,
    pub body_bytes: Vec<u8>,
}

impl Envelope {
    /// Encode to a deterministic byte vector. For M2 this is a simple
    /// length-prefixed wire format; protobuf artifacts remain future.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(256);
        out.extend_from_slice(&(self.protocol_major as u32).to_le_bytes());
        out.extend_from_slice(&(self.protocol_minor as u32).to_le_bytes());
        out.extend_from_slice(&(self.session_id.as_str().len() as u32).to_le_bytes());
        out.extend_from_slice(self.session_id.as_str().as_bytes());
        out.extend_from_slice(&self.sequence.to_le_bytes());
        out.extend_from_slice(&(self.message_id.as_str().len() as u32).to_le_bytes());
        out.extend_from_slice(self.message_id.as_str().as_bytes());
        out.extend_from_slice(&(self.correlation_id.as_str().len() as u32).to_le_bytes());
        out.extend_from_slice(self.correlation_id.as_str().as_bytes());
        out.extend_from_slice(&self.sent_at.to_le_bytes());
        out.push(self.expires_at.map_or(0, |_v| 1) as u8);
        if let Some(t) = self.expires_at {
            out.extend_from_slice(&t.to_le_bytes());
        }
        out.push(self.body_kind as u8);
        out.extend_from_slice(&(self.body_bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.body_bytes);
        out
    }

    /// Decode from byte vector; fail-closed on parse failure.
    pub fn decode(mut data: &[u8]) -> Option<Self> {
        if data.len() < 8 { return None; }
        let protocol_major = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        data = &data[4..];
        let protocol_minor = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        data = &data[4..];
        let sid_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
        data = &data[4..];
        if data.len() < sid_len { return None; }
        let session_id = UuidV7::new(std::str::from_utf8(&data[..sid_len]).ok()?.to_string());
        data = &data[sid_len..];
        if data.len() < 8 { return None; }
        let sequence = u64::from_le_bytes([
            data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
        ]);
        data = &data[8..];
        if data.len() < 4 { return None; }
        let msg_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
        data = &data[4..];
        if data.len() < msg_len { return None; }
        let message_id = UuidV7::new(std::str::from_utf8(&data[..msg_len]).ok()?.to_string());
        data = &data[msg_len..];
        if data.len() < 4 { return None; }
        let corr_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
        data = &data[4..];
        if data.len() < corr_len { return None; }
        let correlation_id = UuidV7::new(std::str::from_utf8(&data[..corr_len]).ok()?.to_string());
        data = &data[corr_len..];
        if data.len() < 8 { return None; }
        let sent_at = i64::from_le_bytes([
            data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
        ]);
        data = &data[8..];
        if data.is_empty() { return None; }
        let has_expiry = data[0] == 1;
        data = &data[1..];
        let expires_at = if has_expiry {
            if data.len() < 8 { return None; }
            Some(i64::from_le_bytes([
                data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
            ]))
        } else {
            None
        };
        data = if has_expiry { &data[8..] } else { data };
        if data.is_empty() { return None; }
        let body_kind = match data[0] {
            0 => BodyKind::Hello,
            1 => BodyKind::CapabilitySnapshot,
            2 => BodyKind::DeckSnapshot,
            3 => BodyKind::DeckPatch,
            4 => BodyKind::ControlEvent,
            5 => BodyKind::ExecuteRequest,
            6 => BodyKind::ExecutionUpdate,
            7 => BodyKind::AssetRequest,
            8 => BodyKind::AssetChunk,
            9 => BodyKind::Ack,
            10 => BodyKind::OscpError,
            11 => BodyKind::Heartbeat,
            _ => return None,
        };
        data = &data[1..];
        if data.len() < 4 { return None; }
        let body_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
        data = &data[4..];
        if data.len() < body_len { return None; }
        let body_bytes = data[..body_len].to_vec();
        Some(Envelope {
            protocol_major,
            protocol_minor,
            session_id,
            sequence,
            message_id,
            correlation_id,
            sent_at,
            expires_at,
            body_kind,
            body_bytes,
        })
    }

    /// Validation stage 1: decode succeeds (§6, S1).
    /// Additional S2–S4 checks are reserved for domain-level validation.
    pub fn validate_s1(&self) -> Result<(), &'static str> {
        if self.protocol_major != PROTOCOL_MAJOR {
            return Err("PROTOCOL_MAJOR_MISMATCH");
        }
        if self.body_bytes.is_empty() && self.body_kind != BodyKind::Heartbeat {
            return Err("MALFORMED_ENVELOPE: empty body for non-heartbeat");
        }
        Ok(())
    }
}

/// Golden fixture F1: canonical encoding for `Hello`.
pub fn fixture_f1_hello() -> Envelope {
    Envelope {
        protocol_major: PROTOCOL_MAJOR,
        protocol_minor: PROTOCOL_MINOR,
        session_id: UuidV7::new("a1b2c3d4-e5f6-7a8b-9c0d-e1f2a3b4c5d6"),
        sequence: 1,
        message_id: UuidV7::new("f1e2d3c4-b5a6-7c8d-9e0f-1a2b3c4d5e6f"),
        correlation_id: UuidV7::new("f1e2d3c4-b5a6-7c8d-9e0f-1a2b3c4d5e6f"), // echoes self
        sent_at: 1_700_000_000_000,
        expires_at: None,
        body_kind: BodyKind::Hello,
        body_bytes: b"hello_fixture_v1".to_vec(),
    }
}

/// Deterministic regression: encode → decode round-trip.
#[cfg(test)]
mod codec_tests {
    use super::*;

    #[test]
    fn f1_encode_decode_roundtrip_hello() {
        let env = fixture_f1_hello();
        let bytes = env.encode();
        let decoded = Envelope::decode(&bytes).expect("decode must succeed");
        assert_eq!(env.protocol_major, decoded.protocol_major);
        assert_eq!(env.protocol_minor, decoded.protocol_minor);
        assert_eq!(env.session_id.as_str(), decoded.session_id.as_str());
        assert_eq!(env.sequence, decoded.sequence);
        assert_eq!(env.message_id.as_str(), decoded.message_id.as_str());
        assert_eq!(env.body_kind, decoded.body_kind);
    }

    #[test]
    fn s1_rejects_major_mismatch() {
        let mut env = fixture_f1_hello();
        env.protocol_major = 99;
        assert!(env.validate_s1().is_err());
    }

    #[test]
    fn decode_rejects_corrupt_bytes() {
        let corrupt = vec![0xff, 0xff, 0xff, 0xff];
        assert!(Envelope::decode(&corrupt).is_none());
    }

    #[test]
    fn property_encode_non_empty_for_hello() {
        let env = fixture_f1_hello();
        assert!(!env.encode().is_empty());
    }
}
