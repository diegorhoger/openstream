//! Property-based tests (proptest): round-trip integrity, serialization
//! determinism, and fail-closed unknown-version rejection across arbitrary
//! valid documents.

mod common;

use common::{deck, profile, uuid7_from};
use openstream_domain::document::{DeckDocument, ProfileDocument};
use openstream_domain::folder::FolderPath;
use openstream_domain::ids::{ControlId, DeckId};
use proptest::prelude::*;
use std::str::FromStr as _;

prop_compose! {
    fn arb_folder()(segments in prop::collection::vec("[a-z][a-z0-9_]{0,15}", 0..4)) -> FolderPath {
        FolderPath::parse(&segments.join("/")).expect("generated folder paths are valid")
    }
}

prop_compose! {
    fn arb_deck()(
        revision in 0u64..1_000,
        folder in arb_folder(),
        // Must start with a non-space so generated titles are never
        // blank-after-trim.
        title in "[a-zA-Z0-9][a-zA-Z0-9 _-]{0,39}",
        deleted in prop::option::of(0i64..4_000_000_000_000),
    ) -> DeckDocument {
        let mut d = deck(&[]);
        d.revision = revision;
        d.folder_path = folder;
        d.title = title;
        d.deleted_at = deleted;
        DeckDocument::new(d)
    }
}

prop_compose! {
    fn arb_profile()(name in "[a-zA-Z0-9][a-zA-Z0-9 _-]{0,39}") -> ProfileDocument {
        let mut p = profile(&[2, 3, 5]);
        p.name = name;
        ProfileDocument::new(p)
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn deck_documents_round_trip_exactly(doc in arb_deck()) {
        let json = doc.to_json_string().unwrap();
        let parsed = DeckDocument::from_json_str(&json).unwrap();
        prop_assert_eq!(&parsed, &doc);
        // Deterministic: re-serialization of the restored document is
        // byte-identical.
        prop_assert_eq!(parsed.to_json_string().unwrap(), json);
    }

    #[test]
    fn profile_documents_round_trip_exactly(doc in arb_profile()) {
        let json = doc.to_json_string().unwrap();
        let parsed = ProfileDocument::from_json_str(&json).unwrap();
        prop_assert_eq!(&parsed, &doc);
        prop_assert_eq!(parsed.to_json_string().unwrap(), json);
    }

    #[test]
    fn serialization_is_deterministic_across_repeated_calls(doc in arb_deck()) {
        let first = doc.to_json_string().unwrap();
        for _ in 0..4 {
            prop_assert_eq!(doc.to_json_string().unwrap(), first.clone());
        }
    }

    #[test]
    fn foreign_majors_reject(major in 2u32..=u32::MAX, minor in any::<u32>()) {
        let mut value = serde_json::to_value(DeckDocument::new(deck(&["x"]))).unwrap();
        value["schema_version"] = serde_json::json!({ "major": major, "minor": minor });
        let raw = serde_json::to_string(&value).unwrap();
        prop_assert!(DeckDocument::from_json_str(&raw).is_err());
    }

    #[test]
    fn minors_newer_than_supported_reject(minor in 1u32..=u32::MAX) {
        let mut value = serde_json::to_value(ProfileDocument::new(profile(&[2]))).unwrap();
        value["schema_version"] = serde_json::json!({ "major": 1, "minor": minor });
        let raw = serde_json::to_string(&value).unwrap();
        prop_assert!(ProfileDocument::from_json_str(&raw).is_err());
    }
}

#[test]
fn synthesized_identifiers_are_canonical_uuidv7() {
    // Exhaustive-ish sweep over structured inputs to the generator.
    for seed in [
        0u128,
        1,
        42,
        u128::from(u64::MAX),
        u128::MAX,
        0x00ff_0000_00aa_0000,
    ] {
        let text = uuid7_from(seed);
        let id = DeckId::from_str(&text).unwrap();
        assert_eq!(id.to_string(), text);
        assert_eq!(id.as_uuid().get_version_num(), 7);
    }
}

#[test]
fn generated_control_ids_do_not_collide_in_practice() {
    let mut seen = std::collections::HashSet::new();
    for _ in 0..10_000 {
        assert!(seen.insert(ControlId::generate()));
    }
}
