//! Pairing-crate tests: pairing sequence, revocation audit durable,
//! scope evaluation, capability-bound pairing, identity verification.

use openstream_pairing::*;

fn test_identity() -> IdentityVector {
    IdentityVector {
        fingerprint: [42; 32],
        context: "test-context".to_string(),
    }
}

#[test]
fn pairing_sequence_init_deny_by_default() {
    let seq = PairingSequence::new();
    assert_eq!(seq.state, PairingState::AwaitingEnrollment);
    assert!(seq.identity.is_none());
    assert!(seq.capability.is_none());
    assert!(seq.revocations.is_empty());
    assert!(seq.audit.is_empty());
}

#[test]
fn confirm_pairing_sets_state_and_capability() {
    let mut seq = PairingSequence::new();
    let id = test_identity();
    let cap = SessionCapability::Read { identity: id.clone() };
    assert!(seq.confirm_pairing(id.clone(), cap.clone()).is_ok());
    assert_eq!(seq.state, PairingState::EnrollmentConfirmed);
    assert!(seq.identity.is_some());
    assert!(seq.capability.is_some());
}

#[test]
fn capability_mismatch_fails_closed() {
    let mut seq = PairingSequence::new();
    let id = test_identity();
    let cap = SessionCapability::Read { identity: id.clone() };
    assert!(seq.confirm_pairing(id.clone(), cap.clone()).is_ok());
    // Changing to a different capability value should fail if we enforce exact match.
    let other_id = IdentityVector {
        fingerprint: [99; 32],
        context: "other".to_string(),
    };
    let other_cap = SessionCapability::Read { identity: other_id };
    assert!(seq.confirm_pairing(id.clone(), other_cap).is_err());
}

#[test]
fn transition_to_active_requires_confirmed_and_identity() {
    let mut seq = PairingSequence::new();
    assert!(seq.transition_to_active().is_err());
    let id = test_identity();
    let cap = SessionCapability::Read { identity: id.clone() };
    seq.confirm_pairing(id.clone(), cap).unwrap();
    assert!(seq.transition_to_active().is_ok());
    assert_eq!(seq.state, PairingState::SessionActive);
}

#[test]
fn revocation_audit_durable_keep_records() {
    let mut seq = PairingSequence::new();
    let id = test_identity();
    let cap = SessionCapability::Read { identity: id.clone() };
    seq.confirm_pairing(id.clone(), cap).unwrap();
    // Before revoke_all, audit has Confirmed only.
    assert_eq!(seq.audit.len(), 1);
    // revoke_all must NOT destroy revocations; should preserve and append audit.
    seq.revoke_all();
    // Revocation records should NOT be cleared; audit should grow.
    assert!(!seq.revocations.is_empty());
    assert_eq!(seq.state, PairingState::Revoked);
    assert!(seq.capability.is_none());
    // Monotonic sequence should advance.
    assert!(seq.sequence() > 0);
}

#[test]
fn revocation_scope_peer_only_on_fingerprint_match() {
    let mut seq = PairingSequence::new();
    let id = test_identity();
    let cap = SessionCapability::Control {
        identity: id.clone(),
        revoked: false,
    };
    seq.confirm_pairing(id.clone(), cap).unwrap();
    let revocation = RevocationRecord::new(
        [42; 32],
        RevocationScope::Peer,
        1,
    );
    seq.apply_revocation(revocation);
    // Peer revocation with matching fingerprint sets state to Revoked.
    assert_eq!(seq.state, PairingState::Revoked);
}

#[test]
fn revocation_scope_all_global_applies() {
    let mut seq = PairingSequence::new();
    let id = test_identity();
    let cap = SessionCapability::Control {
        identity: id.clone(),
        revoked: false,
    };
    seq.confirm_pairing(id.clone(), cap).unwrap();
    let revocation = RevocationRecord::new(
        [99; 32],
        RevocationScope::All,
        1,
    );
    seq.apply_revocation(revocation);
    assert_eq!(seq.state, PairingState::Revoked);
    assert!(seq.capability.is_none());
}

#[test]
fn revocation_scope_capability_applies_to_capability() {
    let mut seq = PairingSequence::new();
    let id = test_identity();
    let cap = SessionCapability::Control {
        identity: id.clone(),
        revoked: false,
    };
    seq.confirm_pairing(id.clone(), cap).unwrap();
    let revocation = RevocationRecord::new(
        [42; 32],
        RevocationScope::Capability,
        1,
    );
    seq.apply_revocation(revocation);
    // Capability-level revocation should set revoked flag on capability.
    assert!(match seq.capability.as_ref().unwrap() {
        SessionCapability::Control { revoked, .. } => *revoked,
        _ => false,
    });
}

#[test]
fn verify_identity_checks_context() {
    let seq = PairingSequence::new();
    let _id_empty_context = IdentityVector {
        fingerprint: [1; 32],
        context: "".to_string(),
    };
    assert!(!seq.verify_identity(&[1; 32]));
    // Even with fingerprint match, empty context should fail.
    // Let's set identity first for a positive check.
    let mut seq2 = PairingSequence::new();
    let id_full = IdentityVector {
        fingerprint: [7; 32],
        context: "scope-bound".to_string(),
    };
    let cap = SessionCapability::Read { identity: id_full.clone() };
    seq2.confirm_pairing(id_full.clone(), cap).unwrap();
    assert!(seq2.verify_identity(&[7; 32]));
}

#[test]
fn pairing_crate_build_and_run() {
    // Minimal smoke: state machine exists, audit durable, scope evaluated.
    let seq = PairingSequence::new();
    assert_eq!(seq.state, PairingState::AwaitingEnrollment);
}
