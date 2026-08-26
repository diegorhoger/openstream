//! Acceptance AC-20-HI-01 through AC-20-HI-06: hostile-input matrix.
//!
//! Every bundle parser defense is exercised with a crafted malicious input.
//! Inputs are constructed by splicing raw binary frames — the public API
//! cannot produce these malformed states, so the tests prove the defenses
//! hold against an adversarial bundle.

use openstream_bundle::BundleError;
use openstream_bundle::limits::{
    BUNDLE_FORMAT_VERSION, MAGIC, MAX_DECOMPRESSION_RATIO, MAX_MEMBER_COUNT,
};
use openstream_bundle::parse_bundle;

mod common;

// ---------------------------------------------------------------------------
// Helpers: raw-frame construction
// ---------------------------------------------------------------------------

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

/// Build a raw frame header (magic + version + member_count).
fn frame_header(member_count: u32) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&BUNDLE_FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&member_count.to_le_bytes());
    out
}

/// Frame a single stored member: name_len + name + raw_len + 0 (stored) + stored_len + payload.
fn frame_member(name: &str, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(name.len() as u32).to_le_bytes());
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.push(0u8); // stored, not deflated
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    out
}

/// Build a complete raw frame from header + members.
fn raw_frame(members: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = frame_header(members.len() as u32);
    for (name, payload) in members {
        out.extend_from_slice(&frame_member(name, payload));
    }
    out
}

// ---------------------------------------------------------------------------
// AC-20-HI-01: invalid magic
// ---------------------------------------------------------------------------

#[test]
fn invalid_magic_rejects() {
    let mut frame = raw_frame(&[("manifest.json", b"{}")]);
    frame[0] = b'X'; // corrupt the first byte of "OSTRBNDL"
    assert_eq!(parse_bundle(&frame).unwrap_err(), BundleError::InvalidMagic);
}

// ---------------------------------------------------------------------------
// AC-20-HI-02: unsupported container version
// ---------------------------------------------------------------------------

#[test]
fn unsupported_container_version_rejects() {
    let mut frame = raw_frame(&[("manifest.json", b"{}")]);
    // Overwrite the u32 version at offset 8..12.
    let bad_version = BUNDLE_FORMAT_VERSION + 1;
    frame[8..12].copy_from_slice(&bad_version.to_le_bytes());
    assert_eq!(
        parse_bundle(&frame).unwrap_err(),
        BundleError::UnsupportedContainerVersion {
            found: bad_version,
            supported: BUNDLE_FORMAT_VERSION,
        }
    );
}

// ---------------------------------------------------------------------------
// AC-20-HI-03: truncated frame
// ---------------------------------------------------------------------------

#[test]
fn truncated_frame_rejects() {
    let frame = raw_frame(&[("manifest.json", b"{}")]);
    // Truncate to just the header (missing all member data).
    let truncated = &frame[..12];
    let err = parse_bundle(truncated);
    assert!(err.is_err(), "truncated frame must reject");
}

#[test]
fn truncated_member_payload_rejects() {
    let mut frame = raw_frame(&[("manifest.json", b"{}")]);
    // Chop the last 2 bytes off.
    frame.truncate(frame.len() - 2);
    assert!(parse_bundle(&frame).is_err());
}

// ---------------------------------------------------------------------------
// AC-20-HI-04: hash mismatch
// ---------------------------------------------------------------------------

#[test]
fn hash_mismatch_rejects() {
    let deck = common::minimal_deck();
    let profile = common::minimal_profile();
    let deck_json = serde_json::to_vec(&deck).expect("deck json");
    let profile_json = serde_json::to_vec(&profile).expect("profile json");

    // Build a manifest that claims a wrong hash for the deck.
    let wrong_hash = "0".repeat(64);
    let manifest_json = serde_json::to_vec(&serde_json::json!({
        "schema_version": { "major": 1, "minor": 0 },
        "counts": { "decks": 1, "profiles": 1 },
        "entries": [
            { "name": format!("deck/{}.json", common::DECK_UUID), "sha256": wrong_hash },
            { "name": format!("profile/{}.json", common::PROFILE_UUID), "sha256": sha256_hex(&profile_json) },
        ]
    }))
    .expect("manifest json");

    let frame = raw_frame(&[
        ("manifest.json", &manifest_json),
        (&format!("deck/{}.json", common::DECK_UUID), &deck_json),
        (
            &format!("profile/{}.json", common::PROFILE_UUID),
            &profile_json,
        ),
    ]);
    assert!(matches!(
        parse_bundle(&frame),
        Err(BundleError::HashMismatch { .. })
    ));
}

// ---------------------------------------------------------------------------
// AC-20-HI-05: decompression-ratio bomb
// ---------------------------------------------------------------------------

#[test]
fn decompression_ratio_bomb_rejects() {
    use openstream_bundle::limits::RATIO_GUARD_FLOOR_BYTES;

    // Craft a deflated member with stored_len > RATIO_GUARD_FLOOR_BYTES
    // and raw_len exceeding the ratio cap.
    let stored_len = (RATIO_GUARD_FLOOR_BYTES + 1) as usize;
    let raw_len = (stored_len as u64 * MAX_DECOMPRESSION_RATIO + 1) as usize;

    let mut frame = frame_header(1);
    // member: "manifest.json"
    frame.extend_from_slice(&(b"manifest.json".len() as u32).to_le_bytes());
    frame.extend_from_slice(b"manifest.json");
    frame.extend_from_slice(&(raw_len as u32).to_le_bytes());
    frame.push(1u8); // deflate codec
    frame.extend_from_slice(&(stored_len as u32).to_le_bytes());
    // Payload: just zeros (not valid deflate, but ratio check happens first).
    frame.extend_from_slice(&vec![0u8; stored_len]);

    assert!(matches!(
        parse_bundle(&frame),
        Err(BundleError::CompressionRatioExceeded { .. })
    ));
}

