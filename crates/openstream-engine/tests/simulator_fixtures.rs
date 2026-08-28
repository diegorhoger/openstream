//! Property/regression tests for simulator fixtures (`issue26-report.md` evidence).
//!
//! Tests cover deterministic fake-state fixtures (F1-F8 patterns) for
//! engine simulators and discovery fixtures. No secrets or production
//! credentials.

use openstream_engine::Clock;
use openstream_engine::fixtures::{
    fixture_f1_simulator_clock_start, fixture_f4_simulator_expiry_boundary,
};

#[test]
fn simulator_f1_clock_start_is_deterministic() {
    let clock = fixture_f1_simulator_clock_start();
    assert_eq!(clock.wall_now_ms(), 1_700_000_000_000);
    assert_eq!(clock.monotonic_ms(), 0);
}

#[test]
fn simulator_f2_network_negotiation_has_both_states() {
    use openstream_engine::fixtures::fixture_f2_simulator_network_negotiation;
    let states = fixture_f2_simulator_network_negotiation();
    assert!(states.contains(&(true, "adapter_available")));
    assert!(states.contains(&(false, "adapter_unavailable")));
}

#[test]
fn simulator_f3_duplicate_key_is_synthetic() {
    use openstream_engine::fixtures::fixture_f3_simulator_dedupe_key;
    let key = fixture_f3_simulator_dedupe_key();
    assert!(key.starts_with("sim-device-001:"));
    assert!(!key.contains("secret"));
    assert!(!key.contains("credential"));
}

#[test]
fn simulator_f4_expiry_boundary_is_exact() {
    let (before, at) = fixture_f4_simulator_expiry_boundary();
    assert!(before < at);
    assert_eq!(before, 1_699_999_999_500);
    assert_eq!(at, 1_700_000_000_000);
}

#[test]
fn simulator_f5_crash_gap_is_redacted() {
    use openstream_engine::fixtures::fixture_f5_simulator_crash_gap;
    let msg = fixture_f5_simulator_crash_gap();
    assert!(msg.contains("outcome_unknown"));
    assert!(!msg.contains("secret"));
}

#[test]
fn simulator_f6_recovery_points_to_full_snapshot() {
    use openstream_engine::fixtures::fixture_f6_simulator_recovery;
    let msg = fixture_f6_simulator_recovery();
    assert!(msg.contains("full_snapshot"));
    assert!(msg.contains("last_known_deck_revision=0"));
}

#[test]
fn simulator_f7_error_vectors_are_typed() {
    use openstream_engine::fixtures::fixture_f7_simulator_error_vectors;
    let codes = fixture_f7_simulator_error_vectors();
    assert!(codes.contains(&"adapter_unavailable"));
    assert!(codes.contains(&"deadline_exceeded"));
}

#[test]
fn simulator_f8_cross_language_parity_is_synthetic() {
    use openstream_engine::fixtures::fixture_f8_simulator_cross_language_parity;
    let (session, msg) = fixture_f8_simulator_cross_language_parity();
    assert!(session.contains("sim-session"));
    assert!(msg.contains("sim-message"));
}
