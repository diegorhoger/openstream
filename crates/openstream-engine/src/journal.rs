//! Durable evidence port: admission dedupe + prepared/terminal lifecycle.
//!
//! Journal-first admission (`TECHNICAL_SPEC` §5, `PROTOCOL.md`, OSCP
//! `OSCP_MESSAGES.md` §6–§8): the dedupe entry and the `accepted` lifecycle
//! record persist BEFORE any effect can be dispatched, and each side effect
//! writes its durable `prepared` record before the adapter is invoked and
//! its terminal evidence afterward. A crash between those records surfaces
//! as [`JournalLifecycle::OutcomeUnknown`] through
//! [`ExecutionJournal::unresolved_prepared`]; it is never inferred as
//! success and never automatically retried for non-idempotent adapters.
//!
//! Persistence is a trait port this milestone: SQLite arrives with #15.
//! [`MemoryJournal`] is the authoritative in-memory implementation with
//! fail-closed capacity bounds; tests inject fault fakes behind the same
//! trait to prove that refused writes block dispatch.

use crate::domain_ids::ExecutionId;
use crate::graph::NodeKey;
use crate::identifiers::SourceDeviceId;
use crate::limits::{MAX_JOURNAL_ADMISSIONS, MAX_JOURNAL_OPEN_PREPARED};
use crate::{MessageId, error::JournalError};
use openstream_domain::audit::ExecutionState;
use std::collections::HashMap;

/// Half of the admission dedupe key: trusted source identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DedupeKey {
    /// Trusted source device identity (peer / installation / membership).
    pub source_device_id: SourceDeviceId,
    /// Globally unique envelope message id.
    pub message_id: MessageId,
}

impl DedupeKey {
    /// Assembles a key from validated parts.
    #[must_use]
    pub const fn new(source_device_id: SourceDeviceId, message_id: MessageId) -> Self {
        Self {
            source_device_id,
            message_id,
        }
    }
}

/// Lifecycle states journaled per admitted command. Terminal variants map
/// one-to-one onto the authoritative execution states; intermediate
/// variants track the admission pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JournalLifecycle {
    /// Admitted; effects not yet requested.
    Accepted,
    /// First effect attempt has begun.
    Running,
    /// Authoritative terminal success.
    Succeeded,
    /// Typed terminal failure; carries the reason token.
    Failed {
        /// Registry-aligned failure token.
        token: String,
    },
    /// Cancelled before completion.
    Cancelled,
    /// Expired before or during execution.
    Expired,
    /// Crash-window gap around an effect; superseded only by explicit
    /// reconciliation evidence (`OSCP_MESSAGES.md` §8).
    OutcomeUnknown,
}

impl JournalLifecycle {
    /// The matching domain execution state (interop for audit/persistence).
    #[must_use]
    pub fn to_execution_state(&self) -> ExecutionState {
        match self {
            Self::Accepted => ExecutionState::Accepted,
            Self::Running => ExecutionState::Running,
            Self::Succeeded => ExecutionState::Succeeded,
            Self::Failed { .. } => ExecutionState::Failed,
            Self::Cancelled => ExecutionState::Cancelled,
            Self::Expired => ExecutionState::Expired,
            Self::OutcomeUnknown => ExecutionState::OutcomeUnknown,
        }
    }

    /// True once this lifecycle is terminal (no further transitions except
    /// the `OutcomeUnknown` corrective path).
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Succeeded
                | Self::Failed { .. }
                | Self::Cancelled
                | Self::Expired
                | Self::OutcomeUnknown
        )
    }
}

impl core::fmt::Display for JournalLifecycle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.to_execution_state().as_str())
    }
}