// ---------------------------------------------------------------------------
// AC-20-HI-06: illegal member names (path traversal, homoglyphs)
// ---------------------------------------------------------------------------

#[test]
fn illegal_member_name_traversal_rejects() {
    let payload = b"{}";
    let frame = raw_frame(&[("../escape.json", payload)]);
    assert!(matches!(
        parse_bundle(&frame),
        Err(BundleError::IllegalMemberName { .. })
    ));
}

#[test]
fn illegal_member_name_absolute_path_rejects() {
    let payload = b"{}";
    let frame = raw_frame(&[("/etc/passwd", payload)]);
    assert!(matches!(
        parse_bundle(&frame),
        Err(BundleError::IllegalMemberName { .. })
    ));
}

#[test]
fn illegal_member_name_uppercase_rejects() {
    let payload = b"{}";
    let frame = raw_frame(&[("DECK/018f6a1c-7b21-7002-9f31-000000000002.json", payload)]);
    assert!(matches!(
        parse_bundle(&frame),
        Err(BundleError::IllegalMemberName { .. })
    ));
}

#[test]
fn illegal_member_name_backslash_rejects() {
    let payload = b"{}";
    let frame = raw_frame(&[("deck\\evil.json", payload)]);
    assert!(matches!(
        parse_bundle(&frame),
        Err(BundleError::IllegalMemberName { .. })
    ));
}

// ---------------------------------------------------------------------------
// Trailing bytes
// ---------------------------------------------------------------------------

#[test]
fn trailing_bytes_after_last_member_rejects() {
    let mut frame = raw_frame(&[("manifest.json", b"{}")]);
    frame.extend_from_slice(&[0xDE, 0xAD]); // trailing garbage
    assert!(matches!(
        parse_bundle(&frame),
        Err(BundleError::MalformedFrame { .. })
    ));
}

// ---------------------------------------------------------------------------
// Duplicate member names
// ---------------------------------------------------------------------------

#[test]
fn duplicate_member_rejects() {
    let manifest_json = b"{}";
    let mut frame = frame_header(2);
    // First member: manifest.json
    frame.extend_from_slice(&frame_member("manifest.json", manifest_json));
    // Second member: also manifest.json
    frame.extend_from_slice(&frame_member("manifest.json", manifest_json));
    assert!(matches!(
        parse_bundle(&frame),
        Err(BundleError::DuplicateMember)
    ));
}

// ---------------------------------------------------------------------------
// Member count exceeds cap
// ---------------------------------------------------------------------------

#[test]
fn member_count_exceeds_cap_rejects() {
    let mut frame = frame_header((MAX_MEMBER_COUNT + 1) as u32);
    // Add one valid manifest member.
    frame.extend_from_slice(&frame_member("manifest.json", b"{}"));
    assert!(matches!(
        parse_bundle(&frame),
        Err(BundleError::TooLarge { .. })
    ));
}

// ---------------------------------------------------------------------------
// Manifest not first member
// ---------------------------------------------------------------------------

#[test]
fn manifest_not_first_member_rejects() {
    let deck = common::minimal_deck();
    let profile = common::minimal_profile();
    let deck_json = serde_json::to_vec(&deck).expect("deck json");
    let profile_json = serde_json::to_vec(&profile).expect("profile json");

    let manifest_json = serde_json::to_vec(&serde_json::json!({
        "schema_version": { "major": 1, "minor": 0 },
        "counts": { "decks": 1, "profiles": 1 },
        "entries": [
            { "name": format!("deck/{}.json", common::DECK_UUID), "sha256": sha256_hex(&deck_json) },
            { "name": format!("profile/{}.json", common::PROFILE_UUID), "sha256": sha256_hex(&profile_json) },
        ]
    }))
    .expect("manifest json");

    // Deck before manifest.
    let frame = raw_frame(&[
        (&format!("deck/{}.json", common::DECK_UUID), &deck_json),
        ("manifest.json", &manifest_json),
        (
            &format!("profile/{}.json", common::PROFILE_UUID),
            &profile_json,
        ),
    ]);
    assert!(matches!(
        parse_bundle(&frame),
        Err(BundleError::MalformedFrame {
            reason: "manifest must be the first member"
        })
    ));
}

// ---------------------------------------------------------------------------
// Manifest missing
// ---------------------------------------------------------------------------

#[test]
fn manifest_member_missing_rejects() {
    let deck = common::minimal_deck();
    let deck_json = serde_json::to_vec(&deck).expect("deck json");
    let frame = raw_frame(&[(&format!("deck/{}.json", common::DECK_UUID), &deck_json)]);
    assert!(matches!(
        parse_bundle(&frame),
        Err(BundleError::MalformedFrame {
            reason: "manifest member missing"
        })
    ));
}
