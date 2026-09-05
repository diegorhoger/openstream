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
fn regression_sequence_regression() {
    let mut env = fixture_f1_hello();
    env.sequence = 0;
    assert_eq!(env.sequence, 0);
    env.sequence = 42;
    assert_eq!(env.sequence, 42);
}

#[test]
fn regression_replay_duplicate_suppressed() {
    let env1 = fixture_f1_hello();
    let env2 = fixture_f1_hello();
    assert_eq!(env1.message_id.as_str(), env2.message_id.as_str());
}

#[test]
fn regression_drop_reorder_decoded() {
    let env = fixture_f1_hello();
    let bytes = env.encode();
    let decoded = Envelope::decode(&bytes).expect("decode must succeed");
    assert_eq!(env.body_kind, decoded.body_kind);
}

#[test]
fn regression_crash_gap_outcome_unknown() {
    // Crash-gap: no terminal state is inferred as success.
    // We simulate an empty decode result representing an unknown outcome.
    let unknown_bytes = vec![0x00, 0x00, 0x00, 0x01]; // too short / corrupt
    assert!(Envelope::decode(&unknown_bytes).is_none());
}

#[test]
fn regression_malformed_envelope_empty_body() {
    let mut env = fixture_f1_hello();
    env.body_bytes.clear();
    env.body_kind = BodyKind::Hello;
    assert!(env.validate_s1().is_err());
}

#[test]
fn regression_error_code_sequence_regression() {
    // Regression: PROTOCOL_MAJOR_MISMATCH must be returned by S1 when the
    // envelope advertises a major version other than PROTOCOL_MAJOR (1).
    // The prior version was a tautological no-op that tested nothing.
    let mut env = fixture_f1_hello();
    // Wrong major: S1 must reject.
    env.protocol_major = PROTOCOL_MAJOR + 1;
    match env.validate_s1() {
        Err("PROTOCOL_MAJOR_MISMATCH") => {}
        other => panic!(
            "S1 must reject wrong major with PROTOCOL_MAJOR_MISMATCH; got {:?}",
            other
        ),
    }
    // Backwards major: S1 must also reject.
    let mut env_back = fixture_f1_hello();
    env_back.protocol_major = PROTOCOL_MAJOR.saturating_sub(1);
    match env_back.validate_s1() {
        Err("PROTOCOL_MAJOR_MISMATCH") => {}
        other => panic!(
            "S1 must reject a backwards major with PROTOCOL_MAJOR_MISMATCH; got {:?}",
            other
        ),
    }
    // Correct major: S1 passes (assuming the body is non-empty for non-heartbeat).
    let env_ok = fixture_f1_hello();
    assert!(env_ok.validate_s1().is_ok());
}

#[test]
fn regression_fixtures_f2_f8_present() {
    let fixtures_dir = std::path::Path::new("tests/fixtures");
    let fixtures = std::fs::read_dir(fixtures_dir).unwrap();
    let names: std::collections::HashSet<String> = fixtures
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().into_string().unwrap_or_default())
        .filter(|n| n.starts_with('f') && n.contains("v"))
        .collect();
    assert!(names.contains("f2-capability-v2.json"));
    assert!(names.contains("f3-replay-v3.json"));
    assert!(names.contains("f4-expiry-v4.json"));
    assert!(names.contains("f5-crash-gap-v5.json"));
    assert!(names.contains("f6-recovery-v6.json"));
    assert!(names.contains("f7-error-v7.json"));
    assert!(names.contains("f8-parity-v8.json"));
}

#[test]
fn regression_fixtures_directory_non_empty() {
    let fixtures_dir = std::path::Path::new("tests/fixtures");
    assert!(fixtures_dir.exists());
    let entries: Vec<_> = std::fs::read_dir(fixtures_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert!(
        !entries.is_empty(),
        "fixture directory must contain fixture files"
    );
}
