//! Contract tests: codec against engine runtime (`OSCP_MESSAGES.md` §8–§9,
//! `crates/openstream-engine`).

use openstream_engine::ENGINE_MAJOR;
use openstream_protocol::*;

/// Contract: engine's ENGINE_MAJOR (1) must match protocol major (§3, ADR-0005).
#[test]
fn contract_engine_protocol_major_alignment() {
    assert_eq!(PROTOCOL_MAJOR, ENGINE_MAJOR);
}

/// Contract: PROTOCOL_MINOR matches what the codec actually advertises in
/// fixtures. F1–F8 fixtures carry `protocol_minor: 2` (see
/// `tests/fixtures/f[1-8]-*.json`); the codec constant must agree.
#[test]
fn contract_engine_protocol_minor_matches_fixtures() {
    assert_eq!(PROTOCOL_MINOR, 2);
}

/// Contract: UUIDv7 format used by engine identifiers (§4).
///
/// The canonical UUIDv7 string form is 36 characters: 8-4-4-4-12 hex digits
/// separated by hyphens, with the version nibble (first hex digit of the
/// third group) equal to `7` and the variant nibble (first hex digit of the
/// fourth group) in `{8, 9, a, b}` per RFC 4122.
#[test]
fn contract_uuid_format_matches_engine_identity() {
    let id = UuidV7::new("a1b2c3d4-e5f6-7a8b-9c0d-e1f2a3b4c5d6");
    let s = id.as_str();
    // 36-character lowercase-hyphen form
    assert_eq!(s.len(), 36);
    assert!(s.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));
    // Group shape: 8-4-4-4-12
    let groups: Vec<&str> = s.split('-').collect();
    assert_eq!(groups.len(), 5);
    assert_eq!(groups[0].len(), 8);
    assert_eq!(groups[1].len(), 4);
    assert_eq!(groups[2].len(), 4);
    assert_eq!(groups[3].len(), 4);
    assert_eq!(groups[4].len(), 12);
    // Version nibble = 7 (UUIDv7 marker in the third group)
    let version_nibble = groups[2].chars().next().unwrap();
    assert_eq!(version_nibble, '7', "version nibble must be '7' for UUIDv7");
    // Variant nibble is one of {8, 9, a, b} for RFC 4122
    let variant_nibble = groups[3].chars().next().unwrap().to_ascii_lowercase();
    assert!(
        matches!(variant_nibble, '8' | '9' | 'a' | 'b'),
        "variant nibble must be 8/9/a/b for RFC 4122; got '{}'",
        variant_nibble
    );
}

/// Contract: `UuidV7::new` panics on a string that is not a valid UUIDv7
/// format. This guards the §4 invariant that engine identifiers are
/// always RFC 9562 (UUIDv7) canonical form.
#[test]
#[should_panic(expected = "invalid UUIDv7 format")]
fn contract_uuid_format_rejects_malformed() {
    // Not 36 chars; not lowercase-hyphen form; the assertion will panic.
    let _ = UuidV7::new("not-a-uuid");
}
