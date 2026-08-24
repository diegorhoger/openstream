//! Audit evidence events and the append-only in-memory audit log.
//!
//! Evidence vocabulary per `CAPABILITY_TAXONOMY.md` §3: durable audit events
//! for grant create/narrow/revoke plus execution states `accepted`,
//! `running`, `succeeded`, `failed`, `cancelled`, `expired`,
//! `outcome_unknown`. Events obey redaction rules by construction — they
//! carry capability *kinds* (qualifier-free), typed identifiers, and
//! timestamps only; no labels, configs, paths, URLs, tokens, scene names,
//! or qualifier values ever enter an event.
//!
//! The log is append-only in memory for M1 (no DB engine yet; that is
//! #15): events can be added and read, never mutated or removed, and the
//! capacity bound fails closed instead of silently dropping evidence.

use crate::error::DomainError;
use crate::ids::{ExecutionId, GrantId};
use crate::limits::MAX_AUDIT_EVENTS;
use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Authoritative execution journal states (taxonomy §3 evidence column;
/// THREAT_MODEL §7 invariant 3). Evidence of success originates only from
/// these states. Serializes as the canonical lowercase journal token;
/// unknown tokens reject (fail closed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExecutionState {
    /// Admitted by the Engine; side effect not yet requested.
    Accepted,
    /// Side effect requested; awaiting terminal result.
    Running,
    /// Authoritative success from the Engine/adapter.
    Succeeded,
    /// Typed failure surfaced in the journal (including denials).
    Failed,
    /// Cancelled before the side effect completed.
    Cancelled,
    /// Deadline/expiry reached before admission or completion.
    Expired,
    /// Crash window around a non-idempotent effect; never inferred as
    /// success and never automatically retried (TM-RPL-03).
    OutcomeUnknown,
}

impl ExecutionState {
    /// Canonical lowercase journal token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Expired => "expired",
            Self::OutcomeUnknown => "outcome_unknown",
        }
    }

    fn from_token(token: &str) -> Option<Self> {
        match token {
            "accepted" => Some(Self::Accepted),
            "running" => Some(Self::Running),
            "succeeded" => Some(Self::Succeeded),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            "expired" => Some(Self::Expired),
            "outcome_unknown" => Some(Self::OutcomeUnknown),
            _ => None,
        }
    }
}

impl Serialize for ExecutionState {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ExecutionState {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let token = String::deserialize(deserializer)?;
        ExecutionState::from_token(&token).ok_or_else(|| DeError::custom("unknown execution state"))
    }
}

impl core::fmt::Display for ExecutionState {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One append-only audit event. Every variant is redaction-safe by
/// construction: capability kinds are qualifier-free static strings,
/// subjects are validated structural references, identifiers are UUIDv7
/// values, and timestamps are caller-supplied epoch milliseconds.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AuditEvent {
    /// A consented grant record was created.
    GrantCreated {
        /// Epoch millis supplied by the clock owner.
        at_ms: i64,
        /// The new grant's identifier.
        grant_id: GrantId,
        /// Subject the authority was recorded for.
        subject: crate::grant::SubjectRef,
        /// Qualifier-free capability kind.
        capability_kind: &'static str,
    },
    /// An existing grant was narrowed to a strictly-not-wider scope.
    GrantNarrowed {
        /// Epoch millis supplied by the clock owner.
        at_ms: i64,
        /// The narrowed grant's identifier.
        grant_id: GrantId,
        /// Qualifier-free capability kind.
        capability_kind: &'static str,
    },
    /// A grant record was deleted by revocation; applies at next evaluation
    /// without restart.
    GrantRevoked {
        /// Epoch millis supplied by the clock owner.
        at_ms: i64,
        /// The revoked grant's identifier.
        grant_id: GrantId,
        /// Subject that held the revoked authority.
        subject: crate::grant::SubjectRef,
        /// Qualifier-free capability kind.
        capability_kind: &'static str,
    },
    /// An execution transitioned into one authoritative journal state.
    ExecutionObserved {
        /// Epoch millis supplied by the clock owner.
        at_ms: i64,
        /// The execution this observation belongs to.
        execution_id: ExecutionId,
        /// The observed state.
        state: ExecutionState,
    },
}

impl AuditEvent {
    /// Epoch-millis timestamp carried by the event.
    #[must_use]
    pub const fn at_ms(&self) -> i64 {
        match self {
            Self::GrantCreated { at_ms, .. }
            | Self::GrantNarrowed { at_ms, .. }
            | Self::GrantRevoked { at_ms, .. }
            | Self::ExecutionObserved { at_ms, .. } => *at_ms,
        }
    }
}

/// Append-only in-memory audit log (M1 domain-level records; durable
/// persistence arrives with #15). Appends preserve order; nothing can
/// mutate or remove a stored event through this API.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuditLog {
    entries: Vec<AuditEvent>,
}

