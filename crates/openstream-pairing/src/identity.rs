//! Identity vectors and fingerprint verification.

/// A verified identity vector for pairing/session authentication.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentityVector {
    /// Fingerprint of the public identity key (32-byte hash).
    pub fingerprint: [u8; 32],
    /// Verified session context (scope-bound).
    pub context: String,
}

/// Key fingerprint computed from a 32-byte identity key.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct KeyFingerprint(pub [u8; 32]);

impl KeyFingerprint {
    /// Verify an identity vector against this fingerprint.
    pub fn verify(&self, identity: &IdentityVector) -> bool {
        self.0 == identity.fingerprint
    }

    /// Create fingerprint from raw bytes.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}
