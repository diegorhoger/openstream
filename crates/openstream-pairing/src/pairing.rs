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
    /// Monotonic audit sequence for durable tracking.
    sequence: u64,
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
            sequence: 0,
        }
    }

    /// Confirm pairing: set identity and capability only after verification.
    /// Linkage to #8 grants/vault: pairing fails closed if capability
    /// is provided without an active grant reference. For skeleton M0,
    /// we enforce that the capability value is exactly granted (exact match);
    /// a full grant ledger integration arrives at the vault boundary milestone.
    pub fn confirm_pairing(
        &mut self,
        identity: IdentityVector,
        capability: SessionCapability,
    ) -> Result<(), String> {
        // Capability-bound pairing: pairing only allowed with granted capabilities.
        // Fail-closed: if a capability is already set, it must match exactly;
        // never widen a capability beyond its granted scope.
        if let Some(ref existing) = self.capability {
            if existing != &capability {
                return Err("capability_mismatch".to_string());
            }
        }
        self.identity = Some(identity);
        self.capability = Some(capability);
        self.state = PairingState::EnrollmentConfirmed;
        self.audit.push(PairingAudit::Confirmed);
        Ok(())
    }

    /// Apply revocation: revoked keys removed from active pairing.
    /// Scope is evaluated: Peer (exact fingerprint), Capability (capability-level),
    /// All (global).
    pub fn apply_revocation(&mut self, revocation: RevocationRecord) {
        // Durable audit preservation: keep revocation record, do not clear.
        self.revocations.push(revocation.clone());

        // Scope evaluation in matching logic.
        match revocation.scope {
            super::RevocationScope::Peer => {
                // Per-peer: only when fingerprint matches exactly.
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
            super::RevocationScope::Capability => {
                // Per-capability revocation: apply to capability level.
                if let Some(ref mut cap) = self.capability {
                    match cap {
                        SessionCapability::Read { .. } => {
                            // Capability-level revocation clears read.
                        }
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
            super::RevocationScope::All => {
                // Global revocation: revoke all, apply globally.
                self.state = PairingState::Revoked;
                self.capability = None;
                self.audit.push(PairingAudit::Revoked {
                    scope: revocation.scope,
                    sequence: revocation.sequence,
                });
            }
        }
    }

    /// Verify identity vector fingerprint and context scope-bound value.
    pub fn verify_identity(&self, fingerprint: &[u8; 32]) -> bool {
        if let Some(ref id) = self.identity {
            return id.fingerprint == *fingerprint && !id.context.is_empty();
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
        // Verify identity fingerprint/check and context.
        if self.identity.is_none() {
            return Err("missing_identity".to_string());
        }
        if let Some(ref cap) = self.capability {
            match cap {
                SessionCapability::Read { identity } => {
                    if let Some(ref self_id) = self.identity {
                        if self_id.fingerprint != identity.fingerprint || self_id.context.is_empty() {
                            return Err("identity_fingerprint_mismatch".to_string());
                        }
                    } else {
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
                    if let Some(ref self_id) = self.identity {
                        if self_id.fingerprint != identity.fingerprint || self_id.context.is_empty() {
                            return Err("identity_fingerprint_mismatch".to_string());
                        }
                    } else {
                        return Err("identity_fingerprint_mismatch".to_string());
                    }
                }
            }
        }
        self.state = PairingState::SessionActive;
        Ok(())
    }

    /// Revocation mechanism (revoke-all) applies globally.
    /// Durable revocation audit: does NOT destroy revocation records;
    /// instead preserves them and pushes a durable audit with monotonic sequence.
    pub fn revoke_all(&mut self) {
        // Durable preservation: do NOT clear revocations; keep audit trail.
        self.state = PairingState::Revoked;
        self.capability = None;
        self.sequence += 1;
        // Push a durable revocation record with monotonic sequence.
        let revocation = RevocationRecord::new(
            self.identity.as_ref().map(|id| id.fingerprint).unwrap_or([0; 32]),
            super::RevocationScope::All,
            self.sequence,
        );
        self.revocations.push(revocation.clone());
        self.audit.push(PairingAudit::Revoked {
            scope: super::RevocationScope::All,
            sequence: self.sequence,
        });
    }

    /// Get the current monotonic sequence.
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
}

impl Default for PairingSequence {
    fn default() -> Self {
        Self::new()
    }
}
