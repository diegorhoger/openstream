//! Acceptance AC-20-WS-01 through AC-20-WS-04: workspace semantics.
//!
//! Cross-document validation: workspace ownership uniformity, deck-reference
//! closure, switch-rule conflict freedom, and domain validation errors.

mod common;

use openstream_bundle::BundleError;
use openstream_bundle::{build_bundle, parse_bundle};

/// AC-20-WS-01: documents from different workspaces reject.
#[test]
fn workspace_mismatch_rejects() {
    let decks = vec![common::minimal_deck(), common::foreign_workspace_deck()];
    let profiles = vec![common::minimal_profile()];
    assert_eq!(
        build_bundle(&decks, &profiles).unwrap_err(),
        BundleError::WorkspaceMismatch
    );
}

/// AC-20-WS-02: profile referencing a deck outside the bundle rejects.
#[test]
fn missing_deck_reference_rejects() {
    let decks = vec![common::minimal_deck()];
    let profiles = vec![common::dangling_reference_profile()];
    assert_eq!(
        build_bundle(&decks, &profiles).unwrap_err(),
        BundleError::MissingDeckReference
    );
}

/// AC-20-WS-03: conflicting switch rules across profiles reject.
#[test]
fn conflicting_switch_rules_rejects() {
    use openstream_domain::ids::{ProfileId, SwitchRuleId, WorkspaceId};
    use openstream_domain::profile::Profile;
    use openstream_domain::switching::{HotkeyCombo, SwitchRule, SwitchTrigger};
    use std::str::FromStr as _;

    let workspace_id = WorkspaceId::from_str(common::WORKSPACE_UUID).unwrap();

    // Two profiles with identical hotkey combos — must conflict.
    let profile_a_id = ProfileId::from_str(common::PROFILE_UUID).unwrap();
    let profile_a = openstream_domain::document::ProfileDocument::new(Profile {
        id: profile_a_id,
        workspace_id,
        name: "A".into(),
        deck_ids: vec![common::minimal_deck().deck.id],
        switch_rules: vec![SwitchRule {
            id: SwitchRuleId::from_str("018f6a1c-7b21-7010-9f31-000000000010").unwrap(),
            profile_id: profile_a_id,
            workspace_id,
            trigger: SwitchTrigger::Hotkey {
                combo: HotkeyCombo::from_str("ctrl+shift+p").unwrap(),
            },
            enabled: true,
        }],
    });

    let profile_b_id = ProfileId::from_str(common::PROFILE2_UUID).unwrap();
    let profile_b = openstream_domain::document::ProfileDocument::new(Profile {
        id: profile_b_id,
        workspace_id,
        name: "B".into(),
        deck_ids: vec![common::second_deck().deck.id],
        switch_rules: vec![SwitchRule {
            id: SwitchRuleId::from_str("018f6a1c-7b21-7011-9f31-000000000011").unwrap(),
            profile_id: profile_b_id,
            workspace_id,
            trigger: SwitchTrigger::Hotkey {
                combo: HotkeyCombo::from_str("ctrl+shift+p").unwrap(),
            },
            enabled: true,
        }],
    });

    let result = build_bundle(
        &[common::minimal_deck(), common::second_deck()],
        &[profile_a, profile_b],
    );
    assert!(matches!(
        result,
        Err(BundleError::ConflictingSwitchRules("hotkey"))
    ));
}

/// AC-20-WS-04: parse errors surface as `BundleError::Document`.
#[test]
fn invalid_domain_document_rejects() {
    use openstream_bundle::limits::{BUNDLE_FORMAT_VERSION, MAGIC};

    // Construct a bundle with a deck JSON that has an invalid schema_version.
    let invalid_deck_json = serde_json::to_vec(&serde_json::json!({
        "schema_version": { "major": 99, "minor": 0 },
        "deck": {
            "id": common::DECK_UUID,
            "workspace_id": common::WORKSPACE_UUID,
            "title": "x",
            "revision": 1,
            "folder_path": "",
            "pages": [],
            "deleted_at": null
        }
    }))
    .expect("json");

    let profile_json =
        serde_json::to_vec(&common::minimal_profile().profile).expect("profile json");

    let deck_hash = sha256_hex(&invalid_deck_json);
    let profile_hash = sha256_hex(&profile_json);

    let manifest_json = serde_json::to_vec(&serde_json::json!({
        "schema_version": { "major": 1, "minor": 0 },
        "counts": { "decks": 1, "profiles": 1 },
        "entries": [
            { "name": format!("deck/{}.json", common::DECK_UUID), "sha256": deck_hash },
            { "name": format!("profile/{}.json", common::PROFILE_UUID), "sha256": profile_hash },
        ]
    }))
    .expect("manifest json");

    let mut frame = Vec::new();
    frame.extend_from_slice(&MAGIC);
    frame.extend_from_slice(&BUNDLE_FORMAT_VERSION.to_le_bytes());
    frame.extend_from_slice(&3u32.to_le_bytes()); // manifest + deck + profile
    frame.extend_from_slice(&frame_member("manifest.json", &manifest_json));
    frame.extend_from_slice(&frame_member(
        &format!("deck/{}.json", common::DECK_UUID),
        &invalid_deck_json,
    ));
    frame.extend_from_slice(&frame_member(
        &format!("profile/{}.json", common::PROFILE_UUID),
        &profile_json,
    ));

    let result = parse_bundle(&frame);
    assert!(result.is_err(), "invalid domain doc must reject");
    // The error should be a Document variant wrapping the domain error.
    match result {
        Err(BundleError::Document(_)) => {} // expected
        other => panic!("expected BundleError::Document, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Helpers
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

fn frame_member(name: &str, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(name.len() as u32).to_le_bytes());
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.push(0u8); // stored
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    out
}
