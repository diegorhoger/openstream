//! Startup composition pipeline: journal-backed crash evidence resume
//! (issue #16, composing #15 persistence and the merged Engine).
//!
//! On every launch the shell runs, in order:
//!
//! 1. **Open** the execution-journal store through the issue #15 pipeline
//!    (`WAL`, `synchronous=FULL`, integrity verification, forward-only
//!    migrations).
//! 2. **Recover honestly when damaged:** the documented remedy ladder
//!    (`openstream_persistence::sqlite::recover`) restores a validated
//!    backup or quarantines damaged files (preserved byte-for-byte) and
//!    recreates fresh. Nothing is silently destroyed or guessed.
//! 3. **Compose the Engine** over that durable store with the real system
//!    clock (`crate::clock::SystemClock`) — the port realization the
//!    engine crate explicitly deferred to this composition root.
//! 4. **Reconcile crash windows:** every prepared-without-terminal record
//!    left by a dead process closes as `outcome_unknown`. Success is never
//!    inferred, non-idempotent effects are never auto-retried.
//! 5. **Surface the truth** in a [`StartupReport`] the tray renders.
//!
//! No dispatch authority exists in this milestone: the composed registry
//! registers no actions and the WebView has no IPC commands, so effective
//! power stays exactly where it was — evidence durability plus honest
//! restart semantics.

use std::path::Path;
use std::sync::{Arc, Mutex};

use openstream_engine::domain_ids::ExecutionId;
use openstream_engine::error::{EngineError, JournalError};
use openstream_engine::journal::{
    AdmissionEntry, DedupeKey, ExecutionJournal, JournalLifecycle, PreparedEntry,
};
use openstream_engine::runtime::{ActionRuntime, RuntimeBuilder};
use openstream_persistence::sqlite::{RecoveryOutcome, SqliteJournal, StorageError, recover};

use crate::clock::SystemClock;

/// Execution-journal store file name inside the data directory.
pub const JOURNAL_FILE_NAME: &str = "journal.sqlite3";

/// What happened to the persistent store at startup, stated exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreOutcome {
    /// No store existed; this launch created one at the current version.
    Fresh,
    /// An existing healthy store opened unchanged.
    OpenedExisting,
    /// The store was damaged and went through the documented recovery
    /// ladder; the exact remedy is carried from the persistence layer.
    Recovered {
        /// Remedy taken (`AlreadyHealthy` / restore / quarantine+recreate).
        outcome: RecoveryOutcome,
    },
}

/// Honest startup summary rendered onto the tray and recorded in evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupReport {
    /// Store-level outcome.
    pub store_outcome: StoreOutcome,
    /// Damaged files moved aside as `.corrupt-<ms>` during recovery
    /// (preserved for forensics, never deleted).
    pub quarantined_files: usize,
    /// Whether a validated pre-damage backup was restored.
    pub backup_restored: bool,
    /// Crash windows closed as `outcome_unknown` by this restart.
    pub reconciled_crash_windows: usize,
    /// Executions currently carrying `outcome_unknown` evidence awaiting
    /// human review (never auto-retried, exempt from pruning until then).
    pub unknown_outcome_executions: usize,
}

/// Typed composition failures. Variants wrap the persistence/engine error
/// types, both of which are redaction-safe by contract.
#[derive(Debug)]
pub enum CompositionError {
    /// The data directory could not be created or accessed.
    DataDirUnavailable,
    /// The journal store refused to open even after the documented
    /// recovery ladder (fail-closed: the shell starts degraded instead).
    Storage(StorageError),
    /// The engine refused its configuration.
    Configuration(EngineError),
    /// A journal write/read refused during reconciliation.
    Journal(JournalError),
}

