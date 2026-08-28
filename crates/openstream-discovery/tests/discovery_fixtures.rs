//! Discovery fixtures regression (issue #26, M2 milestone evidence).
//!
//! Tests synthetic endpoint fixtures (F1-F8). No secrets, no LAN listener
//! activation, no public discovery claims, no account data.

#[test]
fn discovery_f1_encoding_is_approved_mdns() {
    use openstream_discovery::fixtures::fixture_f1_discovery_endpoint_encoding;
    let encoding = fixture_f1_discovery_endpoint_encoding();
    assert!(encoding.contains("openstream-oscp"));
    assert!(!encoding.contains("secret"));
    assert!(!encoding.contains("credential"));
}

#[test]
fn discovery_f2_negotiation_has_permission_denied() {
    use openstream_discovery::fixtures::fixture_f2_discovery_negotiation_matrix;
    let matrix = fixture_f2_discovery_negotiation_matrix();
    let denied = matrix
        .iter()
        .any(|(_, label)| label.contains("permission_denied") || label.contains("manual_fallback"));
    assert!(denied);
}

#[test]
fn discovery_f3_duplicate_is_synthetic() {
    use openstream_discovery::fixtures::fixture_f3_discovery_duplicate_key;
    let key = fixture_f3_discovery_duplicate_key();
    assert!(key.starts_with("sim-endpoint-dup"));
}

#[test]
fn discovery_f4_expiry_is_exact() {
    use openstream_discovery::fixtures::fixture_f4_discovery_expiry_boundary;
    let (ttl, expired) = fixture_f4_discovery_expiry_boundary();
    assert!(ttl > expired);
    assert_eq!(ttl, 120);
    assert_eq!(expired, 0);
}

#[test]
fn discovery_f5_crash_gap_has_no_secrets() {
    use openstream_discovery::fixtures::fixture_f5_discovery_crash_gap;
    let msg = fixture_f5_discovery_crash_gap();
    assert!(msg.contains("unresolved"));
    assert!(!msg.contains("secret"));
}

#[test]
fn discovery_f6_recovery_has_snapshot_hint() {
    use openstream_discovery::fixtures::fixture_f6_discovery_recovery;
    let msg = fixture_f6_discovery_recovery();
    assert!(msg.contains("snapshot"));
    assert!(msg.contains("revision"));
}

#[test]
fn discovery_f7_errors_are_typed_and_redacted() {
    use openstream_discovery::fixtures::fixture_f7_discovery_error_vectors;
    let errors = fixture_f7_discovery_error_vectors();
    assert!(errors.contains(&"permission_denied"));
    assert!(errors.contains(&"manual_fallback_required"));
}

#[test]
fn discovery_f8_cross_language_parity_is_synthetic() {
    use openstream_discovery::fixtures::fixture_f8_discovery_cross_language_parity;
    let (id, service) = fixture_f8_discovery_cross_language_parity();
    assert!(id.contains("sim-endpoint"));
    assert!(service.contains("oscp"));
}
