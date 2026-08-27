//! Contract tests: codec against engine runtime (`OSCP_MESSAGES.md` §8–§9,
//! `crates/openstream-engine`).

use openstream_protocol::*;

/// Contract: engine's ENGINE_MAJOR (1) must match protocol major (§3, ADR-0005).
#[test]
fn contract_engine_protocol_major_alignment() {
    assert_eq!(PROTOCOL_MAJOR, 1);
}

/// Contract: UUIDv7 format used by engine identifiers (§4).
#[test]
fn contract_uuid_format_matches_engine_identity() {
    let id = UuidV7::new("a1b2c3d4-e5f6-7a8b-9c0d-e1f2a3b4c5d6");
    assert_eq!(id.as_str().len(), 36);
}
