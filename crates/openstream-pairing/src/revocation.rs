//! Revocation mechanism: revoked keys removed from active pairing.

/// Scope of a revocation action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RevocationScope {
    /// Per-peer revocation.
    Peer,
    /// Per-capability revocation.
    Capability,
    /// Global revocation (revoke-all).
    All,
}

/// A revocation record: deletes the grant, applies at next evaluation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevocationRecord {
    /// Fingerprint of revoked identity.
    pub fingerprint: [u8; 32],
    /// Scope of revocation.
    pub scope: RevocationScope,
    /// Timestamp or sequence identifier.
    pub sequence: u64,
}

impl RevocationRecord {
    /// Create a new revocation record.
    pub fn new(fingerprint: [u8; 32], scope: RevocationScope, sequence: u64) -> Self {
        Self {
            fingerprint,
            scope,
            sequence,
        }
    }
}
