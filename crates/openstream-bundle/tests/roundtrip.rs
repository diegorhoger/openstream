//! Acceptance AC-20-RT-01: exact round-trip and atomic restore.
//!
//! Export → import → re-export must be byte-identical, and restoring a
//! parsed bundle into a persistence store must reproduce the original
//! workspace semantics.

mod common;

use openstream_bundle::{build_bundle, parse_bundle};
use tempfile::tempdir;

/// AC-20-RT-01a: round-trip with single deck and single profile is
/// byte-identical across two cycles.
#[test]
fn single_deck_single_profile_roundtrip() {
    let decks = vec![common::minimal_deck()];
    let profiles = vec![common::minimal_profile()];

    let bytes_v1 = build_bundle(&decks, &profiles).expect("first build");
    let parsed = parse_bundle(&bytes_v1).expect("first parse");
    assert_eq!(parsed.decks.len(), 1);
    assert_eq!(parsed.profiles.len(), 1);
    assert_eq!(parsed.decks[0].deck.id, common::minimal_deck().deck.id);
    assert_eq!(
        parsed.profiles[0].profile.id,
        common::minimal_profile().profile.id
    );

    // Re-export from the parsed result.
    let bytes_v2 = build_bundle(&parsed.decks, &parsed.profiles).expect("second build");
    assert_eq!(bytes_v1, bytes_v2, "byte-identical round-trip");
}

/// AC-20-RT-01b: round-trip with multiple decks and profiles.
#[test]
fn multi_deck_profile_roundtrip() {
    let decks = vec![common::minimal_deck(), common::second_deck()];
    let profiles = vec![common::minimal_profile(), common::second_profile()];

    let bytes_v1 = build_bundle(&decks, &profiles).expect("build");
    let parsed = parse_bundle(&bytes_v1).expect("parse");

    let bytes_v2 = build_bundle(&parsed.decks, &parsed.profiles).expect("rebuild");
    assert_eq!(bytes_v1, bytes_v2, "multi-deck profile byte-identical");
}

/// AC-20-RT-01c: round-trip with profile that carries a hotkey switch rule.
#[test]
fn hotkey_switch_rule_roundtrip() {
    let decks = vec![common::minimal_deck()];
    let profiles = vec![common::profile_with_hotkey()];

    let bytes_v1 = build_bundle(&decks, &profiles).expect("build");
    let parsed = parse_bundle(&bytes_v1).expect("parse");

    assert_eq!(parsed.profiles[0].profile.switch_rules.len(), 1);
    let bytes_v2 = build_bundle(&parsed.decks, &parsed.profiles).expect("rebuild");
    assert_eq!(bytes_v1, bytes_v2);
}

/// AC-20-RT-01d: empty workspace round-trips to identical empty bundle.
#[test]
fn empty_workspace_roundtrip() {
    let bytes_v1 = build_bundle(&[], &[]).expect("empty build");
    let parsed = parse_bundle(&bytes_v1).expect("empty parse");
    assert!(parsed.decks.is_empty());
    assert!(parsed.profiles.is_empty());

    let bytes_v2 = build_bundle(&parsed.decks, &parsed.profiles).expect("empty rebuild");
    assert_eq!(bytes_v1, bytes_v2);
}

/// AC-20-RT-02: write_bundle_file + read_bundle_file round-trips through
/// the filesystem.
#[test]
fn file_roundtrip() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("test.openstream");

    let decks = vec![common::minimal_deck(), common::second_deck()];
    let profiles = vec![common::minimal_profile(), common::second_profile()];
    let bytes = build_bundle(&decks, &profiles).expect("build");

    openstream_bundle::write_bundle_file(&path, &bytes).expect("write");
    let read_back = openstream_bundle::read_bundle_file(&path).expect("read");
    assert_eq!(bytes, read_back, "file round-trip");

    // Write again: the previous copy is rotated aside (.prev).
    openstream_bundle::write_bundle_file(&path, &bytes).expect("second write");
    let read_back2 = openstream_bundle::read_bundle_file(&path).expect("read again");
    assert_eq!(bytes, read_back2);
}

/// AC-20-RT-03: semantic identity across builds — imported documents
/// carry the same revision, IDs, and structural content.
#[test]
fn semantic_identity_preserved() {
    let decks = vec![common::minimal_deck()];
    let profiles = vec![common::minimal_profile()];

    let bytes = build_bundle(&decks, &profiles).expect("build");
    let parsed = parse_bundle(&bytes).expect("parse");

    // Deck revision, ID, and title survive exactly.
    assert_eq!(parsed.decks[0].deck.revision, 1);
    assert_eq!(parsed.decks[0].deck.title.as_str(), "Test Deck");
    assert_eq!(parsed.decks[0].deck.id, common::minimal_deck().deck.id);

    // Profile name and deck_ids survive exactly.
    assert_eq!(parsed.profiles[0].profile.name.as_str(), "Test Profile");
    assert_eq!(
        parsed.profiles[0].profile.deck_ids,
        common::minimal_profile().profile.deck_ids
    );
}