/// One durable admission record: dedupe key, assigned execution id, and
/// current lifecycle state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmissionEntry {
    /// Dedupe key `(source_device_id, message_id)`.
    pub key: DedupeKey,
    /// Execution id assigned by the Engine at admission (never caller-minted).
    pub execution_id: ExecutionId,
    /// Wall-clock epoch millis of admission (envelope-expiry context).
    pub accepted_at_wall_ms: i64,
    /// Command expiry horizon in wall-clock millis.
    pub expires_at_wall_ms: i64,
    /// Current lifecycle state.
    pub lifecycle: JournalLifecycle,
}

/// One durable prepared-effect record written BEFORE dispatching the
/// adapter. Resolution pairs it with terminal evidence; an unresolved
/// record after a crash is exactly the `outcome_unknown` window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedEntry {
    /// Owning execution.
    pub execution_id: ExecutionId,
    /// Graph node performing the effect.
    pub node_key: NodeKey,
    /// Attempt counter within the owning retry scope (0 = first run).
    pub attempt: u32,
    /// Registered action type whose port will be (or was) invoked.
    pub action_type: String,
    /// Deterministic adapter-facing idempotency key derived from the dedupe
    /// key, node, and attempt (`OSCP_MESSAGES.md` §7).
    pub idempotency_key: String,
    /// Monotonic milliseconds when preparation was durably recorded.
    pub prepared_at_monotonic_ms: u64,
}

impl PreparedEntry {
    /// Stable identity pairing preparations with resolutions.
    #[must_use]
    pub fn identity(&self) -> (ExecutionId, NodeKey, u32) {
        (self.execution_id, self.node_key.clone(), self.attempt)
    }
}

/// Object-safe durability boundary owned by the runtime. Implementations
/// must be deterministic under test and must never drop evidence: capacity
/// overflow returns [`JournalError::Capacity`] instead.
pub trait ExecutionJournal: core::fmt::Debug + Send {
    /// Persists a new admission record (dedupe insert + initial lifecycle)
    /// before any effect may be requested for the command.
    ///
    /// # Errors
    /// Capacity/refusal failures abort admission fail-closed.
    fn admit(&mut self, entry: AdmissionEntry) -> Result<(), JournalError>;

    /// Looks up the admission record for a dedupe key.
    fn lookup(&self, key: &DedupeKey) -> Option<AdmissionEntry>;

    /// Transitions an admitted command's lifecycle forward (including
    /// terminal states and the corrective `outcome_unknown` path).
    ///
    /// # Errors
    /// [`JournalError::UnknownExecution`] without an admission record;
    /// capacity/refusal failures otherwise.
    fn set_lifecycle(
        &mut self,
        execution_id: ExecutionId,
        lifecycle: JournalLifecycle,
    ) -> Result<(), JournalError>;

    /// Persists a prepared-effect record before dispatching the port.
    ///
    /// # Errors
    /// Capacity/refusal failures leave the effect undispatched (the runtime
    /// fails the execution closed instead).
    fn prepare(&mut self, entry: PreparedEntry) -> Result<(), JournalError>;

    /// Pairs a prepared record with its observed outcome, closing the
    /// crash window for that attempt.
    ///
    /// # Errors
    /// Refusal failures surface on the terminal evidence path.
    fn resolve_prepared(
        &mut self,
        execution_id: ExecutionId,
        node_key: &NodeKey,
        attempt: u32,
    ) -> Result<(), JournalError>;

    /// All open prepared records (crash-gap candidates). Order is stable:
    /// insertion order.
    fn unresolved_prepared(&self) -> Vec<PreparedEntry>;

    /// Prunes admissions older than the retention window, oldest-first.
    /// Entries in [`JournalLifecycle::OutcomeUnknown`] are exempt until
    /// reconciled (`ADR-0005` decision item 3).
    fn prune(&mut self, now_wall_ms: i64, retention_ms: i64);

    /// Ordered snapshot of retained admissions (evidence/inspection).
    fn snapshot_admissions(&self) -> Vec<AdmissionEntry>;
}

