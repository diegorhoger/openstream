//! Shared fixtures for the bundle integration tests (issue #20).
//!
//! Provides deterministic domain document builders for decks and profiles
//! that exercise all acceptance criteria without requiring real user data.

#![allow(missing_docs)]
#![allow(dead_code)]

use openstream_domain::control::{Control, ControlKind, Geometry, InteractionPolicy};
use openstream_domain::deck::Deck;
use openstream_domain::document::{DeckDocument, ProfileDocument};
use openstream_domain::folder::FolderPath;
use openstream_domain::ids::{ControlId, DeckId, PageId, ProfileId, SwitchRuleId, WorkspaceId};
use openstream_domain::page::{GridDimensions, Page};
use openstream_domain::profile::Profile;
use openstream_domain::switching::{HotkeyCombo, SwitchRule, SwitchTrigger};
use std::str::FromStr as _;

/// Canonical workspace UUIDv7 for all fixture documents.
pub const WORKSPACE_UUID: &str = "018f6a1c-7b21-7000-9f31-000000000000";
/// Canonical deck UUIDv7.
pub const DECK_UUID: &str = "018f6a1c-7b21-7002-9f31-000000000002";
/// Canonical profile UUIDv7.
pub const PROFILE_UUID: &str = "018f6a1c-7b21-7001-9f31-000000000001";
/// Second deck UUIDv7 for multi-deck tests.
pub const DECK2_UUID: &str = "018f6a1c-7b21-7003-9f31-000000000003";
/// Second profile UUIDv7 for multi-profile tests.
pub const PROFILE2_UUID: &str = "018f6a1c-7b21-7004-9f31-000000000004";
/// Profile UUIDv7 for the hotkey-profile fixture.
pub const PROFILE_HOTKEY_UUID: &str = "018f6a1c-7b21-7007-9f31-000000000007";

/// Builds a minimal but valid DeckDocument with one page and one button control.
pub fn minimal_deck() -> DeckDocument {
    let deck_id = DeckId::from_str(DECK_UUID).unwrap();
    let page_id = PageId::from_str("018f6a1c-7b21-7005-9f31-000000000005").unwrap();
    DeckDocument::new(Deck {
        id: deck_id,
        workspace_id: WorkspaceId::from_str(WORKSPACE_UUID).unwrap(),
        title: "Test Deck".into(),
        revision: 1,
        folder_path: FolderPath::parse("test").unwrap(),
        pages: vec![Page {
            id: page_id,
            deck_id,
            ordinal: 0,
            grid: GridDimensions {
                columns: 4,
                rows: 2,
            },
            controls: vec![Control {
                id: ControlId::from_str("018f6a1c-7b21-7006-9f31-000000000006").unwrap(),
                page_id,
                kind: ControlKind::Button,
                geometry: Geometry {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                },
                label: "Button".into(),
                policy: Some(InteractionPolicy::Press),
                enabled: true,
            }],
        }],
        deleted_at: None,
    })
}

/// Builds a second valid DeckDocument for multi-deck tests.
pub fn second_deck() -> DeckDocument {
    let deck_id = DeckId::from_str(DECK2_UUID).unwrap();
    DeckDocument::new(Deck {
        id: deck_id,
        workspace_id: WorkspaceId::from_str(WORKSPACE_UUID).unwrap(),
        title: "Second Deck".into(),
        revision: 1,
        folder_path: FolderPath::parse("second").unwrap(),
        pages: vec![],
        deleted_at: None,
    })
}

/// Builds a valid ProfileDocument referencing the primary deck.
pub fn minimal_profile() -> ProfileDocument {
    ProfileDocument::new(Profile {
        id: ProfileId::from_str(PROFILE_UUID).unwrap(),
        workspace_id: WorkspaceId::from_str(WORKSPACE_UUID).unwrap(),
        name: "Test Profile".into(),
        deck_ids: vec![DeckId::from_str(DECK_UUID).unwrap()],
        switch_rules: Vec::new(),
    })
}

/// Builds a second valid ProfileDocument referencing the second deck.
pub fn second_profile() -> ProfileDocument {
    ProfileDocument::new(Profile {
        id: ProfileId::from_str(PROFILE2_UUID).unwrap(),
        workspace_id: WorkspaceId::from_str(WORKSPACE_UUID).unwrap(),
        name: "Second Profile".into(),
        deck_ids: vec![DeckId::from_str(DECK2_UUID).unwrap()],
        switch_rules: Vec::new(),
    })
}

/// Builds a valid ProfileDocument with a hotkey switch rule.
pub fn profile_with_hotkey() -> ProfileDocument {
    let profile_id = ProfileId::from_str(PROFILE_HOTKEY_UUID).unwrap();
    ProfileDocument::new(Profile {
        id: profile_id,
        workspace_id: WorkspaceId::from_str(WORKSPACE_UUID).unwrap(),
        name: "Hotkey Profile".into(),
        deck_ids: vec![DeckId::from_str(DECK_UUID).unwrap()],
        switch_rules: vec![SwitchRule {
            id: SwitchRuleId::from_str("018f6a1c-7b21-7010-9f31-000000000010").unwrap(),
            profile_id,
            workspace_id: WorkspaceId::from_str(WORKSPACE_UUID).unwrap(),
            trigger: SwitchTrigger::Hotkey {
                combo: HotkeyCombo::from_str("ctrl+shift+p").unwrap(),
            },
            enabled: true,
        }],
    })
}

/// Builds a ProfileDocument whose deck_ids reference a deck NOT in the bundle
/// (for workspace-mismatch tests).
pub fn dangling_reference_profile() -> ProfileDocument {
    ProfileDocument::new(Profile {
        id: ProfileId::from_str(PROFILE_UUID).unwrap(),
        workspace_id: WorkspaceId::from_str(WORKSPACE_UUID).unwrap(),
        name: "Dangling".into(),
        deck_ids: vec![DeckId::from_str("018f6a1c-7b21-7099-9f31-000000000099").unwrap()],
        switch_rules: Vec::new(),
    })
}

/// Builds a DeckDocument with a different workspace_id (for cross-workspace tests).
pub fn foreign_workspace_deck() -> DeckDocument {
    DeckDocument::new(Deck {
        id: DeckId::from_str(DECK_UUID).unwrap(),
        workspace_id: WorkspaceId::from_str("018f6a1c-7b21-7098-9f31-000000000098").unwrap(),
        title: "Foreign".into(),
        revision: 1,
        folder_path: FolderPath::root(),
        pages: vec![],
        deleted_at: None,
    })
}
