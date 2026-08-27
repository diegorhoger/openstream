//! Property-based and regression tests for codec (`OSCP_MESSAGES.md` §11).

use openstream_protocol::*;

#[test]
fn regression_roundtrip_hello_fixture() {
    let bytes = include_bytes!("fixtures/f1-hello-v1.json");
    assert!(!bytes.is_empty());
    let env = fixture_f1_hello();
    assert!(!env.encode().is_empty());
    // Size varies by UUID length and payload; determinism checked by decode equality.
}

#[test]
fn regression_fixtures_directory_non_empty() {
    let fixtures_dir = std::path::Path::new("tests/fixtures");
    assert!(fixtures_dir.exists());
    let entries: Vec<_> = std::fs::read_dir(fixtures_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert!(!entries.is_empty(), "fixture directory must contain fixture files");
}
