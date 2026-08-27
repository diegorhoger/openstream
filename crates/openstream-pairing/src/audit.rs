//! Audit records for pairing and revocation events.

/// Pairing audit event (grant create/revoke + execution state tracking).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PairingAudit {
    /// Pairing initiated.
    Initiated,
    /// Pairing confirmed.
    Confirmed,
    /// Pairing revoked (scope specified).
    /// Pairing revoked with revocation scope.
    Revoked {
        /// Scope of revocation action.
        scope: super::RevocationScope,
        /// Sequence identifier.
        sequence: u64,
    },
    /// Pairing session expired.
    Expired,
    /// Pairing failed (typed denial, never silent success).
    /// Pairing failed with typed denial reason.
    Failed {
        /// Typed failure reason (never silent success).
        reason: String,
    },
}

/// Revocation audit record (durable, append-only).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevocationAudit {
    /// The revoked fingerprint.
    pub fingerprint: [u8; 32],
    /// Scope applied.
    pub scope: super::RevocationScope,
    /// Sequence number (monotonic within session).
    pub sequence: u64,
    /// Evidence class (redacted; no raw secrets).
    pub evidence_class: String,
}
