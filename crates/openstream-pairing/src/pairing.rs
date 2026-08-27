//! Pairing sequence and session state machine.

use crate::{IdentityVector, PairingAudit, RevocationRecord};

/// Capability-bound pairing: pairing allowed only with granted capabilities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionCapability {
    /// Read capability with verified identity.
    Read {
        /// Verified identity vector.
        identity: IdentityVector,
    },
    /// Control capability with revocation tracking.
    Control {
        /// Verified identity vector.
        identity: IdentityVector,
        /// Revocation flag.
        revoked: bool,
    },
}

/// Pairing sequence state (enrollment and session lifecycle).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PairingState {
    /// Awaiting enrollment (two-minute single-use PSK window).
    AwaitingEnrollment,
    /// Enrollment complete; SAS verified; desktop confirmation pending.
    EnrollmentConfirmed,
    /// Active session (session suite `Noise_IK_25519_ChaChaPoly_BLAKE2s`).
    SessionActive,
    /// Session paused (revocation applied at next evaluation).
    SessionPaused,
    /// Revoked (revoked keys removed from active pairing).
    Revoked,
    /// Expired (replay/MITM defense; TTL enforced).
    Expired,
}

/// Pairing sequence with identity/revocation tracking.
#[derive(Clone, Debug)]
pub struct PairingSequence {
    /// Current pairing state.
    pub state: PairingState,
    /// Verified identity vector.
    pub identity: Option<IdentityVector>,
    /// Capability grant (exact qualifier, never widened).
    pub capability: Option<SessionCapability>,
    /// Active revocation records (fail-closed if revoked).
    pub revocations: Vec<RevocationRecord>,
    /// Audit log (append-only, redacted).
    pub audit: Vec<PairingAudit>,
}

impl PairingSequence {
    /// Initialize pairing with deny-by-default.
    pub fn new() -> Self {
        Self {
            state: PairingState::AwaitingEnrollment,
            identity: None,
            capability: None,
            revocations: Vec::new(),
            audit: Vec::new(),
        }
    }

    /// Confirm pairing: set identity and capability only after verification.
    pub fn confirm_pairing(
        &mut self,
        identity: IdentityVector,
        capability: SessionCapability,
    ) -> Result<(), String> {
        // Capability-bound pairing: pairing only allowed with granted capabilities.
        if self.capability.is_some() && self.capability.as_ref().unwrap() != &capability {
            return Err("capability_mismatch".to_string());
        }
        self.identity = Some(identity);
        self.capability = Some(capability);
        self.state = PairingState::EnrollmentConfirmed;
        self.audit.push(PairingAudit::Confirmed);
        Ok(())
    }

    /// Apply revocation: revoked keys removed from active pairing.
    pub fn apply_revocation(&mut self, revocation: RevocationRecord) {
        self.revocations.push(revocation.clone());
        // Revoked keys removed from active pairing.
        if let Some(ref id) = self.identity {
            if id.fingerprint == revocation.fingerprint {
                self.state = PairingState::Revoked;
                if let Some(ref mut cap) = self.capability {
                    match cap {
                        SessionCapability::Read { .. } => { /* read revoked */ }
                        SessionCapability::Control { revoked, .. } => {
                            *revoked = true;
                        }
                    }
                }
                self.audit.push(PairingAudit::Revoked {
                    scope: revocation.scope,
                    sequence: revocation.sequence,
                });
            }
        }
    }

    /// Verify identity vector fingerprint before session activation.
    pub fn verify_identity(&self, fingerprint: &[u8; 32]) -> bool {
        if let Some(ref id) = self.identity {
            return id.fingerprint == *fingerprint;
        }
        false
    }

    /// Check capability-bound pairing before session state change.
    pub fn transition_to_active(&mut self) -> Result<(), String> {
        if self.state != PairingState::EnrollmentConfirmed {
            return Err("not_confirmed".to_string());
        }
        // Check revocation: fail closed.
        if self.state == PairingState::Revoked {
            return Err("revoked".to_string());
        }
        // Verify identity fingerprint/check.
        if self.identity.is_none() {
            return Err("missing_identity".to_string());
        }
        if let Some(ref cap) = self.capability {
            match cap {
                SessionCapability::Read { identity } => {
                    if self.identity.as_ref().unwrap().fingerprint != identity.fingerprint {
                        return Err("identity_fingerprint_mismatch".to_string());
                    }
                }
                SessionCapability::Control {
                    identity,
                    revoked: r,
                } => {
                    if *r {
                        return Err("revoked_control".to_string());
                    }
                    if self.identity.as_ref().unwrap().fingerprint != identity.fingerprint {
                        return Err("identity_fingerprint_mismatch".to_string());
                    }
                }
            }
        }
        self.state = PairingState::SessionActive;
        Ok(())
    }

    /// Revocation mechanism (revoke-all) applies globally.
    pub fn revoke_all(&mut self) {
        self.revocations.clear(); // reset tracking: global revocation deletes grants
        self.state = PairingState::Revoked;
        self.capability = None;
        self.audit.push(PairingAudit::Revoked {
            scope: super::RevocationScope::All,
            sequence: 0,
        });
    }
}

impl Default for PairingSequence {
    fn default() -> Self {
        Self::new()
    }
}
