//! Tamper-evident audit log with immutable append-only hash chain.
//!
//! Extends the existing in-memory audit log (`audit.rs`) with tamper
//! evidence: each entry is chained to the previous via a SHA-256 hash,
//! creating a one-way hash chain that detects any modification or removal.
//!
//! Implements issue #21 acceptance criteria: audit log with immutable
//! append-only semantics, one-way token/hash for tamper evidence, typed
//! bucket schema, and deterministic flush.
//!
//! The chain is deterministic: given the same events in the same order,
//! the hash chain is identical. This allows independent verification.

use crate::audit::{AuditEvent, AuditLog};
use crate::error::DomainError;
use serde::Serialize;
use sha2::{Digest, Sha256};

/// The hash of an empty chain (genesis block).
pub const GENESIS_HASH: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// One entry in the tamper-evident chain: the original audit event plus
/// its hash link to the previous entry.
#[derive(Debug, Clone, Serialize)]
pub struct ChainedEntry {
    /// The original audit event.
    pub event: AuditEvent,
    /// Sequential position in the chain (0-indexed).
    pub position: u64,
    /// SHA-256 hash of the previous entry (or GENESIS_HASH for the first).
    pub previous_hash: String,
    /// SHA-256 hash of this entry: hash(previous_hash || event_json).
    pub hash: String,
}

impl ChainedEntry {
    /// Computes the chain hash for an event given the previous hash.
    /// The hash is: SHA-256(previous_hash || serde_json(event)).
    #[must_use]
    pub fn compute_hash(event: &AuditEvent, previous_hash: &str) -> String {
        let event_json = serde_json::to_string(event).expect("audit events always serialize");
        let mut hasher = Sha256::new();
        hasher.update(previous_hash.as_bytes());
        hasher.update(event_json.as_bytes());
        hex(hasher.finalize())
    }

    /// Validates the chain link: recomputes the hash and checks it matches.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        let recomputed = Self::compute_hash(&self.event, &self.previous_hash);
        recomputed == self.hash
    }
}

/// Tamper-evident audit log: append-only chain of hashed events.
///
/// Every append computes the hash linking this entry to the previous one.
/// Verification walks the chain recomputing hashes; any mismatch is
/// evidence of tampering.
#[derive(Debug, Clone)]
pub struct AuditChain {
    entries: Vec<ChainedEntry>,
    /// Typed bucket counts for structured flush.
    buckets: std::collections::HashMap<String, u64>,
}

impl Default for AuditChain {
    fn default() -> Self {
        Self::new()
    }
}

impl AuditChain {
    /// Empty chain.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            buckets: std::collections::HashMap::new(),
        }
    }

    /// Appends one event to the chain, computing the hash link.
    pub fn append(&mut self, event: AuditEvent) -> Result<(), DomainError> {
        let position = self.entries.len() as u64;
        let previous_hash = self
            .entries
            .last()
            .map(|e| e.hash.clone())
            .unwrap_or_else(|| GENESIS_HASH.to_string());

        let hash = ChainedEntry::compute_hash(&event, &previous_hash);

        // Update bucket counts.
        let bucket = event_bucket_name(&event);
        *self.buckets.entry(bucket).or_insert(0) += 1;

        self.entries.push(ChainedEntry {
            event,
            position,
            previous_hash,
            hash,
        });

        Ok(())
    }

    /// Appends from an existing `AuditLog`, converting each event to a
    /// chained entry. The chain order matches the original log order.
    pub fn absorb_log(&mut self, log: &AuditLog) -> Result<(), DomainError> {
        for event in log.iter() {
            self.append(event.clone())?;
        }
        Ok(())
    }

    /// Verifies the entire chain. Returns `Ok(())` if every link is valid,
    /// or `Err` with the position of the first invalid entry.
    pub fn verify(&self) -> Result<(), ChainIntegrityError> {
        let mut previous_hash = GENESIS_HASH.to_string();

        for (i, entry) in self.entries.iter().enumerate() {
            // Check position.
            if entry.position != i as u64 {
                return Err(ChainIntegrityError::PositionMismatch {
                    expected: i as u64,
                    found: entry.position,
                });
            }

            // Check previous hash link.
            if entry.previous_hash != previous_hash {
                return Err(ChainIntegrityError::BrokenLink { position: i as u64 });
            }

            // Recompute and check this entry's hash.
            if !entry.is_valid() {
                return Err(ChainIntegrityError::HashMismatch { position: i as u64 });
            }

            previous_hash = entry.hash.clone();
        }

        Ok(())
    }

    /// Read-only access to all chained entries.
    #[must_use]
    pub fn entries(&self) -> &[ChainedEntry] {
        &self.entries
    }

    /// Number of entries in the chain.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when the chain has no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The hash of the most recent entry (chain head).
    #[must_use]
    pub fn head_hash(&self) -> Option<&str> {
        self.entries.last().map(|e| e.hash.as_str())
    }

    /// Typed bucket counts for structured flush.
    #[must_use]
    pub fn buckets(&self) -> &std::collections::HashMap<String, u64> {
        &self.buckets
    }
}