impl core::fmt::Display for CompositionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::DataDirUnavailable => f.write_str("data-dir-unavailable"),
            // The wrapped types are redaction-safe by contract: their
            // Display emits structural tokens only (no OS text, no paths).
            Self::Storage(error) => write!(f, "journal-store-unusable ({error})"),
            Self::Configuration(error) => {
                write!(f, "engine-configuration-refused ({error})")
            }
            Self::Journal(error) => write!(f, "journal-refused ({error})"),
        }
    }
}

/// Shared handle around the SQLite journal so BOTH the engine runtime and
/// the shell lifecycle (explicit checkpoint, post-reconciliation reads)
/// operate on the same store without duplicating connections.
///
/// Poisoned mutexes are recovered via `into_inner`: SQLite transactions
/// guarantee committed-state integrity regardless of which thread panicked
/// while holding the lock, so evidence access must not be lost.
#[derive(Debug, Clone)]
pub struct SharedJournal {
    handle: Arc<Mutex<SqliteJournal>>,
}

impl SharedJournal {
    /// Wraps an opened store.
    #[must_use]
    pub fn new(journal: SqliteJournal) -> Self {
        Self {
            handle: Arc::new(Mutex::new(journal)),
        }
    }

    /// Explicit WAL checkpoint (TRUNCATE) while the connection is alive.
    ///
    /// # Errors
    /// [`StorageError`] from the store; structural only.
    pub fn checkpoint(&self) -> Result<(), StorageError> {
        Self::with_inner(&self.handle, |journal| journal.checkpoint())
    }

    fn with_inner<R>(
        handle: &Arc<Mutex<SqliteJournal>>,
        operation: impl FnOnce(&mut SqliteJournal) -> R,
    ) -> R {
        let mut guard = handle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        operation(&mut guard)
    }
}

impl ExecutionJournal for SharedJournal {
    fn admit(&mut self, entry: AdmissionEntry) -> Result<(), JournalError> {
        Self::with_inner(&self.handle, |journal| journal.admit(entry))
    }

    fn lookup(&self, key: &DedupeKey) -> Option<AdmissionEntry> {
        Self::with_inner(&self.handle, |journal| journal.lookup(key))
    }

    fn set_lifecycle(
        &mut self,
        execution_id: ExecutionId,
        lifecycle: JournalLifecycle,
    ) -> Result<(), JournalError> {
        Self::with_inner(&self.handle, |journal| {
            journal.set_lifecycle(execution_id, lifecycle)
        })
    }

    fn prepare(&mut self, entry: PreparedEntry) -> Result<(), JournalError> {
        Self::with_inner(&self.handle, |journal| journal.prepare(entry))
    }

    fn resolve_prepared(
        &mut self,
        execution_id: ExecutionId,
        node_key: &openstream_engine::graph::NodeKey,
        attempt: u32,
    ) -> Result<(), JournalError> {
        Self::with_inner(&self.handle, |journal| {
            journal.resolve_prepared(execution_id, node_key, attempt)
        })
    }

    fn unresolved_prepared(&self) -> Vec<PreparedEntry> {
        Self::with_inner(&self.handle, |journal| journal.unresolved_prepared())
    }

    fn prune(&mut self, now_wall_ms: i64, retention_ms: i64) {
        Self::with_inner(&self.handle, |journal| {
            journal.prune(now_wall_ms, retention_ms)
        });
    }

    fn snapshot_admissions(&self) -> Vec<AdmissionEntry> {
        Self::with_inner(&self.handle, |journal| journal.snapshot_admissions())
    }
}

/// Everything the shell owns after a successful composition.
pub struct ShellComposition {
    /// Composed engine runtime (idle this milestone; owned by the shell).
    pub runtime: ActionRuntime,
    /// Shared store handle for explicit shutdown checkpointing.
    pub journal: SharedJournal,
    /// Startup evidence for surfacing.
    pub report: StartupReport,
}

