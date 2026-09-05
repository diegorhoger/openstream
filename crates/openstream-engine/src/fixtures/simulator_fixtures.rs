//! Simulator fixtures for engine contract tests (issue #26, M2 milestone).
//!
//! Fixtures contain only synthetic identifiers and deterministic fake-state
//! vectors. No secrets, credentials, or personal data (SECURITY.md).
//!
//! Classes: F1-F8 per `docs/architecture/OSCP_MESSAGES.md` §11.

use crate::clock::FakeClock;

/// F1: Canonical encoding fixture — fake clock at start.
pub fn fixture_f1_simulator_clock_start() -> FakeClock {
    FakeClock::new(1_700_000_000_000, 0)
}

/// F2: Negotiation matrix — simulated network adapter available/unavailable.
pub fn fixture_f2_simulator_network_negotiation() -> Vec<(bool, &'static str)> {
    vec![(true, "adapter_available"), (false, "adapter_unavailable")]
}

/// F3: Replay/duplication — deterministic duplicate key.
pub fn fixture_f3_simulator_dedupe_key() -> String {
    "sim-device-001:msg-id-v7-duplicate".to_string()
}

/// F4: Expiry boundary — clock before/after expiry.
pub fn fixture_f4_simulator_expiry_boundary() -> (i64, i64) {
    (1_699_999_999_500, 1_700_000_000_000) // before / at expiry
}

/// F5: Crash window — prepared without terminal (simulator state).
pub fn fixture_f5_simulator_crash_gap() -> &'static str {
    "prepared_without_terminal: out = outcome_unknown"
}

/// F6: Recovery chain — snapshot to full snapshot when base revision is 0.
pub fn fixture_f6_simulator_recovery() -> &'static str {
    "recovery: last_known_deck_revision=0 -> full_snapshot_required"
}

/// F7: Error vectors — typed adapter failure code.
pub fn fixture_f7_simulator_error_vectors() -> Vec<&'static str> {
    vec![
        "adapter_unavailable",
        "deadline_exceeded",
        "capability_denied",
        "revision_conflict",
    ]
}

/// F8: Cross-language parity — synthetic identifier set.
pub fn fixture_f8_simulator_cross_language_parity() -> (&'static str, &'static str) {
    ("sim-session-uuidv7-a1b2", "sim-message-uuidv7-c3d4")
}