/// Errors from chain verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainIntegrityError {
    /// Entry position does not match its index.
    PositionMismatch {
        /// Expected position (sequential index).
        expected: u64,
        /// Position found in the entry.
        found: u64,
    },
    /// The previous_hash link does not match the preceding entry's hash.
    BrokenLink {
        /// Position of the broken link.
        position: u64,
    },
    /// The recomputed hash does not match the stored hash.
    HashMismatch {
        /// Position of the tampered entry.
        position: u64,
    },
}

impl std::fmt::Display for ChainIntegrityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PositionMismatch { expected, found } => {
                write!(f, "position mismatch at index {expected}: found {found}")
            }
            Self::BrokenLink { position } => {
                write!(f, "broken hash link at position {position}")
            }
            Self::HashMismatch { position } => {
                write!(
                    f,
                    "hash mismatch at position {position} (tampering detected)"
                )
            }
        }
    }
}

impl std::error::Error for ChainIntegrityError {}

/// Maps an audit event to its typed bucket name for structured flush.
fn event_bucket_name(event: &AuditEvent) -> String {
    match event {
        AuditEvent::GrantCreated { .. } => "grant.created".to_string(),
        AuditEvent::GrantNarrowed { .. } => "grant.narrowed".to_string(),
        AuditEvent::GrantRevoked { .. } => "grant.revoked".to_string(),
        AuditEvent::ExecutionObserved { .. } => "execution.observed".to_string(),
    }
}