/// Authoritative in-memory implementation. Insertion order preserved;
/// lookups indexed; every bound fails closed.
///
/// Persistence arrives with #15; this type is what that layer adopts.
#[derive(Debug, Default)]
pub struct MemoryJournal {
    admissions: Vec<AdmissionEntry>,
    by_key: HashMap<DedupeKey, usize>,
    by_execution: HashMap<ExecutionId, usize>,
    open_prepared: Vec<PreparedEntry>,
}

impl MemoryJournal {
    /// Empty journal; nothing is admissible until records exist.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of retained admission records.
    #[must_use]
    pub fn len(&self) -> usize {
        self.admissions.len()
    }

    /// True when no admission was ever recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.admissions.is_empty()
    }
}

impl ExecutionJournal for MemoryJournal {
    fn admit(&mut self, entry: AdmissionEntry) -> Result<(), JournalError> {
        if self.admissions.len() >= MAX_JOURNAL_ADMISSIONS {
            return Err(JournalError::Capacity {
                what: "journal admissions",
                limit: MAX_JOURNAL_ADMISSIONS,
            });
        }
        let index = self.admissions.len();
        self.by_key.insert(entry.key.clone(), index);
        self.by_execution.insert(entry.execution_id, index);
        self.admissions.push(entry);
        Ok(())
    }

    fn lookup(&self, key: &DedupeKey) -> Option<AdmissionEntry> {
        let index = *self.by_key.get(key)?;
        self.admissions.get(index).cloned()
    }

    fn set_lifecycle(
        &mut self,
        execution_id: ExecutionId,
        lifecycle: JournalLifecycle,
    ) -> Result<(), JournalError> {
        let index = *self
            .by_execution
            .get(&execution_id)
            .ok_or(JournalError::UnknownExecution)?;
        let slot = index;
        if let Some(entry) = self.admissions.get_mut(slot) {
            entry.lifecycle = lifecycle;
        }
        Ok(())
    }

    fn prepare(&mut self, entry: PreparedEntry) -> Result<(), JournalError> {
        if self.open_prepared.len() >= MAX_JOURNAL_OPEN_PREPARED {
            return Err(JournalError::Capacity {
                what: "open prepared records",
                limit: MAX_JOURNAL_OPEN_PREPARED,
            });
        }
        self.open_prepared.push(entry);
        Ok(())
    }

    fn resolve_prepared(
        &mut self,
        execution_id: ExecutionId,
        node_key: &NodeKey,
        attempt: u32,
    ) -> Result<(), JournalError> {
        let position = self.open_prepared.iter().position(|entry| {
            entry.execution_id == execution_id
                && entry.node_key == *node_key
                && entry.attempt == attempt
        });
        if let Some(position) = position {
            self.open_prepared.remove(position);
        }
        // Resolving an unknown preparation stays honest-but-tolerant: the
        // crash-recovery path may resolve entries whose admission row was
        // pruned by retention bounds.
        Ok(())
    }

    fn unresolved_prepared(&self) -> Vec<PreparedEntry> {
        self.open_prepared.clone()
    }

    fn prune(&mut self, now_wall_ms: i64, retention_ms: i64) {
        let cutoff = now_wall_ms.saturating_sub(retention_ms);
        let mut kept: Vec<AdmissionEntry> = Vec::with_capacity(self.admissions.len());
        for entry in self.admissions.drain(..) {
            let exempt = entry.lifecycle == JournalLifecycle::OutcomeUnknown;
            if exempt || entry.accepted_at_wall_ms > cutoff {
                kept.push(entry);
            } else {
                self.by_key.remove(&entry.key.clone());
                self.by_execution.remove(&entry.execution_id);
            }
        }
        // Re-index surviving rows compactly, preserving order.
        self.by_key.clear();
        self.by_execution.clear();
        for (index, entry) in kept.iter().enumerate() {
            self.by_key.insert(entry.key.clone(), index);
            self.by_execution.insert(entry.execution_id, index);
        }
        self.admissions = kept;
    }

    fn snapshot_admissions(&self) -> Vec<AdmissionEntry> {
        self.admissions.clone()
    }
}
