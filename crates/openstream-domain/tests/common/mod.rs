//! Shared deterministic builders for integration and property tests.
//!
//! All identifiers are synthesized from a counter into valid UUIDv7-shaped
//! strings; no data here is real.

#![allow(dead_code)]

use openstream_domain::control::{Control, ControlKind, Geometry, InteractionPolicy};
use openstream_domain::deck::Deck;
use openstream_domain::folder::FolderPath;
use openstream_domain::ids::{ControlId, DeckId, PageId, ProfileId, WorkspaceId};
use openstream_domain::page::{GridDimensions, Page};
use openstream_domain::profile::Profile;
use std::str::FromStr as _;

/// Derives a canonical lowercase UUIDv7 string from an arbitrary `u128`.
/// Version nibble is pinned to `7`; variant bits are pinned to RFC 4122.
pub fn uuid7_from(seed: u128) -> String {
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&seed.to_be_bytes());
    bytes[6] = 0x70 | (bytes[6] & 0x0f);
    bytes[8] = 0x80 | (bytes[8] & 0x3f);
    let h = |range: std::ops::Range<usize>| {
        bytes[range]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    format!(
        "{}-{}-{}-{}-{}",
        h(0..4),
        h(4..6),
        h(6..8),
        h(8..10),
        h(10..16)
    )
}

pub fn workspace_id(seed: u128) -> WorkspaceId {
    WorkspaceId::from_str(&uuid7_from(seed)).unwrap()
}

pub fn profile_id(seed: u128) -> ProfileId {
    ProfileId::from_str(&uuid7_from(seed)).unwrap()
}

pub fn deck_id(seed: u128) -> DeckId {
    DeckId::from_str(&uuid7_from(seed)).unwrap()
}

pub fn page_id(seed: u128) -> PageId {
    PageId::from_str(&uuid7_from(seed)).unwrap()
}

pub fn control_id(seed: u128) -> ControlId {
    ControlId::from_str(&uuid7_from(seed)).unwrap()
}

pub fn geometry(x: u16, y: u16, width: u16, height: u16) -> Geometry {
    Geometry {
        x,
        y,
        width,
        height,
    }
}

pub fn control(page: PageId, index: u128, kind: ControlKind) -> Control {
    let policy = match kind {
        ControlKind::VariableDisplay => None,
        _ => Some(InteractionPolicy::Press),
    };
    Control {
        id: control_id(index),
        page_id: page,
        kind,
        geometry: geometry((index % 7) as u16, (index / 7 % 3) as u16, 2, 1),
        label: format!("synthetic control {index}"),
        policy,
        enabled: true,
    }
}

pub fn page(deck: DeckId, ordinal: u32) -> Page {
    let id = page_id(u128::from(ordinal) + 100);
    let mut controls = vec![
        control(id, u128::from(ordinal) * 10, ControlKind::Button),
        control(id, u128::from(ordinal) * 10 + 1, ControlKind::Toggle),
        control(id, u128::from(ordinal) * 10 + 2, ControlKind::PageJump),
        control(
            id,
            u128::from(ordinal) * 10 + 3,
            ControlKind::VariableDisplay,
        ),
    ];
    controls[2].policy = Some(InteractionPolicy::Press);
    Page {
        id,
        deck_id: deck,
        ordinal,
        grid: GridDimensions {
            columns: 16,
            rows: 8,
        },
        controls,
    }
}

pub fn folder(segments: &[&str]) -> FolderPath {
    FolderPath::parse(&segments.join("/")).unwrap()
}

pub fn deck(folder_segments: &[&str]) -> Deck {
    let id = deck_id(1);
    Deck {
        id,
        workspace_id: workspace_id(0),
        title: "synthetic deck".into(),
        revision: 42,
        folder_path: folder(folder_segments),
        pages: vec![page(id, 0), page(id, 1)],
        deleted_at: None,
    }
}

pub fn profile(deck_seeds: &[u128]) -> Profile {
    Profile {
        id: profile_id(9),
        workspace_id: workspace_id(0),
        name: "synthetic profile".into(),
        deck_ids: deck_seeds.iter().map(|seed| deck_id(*seed)).collect(),
        switch_rules: Vec::new(),
    }
}