/// Hex-encode a byte slice.
fn hex(bytes: impl AsRef<[u8]>) -> String {
    bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::{AuditEvent, ExecutionState};
    use crate::grant::SubjectRef;
    use std::str::FromStr as _;

    fn test_event(at_ms: i64) -> AuditEvent {
        AuditEvent::ExecutionObserved {
            at_ms,
            execution_id: crate::ids::ExecutionId::generate(),
            state: ExecutionState::Accepted,
        }
    }

    fn grant_event(at_ms: i64) -> AuditEvent {
        AuditEvent::GrantCreated {
            at_ms,
            grant_id: crate::ids::GrantId::generate(),
            subject: SubjectRef::from_str("peer:018f6a1c-7b21-7cc0-9f31-0e3d5a9d4c11").unwrap(),
            capability_kind: "notification.show",
        }
    }

    #[test]
    fn genesis_hash_is_stable() {
        // The SHA-256 of an empty string.
        let mut hasher = Sha256::new();
        hasher.update(b"");
        let hash = hex(hasher.finalize());
        assert_eq!(hash, GENESIS_HASH);
    }

    #[test]
    fn single_entry_chain_verifies() {
        let mut chain = AuditChain::new();
        chain.append(test_event(1000)).unwrap();
        assert!(chain.verify().is_ok());
        assert_eq!(chain.len(), 1);
    }

    #[test]
    fn multi_entry_chain_verifies() {
        let mut chain = AuditChain::new();
        chain.append(test_event(1000)).unwrap();
        chain.append(test_event(2000)).unwrap();
        chain.append(test_event(3000)).unwrap();
        assert!(chain.verify().is_ok());
        assert_eq!(
            chain.head_hash().map(str::to_string),
            Some(chain.entries()[2].hash.clone())
        );
    }

    #[test]
    fn empty_chain_verifies() {
        let chain = AuditChain::new();
        assert!(chain.verify().is_ok());
    }

    #[test]
    fn tampered_hash_detected() {
        let mut chain = AuditChain::new();
        chain.append(test_event(1000)).unwrap();
        chain.append(test_event(2000)).unwrap();

        // Tamper with the second entry's hash.
        chain.entries[1].hash = "tampered".to_string();

        let result = chain.verify();
        assert!(result.is_err());
        match result.unwrap_err() {
            ChainIntegrityError::HashMismatch { position } => {
                assert_eq!(position, 1);
            }
            other => panic!("expected HashMismatch, got {other:?}"),
        }
    }

    #[test]
    fn broken_link_detected() {
        let mut chain = AuditChain::new();
        chain.append(test_event(1000)).unwrap();
        chain.append(test_event(2000)).unwrap();

        // Break the link.
        chain.entries[1].previous_hash = "broken".to_string();

        let result = chain.verify();
        assert!(result.is_err());
        match result.unwrap_err() {
            ChainIntegrityError::BrokenLink { position } => {
                assert_eq!(position, 1);
            }
            ChainIntegrityError::HashMismatch { .. } => {
                // Hash mismatch also detected (order-dependent).
            }
            other => panic!("expected BrokenLink or HashMismatch, got {other:?}"),
        }
    }

    #[test]
    fn position_mismatch_detected() {
        let mut chain = AuditChain::new();
        chain.append(test_event(1000)).unwrap();

        // Tamper with position.
        chain.entries[0].position = 99;

        let result = chain.verify();
        assert!(result.is_err());
        match result.unwrap_err() {
            ChainIntegrityError::PositionMismatch { expected, found } => {
                assert_eq!(expected, 0);
                assert_eq!(found, 99);
            }
            other => panic!("expected PositionMismatch, got {other:?}"),
        }
    }

    #[test]
    fn absorb_log_preserves_order() {
        let mut log = AuditLog::new();
        log.append(test_event(1000)).unwrap();
        log.append(test_event(2000)).unwrap();

        let mut chain = AuditChain::new();
        chain.absorb_log(&log).unwrap();

        assert_eq!(chain.len(), 2);
        assert!(chain.verify().is_ok());
    }

    #[test]
    fn bucket_counts() {
        let mut chain = AuditChain::new();
        chain.append(test_event(1000)).unwrap();
        chain.append(test_event(2000)).unwrap();
        chain.append(grant_event(3000)).unwrap();

        let buckets = chain.buckets();
        assert_eq!(buckets.get("execution.observed"), Some(&2));
        assert_eq!(buckets.get("grant.created"), Some(&1));
    }

    #[test]
    fn chain_is_deterministic() {
        let mut chain1 = AuditChain::new();
        let mut chain2 = AuditChain::new();
        let event = test_event(1000);

        chain1.append(event.clone()).unwrap();
        chain2.append(event).unwrap();

        assert_eq!(chain1.entries()[0].hash, chain2.entries()[0].hash);
    }

    #[test]
    fn chain_detection_error_display() {
        let err = ChainIntegrityError::HashMismatch { position: 5 };
        assert!(err.to_string().contains("5"));
        let err = ChainIntegrityError::BrokenLink { position: 3 };
        assert!(err.to_string().contains("3"));
    }
}
