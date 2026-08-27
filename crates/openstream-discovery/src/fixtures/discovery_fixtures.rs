//! Discovery fixtures for mDNS/surface endpoint contract (`docs/architecture/PROTOCOL.md`, `SECURITY.md`).
//!
//! Fixtures contain only synthetic endpoint identifiers and approved mDNS
//! field patterns. No secrets, credentials, LAN listener activation, or
//! personal data (`SECURITY.md` §3, `PROTOCOL.md` §11).
//!
//! Patterns F1-F8 aligned with `OSCP_MESSAGES.md` §11.

/// F1: Canonical endpoint encoding fixture.
pub fn fixture_f1_discovery_endpoint_encoding() -> &'static str {
    r#"{"service":"openstream-oscp","proto":"_oscp._tcp","ttl":120}"#
}

/// F2: Negotiation matrix — endpoint available / unavailable states.
pub fn fixture_f2_discovery_negotiation_matrix() -> Vec<(&'static str, &'static str)> {
    vec![
        ("service_available", "openstream-oscp"),
        ("service_denied", "permission_denied"),
        ("manual_fallback", "manual_endpoint_fallback"),
    ]
}

/// F3: Replay/duplication — duplicate endpoint record detection.
pub fn fixture_f3_discovery_duplicate_key() -> &'static str {
    "sim-endpoint-dup-v1: openstream-oscp"
}

/// F4: Expiry boundary — service TTL at/before expiry.
pub fn fixture_f4_discovery_expiry_boundary() -> (u32, u32) {
    (120, 0) // ttl / expired
}

/// F5: Crash window — endpoint discovery with unresolved service state.
pub fn fixture_f5_discovery_crash_gap() -> &'static str {
    "service_state_unresolved: outcome_unknown (no real listener)"
}

/// F6: Recovery chain — endpoint snapshot / full snapshot.
pub fn fixture_f6_discovery_recovery() -> &'static str {
    "recovery_hint: last_known_service_revision=0 -> full_snapshot"
}

/// F7: Error vectors — typed discovery denial codes.
pub fn fixture_f7_discovery_error_vectors() -> Vec<&'static str> {
    vec![
        "permission_denied",
        "service_unavailable",
        "manual_fallback_required",
        "network_denied",
    ]
}

/// F8: Cross-language parity — synthetic endpoint identifier set.
pub fn fixture_f8_discovery_cross_language_parity() -> (&'static str, &'static str) {
    ("sim-endpoint-id-uuidv7-f8", "sim-service-type-oscp-v1")
}