/// Runs the full startup pipeline against `data_dir`.
///
/// # Errors
/// [`CompositionError`] when the data directory, the store, or the engine
/// configuration refuses. The caller starts the shell degraded rather than
/// retrying anything behind the user's back.
pub fn compose_shell_runtime(data_dir: &Path) -> Result<ShellComposition, CompositionError> {
    std::fs::create_dir_all(data_dir).map_err(|_| CompositionError::DataDirUnavailable)?;
    let journal_path = data_dir.join(JOURNAL_FILE_NAME);

    let mut store_outcome = if journal_path.exists() {
        StoreOutcome::OpenedExisting
    } else {
        StoreOutcome::Fresh
    };
    let quarantined_files;
    let backup_restored;

    let journal = match SqliteJournal::open(&journal_path) {
        Ok(journal) => {
            quarantined_files = 0;
            backup_restored = false;
            journal
        }
        // Corruption takes the DOCUMENTED remedy ladder, then reopens once.
        Err(StorageError::Corrupted { .. }) => {
            let recovery_report = recover(&journal_path).map_err(CompositionError::Storage)?;
            let reopened = SqliteJournal::open(&journal_path);
            store_outcome = StoreOutcome::Recovered {
                outcome: recovery_report.outcome(),
            };
            quarantined_files = recovery_report.quarantined().len();
            backup_restored = recovery_report.restored_from().is_some();
            reopened.map_err(CompositionError::Storage)?
        }
        // Unrecognized schemas, future versions, and unavailable files are
        // NOT corruption: they refuse fail-closed with prior state intact.
        Err(other) => return Err(CompositionError::Storage(other)),
    };

    let shared = SharedJournal::new(journal);
    let mut runtime = RuntimeBuilder::new()
        .clock(Arc::new(SystemClock::new()))
        .journal(Box::new(shared.clone()))
        .build()
        .map_err(CompositionError::Configuration)?;

    let reconciled = runtime
        .recover_outcome_unknown()
        .map_err(CompositionError::Journal)?;
    let unknown_outcome_executions = shared
        .snapshot_admissions()
        .iter()
        .filter(|entry| entry.lifecycle == JournalLifecycle::OutcomeUnknown)
        .count();

    Ok(ShellComposition {
        runtime,
        journal: shared,
        report: StartupReport {
            store_outcome,
            quarantined_files,
            backup_restored,
            reconciled_crash_windows: reconciled.len(),
            unknown_outcome_executions,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::{JOURNAL_FILE_NAME, SharedJournal, StoreOutcome, compose_shell_runtime};
    use openstream_engine::graph::NodeKey;
    use openstream_engine::identifiers::{MessageId, SourceDeviceId};
    use openstream_engine::journal::ExecutionJournal as _;
    use openstream_persistence::sqlite::RecoveryOutcome;
    use std::fs;
    use tempfile::TempDir;

    fn seeded_admission(
        journal: &mut SharedJournal,
        message_tag: &str,
    ) -> openstream_engine::domain_ids::ExecutionId {
        use openstream_engine::journal::{
            AdmissionEntry, DedupeKey, JournalLifecycle, PreparedEntry,
        };
        let source = SourceDeviceId::try_new("desktop-shell-test").expect("valid source identity");
        let dedupe = DedupeKey::new(source, MessageId::generate());
        let execution_id = openstream_engine::domain_ids::ExecutionId::generate();
        journal
            .admit(AdmissionEntry {
                key: dedupe,
                execution_id,
                accepted_at_wall_ms: 1_700_000_000_000,
                expires_at_wall_ms: 1_700_000_030_000,
                lifecycle: JournalLifecycle::Accepted,
            })
            .expect("admission persists");
        journal
            .set_lifecycle(execution_id, JournalLifecycle::Running)
            .expect("running transition persists");
        journal
            .prepare(PreparedEntry {
                execution_id,
                node_key: NodeKey::try_new("node-effect").expect("valid node key"),
                attempt: 0,
                action_type: "test.action".to_string(),
                idempotency_key: format!("test-{message_tag}"),
                prepared_at_monotonic_ms: 42,
            })
            .expect("preparation persists BEFORE any effect");
        execution_id
    }

    #[test]
    fn fresh_launch_creates_store_and_second_launch_opens_it() {
        let dir = TempDir::new().expect("temp dir");
        let first = compose_shell_runtime(dir.path()).expect("first composition");
        assert_eq!(first.report.store_outcome, StoreOutcome::Fresh);
        assert_eq!(first.report.unknown_outcome_executions, 0);
        assert_eq!(first.report.reconciled_crash_windows, 0);
        drop(first);

        let second = compose_shell_runtime(dir.path()).expect("second composition");
        assert_eq!(second.report.store_outcome, StoreOutcome::OpenedExisting);
    }

    #[test]
    fn damaged_store_goes_through_the_documented_recovery_ladder() {
        let dir = TempDir::new().expect("temp dir");

        // Create a valid store first, close it cleanly, then destroy its
        // header deterministically (guaranteed Corrupted classification).
        drop(compose_shell_runtime(dir.path()).expect("seed composition"));
        let journal_path = dir.path().join(JOURNAL_FILE_NAME);
        let original = fs::read(&journal_path).expect("store readable");
        let mut damaged = vec![0u8; original.len()];
        let stamp = b"OPENSTREAM-CORRUPTION-FIXTURE";
        let stamped = stamp.len().min(damaged.len());
        damaged[..stamped].copy_from_slice(&stamp[..stamped]);
        fs::write(&journal_path, &damaged).expect("damage written");

        let composed = compose_shell_runtime(dir.path()).expect("composition recovers");
        let report = &composed.report;
        assert!(
            matches!(
                report.store_outcome,
                StoreOutcome::Recovered {
                    outcome: RecoveryOutcome::QuarantinedAndRecreated
                }
            ),
            "unexpected outcome {:?}",
            report.store_outcome
        );
        assert!(report.quarantined_files >= 1, "damaged originals preserved");
        assert!(!report.backup_restored);
        // Quarantined evidence stays on disk for forensics (naming per
        // issue #15: `<original>.corrupt-<ms>` suffix, never deleted).
        let leftovers = fs::read_dir(dir.path())
            .expect("dir listing")
            .filter_map(Result::ok)
            .any(|entry| entry.file_name().to_string_lossy().contains(".corrupt-"));
        assert!(leftovers, "quarantine artifact must remain on disk");
    }

    #[test]
    fn crash_window_reconciles_to_outcome_unknown_and_stays_pending_review() {
        let dir = TempDir::new().expect("temp dir");

        // Session 1: admit, run, prepare an effect... then the "process"
        // dies before terminal evidence (drop without resolve).
        let first = compose_shell_runtime(dir.path()).expect("session 1");
        let orphan_execution = {
            let mut handle = first.journal.clone();
            seeded_admission(&mut handle, "orphan")
        };
        assert_eq!(first.journal.unresolved_prepared().len(), 1);
        drop(first);

        // Session 2 (restart): the crash window closes as outcome_unknown.
        let second = compose_shell_runtime(dir.path()).expect("session 2");
        assert_eq!(second.report.reconciled_crash_windows, 1);
        assert_eq!(second.report.unknown_outcome_executions, 1);

        let admitted = second
            .journal
            .snapshot_admissions()
            .into_iter()
            .find(|entry| entry.execution_id == orphan_execution)
            .expect("evidence survives restart");
        assert_eq!(
            admitted.lifecycle,
            openstream_engine::journal::JournalLifecycle::OutcomeUnknown,
            "success is never inferred across a crash window"
        );
        drop(second);

        // Session 3: the unknown stays pending review; nothing auto-retries
        // and no further reconciliation churns it.
        let third = compose_shell_runtime(dir.path()).expect("session 3");
        assert_eq!(third.report.reconciled_crash_windows, 0);
        assert_eq!(third.report.unknown_outcome_executions, 1);
    }
}
