//! Telemetry consent gate: explicit opt-in, revocable, separate capability
//! class.
//!
//! Implements issue #21 acceptance criteria: telemetry consent is OFF by
//! default, revocable at any time, and never implicit. The consent state
//! is a pure domain value that can be inspected by any layer without side
//! effects.
//!
//! Per SECURITY.md hard-stop: sensitive fields never enter telemetry APIs
//! and consent is never implicit. The capability taxonomy grants telemetry
//! as a separate class requiring explicit user consent.

use crate::error::DomainError;
use crate::limits::TELEMETRY_CONSENT_DEFAULT;
use serde::{Deserialize, Serialize};

/// The consent state for telemetry data collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TelemetryConsent {
    /// Consent granted; telemetry data may be collected.
    Granted,
    /// Consent denied; no telemetry data may be collected.
    Denied,
}

impl TelemetryConsent {
    /// Whether telemetry is allowed.
    #[must_use]
    pub const fn is_allowed(self) -> bool {
        matches!(self, Self::Granted)
    }

    /// Canonical lowercase token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Granted => "granted",
            Self::Denied => "denied",
        }
    }
}

impl std::fmt::Display for TelemetryConsent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Telemetry consent manager. Tracks consent state with an audit trail
/// of consent changes. The default is OFF (deny-by-default).
#[derive(Debug, Clone)]
pub struct ConsentManager {
    /// Current consent state.
    state: TelemetryConsent,
    /// Number of times consent has been changed.
    change_count: u64,
    /// Timestamp (epoch millis) of the last consent change, if any.
    last_change_at_ms: Option<i64>,
}

impl Default for ConsentManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsentManager {
    /// Creates a new consent manager with the configured default.
    /// Default is OFF (deny-by-default).
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: if TELEMETRY_CONSENT_DEFAULT {
                TelemetryConsent::Granted
            } else {
                TelemetryConsent::Denied
            },
            change_count: 0,
            last_change_at_ms: None,
        }
    }

    /// Grants telemetry consent.
    pub fn grant(&mut self, at_ms: i64) -> Result<(), DomainError> {
        if at_ms < 0 {
            return Err(DomainError::InvalidTimestamp);
        }
        self.state = TelemetryConsent::Granted;
        self.change_count += 1;
        self.last_change_at_ms = Some(at_ms);
        Ok(())
    }

    /// Revokes telemetry consent.
    pub fn revoke(&mut self, at_ms: i64) -> Result<(), DomainError> {
        if at_ms < 0 {
            return Err(DomainError::InvalidTimestamp);
        }
        self.state = TelemetryConsent::Denied;
        self.change_count += 1;
        self.last_change_at_ms = Some(at_ms);
        Ok(())
    }

    /// Current consent state.
    #[must_use]
    pub fn state(&self) -> TelemetryConsent {
        self.state
    }

    /// Whether telemetry is currently allowed.
    #[must_use]
    pub fn is_allowed(&self) -> bool {
        self.state.is_allowed()
    }

    /// Number of consent changes recorded.
    #[must_use]
    pub fn change_count(&self) -> u64 {
        self.change_count
    }

    /// Timestamp of the last consent change.
    #[must_use]
    pub fn last_change_at_ms(&self) -> Option<i64> {
        self.last_change_at_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_consent_is_denied() {
        let manager = ConsentManager::new();
        assert_eq!(manager.state(), TelemetryConsent::Denied);
        assert!(!manager.is_allowed());
    }

    #[test]
    fn grant_and_revoke_cycle() {
        let mut manager = ConsentManager::new();
        manager.grant(1000).unwrap();
        assert!(manager.is_allowed());
        assert_eq!(manager.change_count(), 1);
        assert_eq!(manager.last_change_at_ms(), Some(1000));

        manager.revoke(2000).unwrap();
        assert!(!manager.is_allowed());
        assert_eq!(manager.change_count(), 2);
        assert_eq!(manager.last_change_at_ms(), Some(2000));
    }

    #[test]
    fn revoke_when_already_denied_still_counts() {
        let mut manager = ConsentManager::new();
        manager.revoke(1000).unwrap();
        assert_eq!(manager.change_count(), 1);
        assert!(!manager.is_allowed());
    }

    #[test]
    fn grant_when_already_granted_still_counts() {
        let mut manager = ConsentManager::new();
        manager.grant(1000).unwrap();
        manager.grant(2000).unwrap();
        assert_eq!(manager.change_count(), 2);
        assert!(manager.is_allowed());
    }

    #[test]
    fn negative_timestamp_rejects() {
        let mut manager = ConsentManager::new();
        assert!(manager.grant(-1).is_err());
        assert!(manager.revoke(-1).is_err());
    }

    #[test]
    fn consent_string_roundtrip() {
        assert_eq!(TelemetryConsent::Granted.to_string(), "granted");
        assert_eq!(TelemetryConsent::Denied.to_string(), "denied");
    }

    #[test]
    fn serde_roundtrip() {
        let json = serde_json::to_string(&TelemetryConsent::Granted).unwrap();
        let back: TelemetryConsent = serde_json::from_str(&json).unwrap();
        assert_eq!(back, TelemetryConsent::Granted);
    }
}
