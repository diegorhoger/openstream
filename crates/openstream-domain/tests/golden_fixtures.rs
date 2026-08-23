//! Golden-fixture conformance: the checked-in JSON must match
//! serialization byte-for-byte, and deserialization must restore the exact
//! document. Fixtures carry only synthetic data (no secrets, no PII).

mod common;

use common::{deck, profile};
use openstream_domain::document::{DeckDocument, ProfileDocument};

fn assert_golden_round_trip(path: &str, json: String) {
    let checked_in = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("fixture {path} must be checked in: {error}"));
    // Byte-exact: deterministic serialization means no reformatting is ever
    // acceptable.
    assert_eq!(
        json, checked_in,
        "serialization drifted from fixture {path}"
    );
}

#[test]
fn deck_document_matches_golden_fixture_exactly() {
    let document = DeckDocument::new(deck(&["streaming", "overlays"]));
    let json = document.to_json_string().unwrap();
    let path = "tests/fixtures/deck-document-v1.json";
    assert_golden_round_trip(path, json);

    let parsed = DeckDocument::from_json_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(parsed, document);
}

#[test]
fn profile_document_matches_golden_fixture_exactly() {
    let document = ProfileDocument::new(profile(&[2, 3]));
    let json = document.to_json_string().unwrap();
    let path = "tests/fixtures/profile-document-v1.json";
    assert_golden_round_trip(path, json);

    let parsed = ProfileDocument::from_json_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(parsed, document);
}

#[test]
fn unknown_schema_versions_reject_at_document_level() {
    for (major, minor) in [(2u32, 0u32), (0, 0), (1, 1), (1, u32::MAX)] {
        let mut value = serde_json::to_value(DeckDocument::new(deck(&[]))).unwrap();
        value["schema_version"] = serde_json::json!({ "major": major, "minor": minor });
        let raw = serde_json::to_string(&value).unwrap();
        let error = DeckDocument::from_json_str(&raw)
            .expect_err("unknown versions fail closed")
            .to_string();
        assert!(
            error.contains("fail closed") || error.to_lowercase().contains("unknown"),
            "unexpected rejection message: {error}"
        );
    }
}

#[test]
fn unknown_fields_reject_instead_of_being_ignored() {
    let mut value = serde_json::to_value(DeckDocument::new(deck(&[]))).unwrap();
    value["surprise"] = serde_json::Value::Bool(true);
    let raw = serde_json::to_string(&value).unwrap();
    assert!(
        DeckDocument::from_json_str(&raw).is_err(),
        "v1.0 marks no forward-compatible members; unknown fields must reject"
    );

    let mut value = serde_json::to_value(ProfileDocument::new(profile(&[2]))).unwrap();
    value["profile"]["title"] = serde_json::Value::String("renamed field".into());
    let raw = serde_json::to_string(&value).unwrap();
    assert!(ProfileDocument::from_json_str(&raw).is_err());
}

#[test]
fn invalid_documents_never_validate_after_decode() {
    // A structurally valid JSON envelope whose entity breaks invariants
    // must still be rejected by the save-time validation stage.
    let mut value = serde_json::to_value(DeckDocument::new(deck(&[]))).unwrap();
    value["deck"]["title"] = serde_json::Value::String(String::new());
    let raw = serde_json::to_string(&value).unwrap();
    assert!(
        DeckDocument::from_json_str(&raw).is_err(),
        "empty title must fail save-time validation"
    );

    let mut value = serde_json::to_value(DeckDocument::new(deck(&[]))).unwrap();
    value["deck"]["pages"][0]["controls"][0]["geometry"]["width"] = serde_json::json!(99);
    let raw = serde_json::to_string(&value).unwrap();
    assert!(
        DeckDocument::from_json_str(&raw).is_err(),
        "geometry outside the grid must fail save-time validation"
    );
}

/// Regeneration tool for the checked-in fixtures. Serialization is
/// deterministic, so a regenerated fixture that differs from the committed
/// one means the contract drifted and the diff must be reviewed. Run with:
///
/// ```text
/// cargo test -p openstream-domain --test golden_fixtures -- --ignored write_golden_fixtures
/// ```
#[test]
#[ignore = "regeneration tool; run explicitly, review any diff"]
fn write_golden_fixtures() {
    std::fs::create_dir_all("tests/fixtures").unwrap();
    std::fs::write(
        "tests/fixtures/deck-document-v1.json",
        DeckDocument::new(deck(&["streaming", "overlays"]))
            .to_json_string()
            .unwrap(),
    )
    .unwrap();
    std::fs::write(
        "tests/fixtures/profile-document-v1.json",
        ProfileDocument::new(profile(&[2, 3]))
            .to_json_string()
            .unwrap(),
    )
    .unwrap();
}