impl AuditLog {
    /// Empty log; deny-by-default means zero evidence exists until real
    /// actions append it.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Appends one event, preserving order. Fails closed with
    /// [`DomainError::LimitExceeded`] once [`crate::limits::MAX_AUDIT_EVENTS`]
    /// entries are retained — evidence is never dropped to keep accepting
    /// writes; callers must persist (#15) before continuing.
    pub fn append(&mut self, event: AuditEvent) -> Result<(), DomainError> {
        if self.entries.len() >= MAX_AUDIT_EVENTS {
            return Err(DomainError::LimitExceeded {
                what: "audit events",
                limit: MAX_AUDIT_EVENTS,
            });
        }
        self.entries.push(event);
        Ok(())
    }

    /// Stored events in append order.
    pub fn iter(&self) -> std::slice::Iter<'_, AuditEvent> {
        self.entries.iter()
    }

    /// Number of retained events.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when nothing has been recorded yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Consumes the log, handing over the ordered evidence trail (used when
    /// a persistence layer takes ownership).
    #[must_use]
    pub fn into_entries(self) -> Vec<AuditEvent> {
        self.entries
    }
}

impl<'a> IntoIterator for &'a AuditLog {
    type Item = &'a AuditEvent;
    type IntoIter = std::slice::Iter<'a, AuditEvent>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::{AuditEvent, AuditLog, ExecutionState};
    use crate::grant::SubjectRef;
    use crate::ids::{ExecutionId, GrantId};
    use std::str::FromStr as _;

    fn subject() -> SubjectRef {
        SubjectRef::from_str("peer:018f6a1c-7b21-7cc0-9f31-0e3d5a9d4c11").unwrap()
    }

    #[test]
    fn execution_states_use_journal_vocabulary() {
        let cases = [
            (ExecutionState::Accepted, "accepted"),
            (ExecutionState::Running, "running"),
            (ExecutionState::Succeeded, "succeeded"),
            (ExecutionState::Failed, "failed"),
            (ExecutionState::Cancelled, "cancelled"),
            (ExecutionState::Expired, "expired"),
            (ExecutionState::OutcomeUnknown, "outcome_unknown"),
        ];
        for (state, token) in cases {
            assert_eq!(state.as_str(), token);
            assert_eq!(state.to_string(), token);
            let json = serde_json::to_string(&state).unwrap();
            assert_eq!(json, format!("\"{token}\""));
            let back: ExecutionState = serde_json::from_str(&json).unwrap();
            assert_eq!(back, state);
        }
    }

    #[test]
    fn log_is_append_only_and_ordered() {
        let mut log = AuditLog::new();
        assert!(log.is_empty());
        let execution = ExecutionId::generate();
        log.append(AuditEvent::ExecutionObserved {
            at_ms: 1,
            execution_id: execution,
            state: ExecutionState::Accepted,
        })
        .unwrap();
        log.append(AuditEvent::ExecutionObserved {
            at_ms: 2,
            execution_id: execution,
            state: ExecutionState::Running,
        })
        .unwrap();
        let stamps: Vec<i64> = log.iter().map(AuditEvent::at_ms).collect();
        assert_eq!(stamps, vec![1, 2]);
        assert_eq!(log.len(), 2);
        // Consuming iterator over a reference works.
        assert_eq!((&log).into_iter().count(), 2);
        let entries = log.into_entries();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn overflow_fails_closed_instead_of_dropping_evidence() {
        let mut log = AuditLog::new();
        let grant = GrantId::generate();
        for i in 0..crate::limits::MAX_AUDIT_EVENTS {
            log.append(AuditEvent::GrantCreated {
                at_ms: i64::try_from(i % 1_000_000).unwrap(),
                grant_id: grant,
                subject: subject(),
                capability_kind: "notification.show",
            })
            .unwrap();
        }
        assert_eq!(log.len(), crate::limits::MAX_AUDIT_EVENTS);
        let error = log
            .append(AuditEvent::GrantRevoked {
                at_ms: 0,
                grant_id: grant,
                subject: subject(),
                capability_kind: "notification.show",
            })
            .unwrap_err();
        match error {
            crate::error::DomainError::LimitExceeded { what, limit } => {
                assert_eq!(what, "audit events");
                assert_eq!(limit, crate::limits::MAX_AUDIT_EVENTS);
            }
            other => panic!("unexpected error {other:?}"),
        }
        // The rejected event was not partially stored.
        assert_eq!(log.len(), crate::limits::MAX_AUDIT_EVENTS);
    }

    #[test]
    fn events_serialize_without_qualifier_values_by_construction() {
        // The event carries only the capability KIND; even a hostile-looking
        // qualifier value cannot appear because it is never accepted into an
        // event field.
        let mut log = AuditLog::new();
        log.append(AuditEvent::GrantCreated {
            at_ms: 7,
            grant_id: GrantId::generate(),
            subject: SubjectRef::builtin("obs-broker").unwrap(),
            capability_kind: "network.connect",
        })
        .unwrap();
        let json = serde_json::to_string(&log.into_entries()[0]).unwrap();
        assert!(json.contains("network.connect"));
        assert!(json.contains("builtin:obs-broker"));
    }
}
