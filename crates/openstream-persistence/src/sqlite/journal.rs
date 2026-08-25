//! Durable [`ExecutionJournal`] over SQLite WAL: the #15 realization of the
//! engine's durability port (`openstream_engine::journal`).
//!
//! Semantics mirror the engine's in-memory `MemoryJournal` exactly where
//! the runtime depends on them, and strengthen two points where durability
//! demands it:
//!
//! - **Atomic autosave.** Every mutating port call is exactly one
//!   committed transaction under `journal_mode=WAL` +
//!   `synchronous=FULL`; returning `Ok` means the evidence survives
//!   immediate process death or power loss without any flush step. A
//!   refused call can never tear: the prior committed state stands.
//! - **Durable dedupe integrity.** The `(source_device_id, message_id)`
//!   dedupe key and Engine-assigned `execution_id` carry UNIQUE
//!   constraints; a duplicate admission insert fails closed instead of
//!   silently shadowing evidence (the runtime's lookup-first contract makes
//!   that path unreachable; the database refuses it anyway).
//!
//! **Durable resolution of a refused terminal write** (recorded at the #14
//! gate): because each write is one atomic transaction, a refused terminal
//! transition leaves the previously persisted state intact and readable —
//! never a torn hybrid. After a crash or refusal, the reopened journal
//! exposes the exact evidence the recovery paths consume:
//! [`ExecutionJournal::unresolved_prepared`] lists preparations whose
//! outcome was never durably recorded, and
//! [`ExecutionJournal::snapshot_admissions`] shows the last committed
//! lifecycle per command. Recovery therefore always relabels from durable
//! facts, never from inference.
//!
//! Capacity bounds default to the engine constants ([`MAX_JOURNAL_ADMISSIONS`],
//! [`MAX_JOURNAL_OPEN_PREPARED`]) and fail closed identically
//! (`JournalError::Capacity`); embedding hosts with tighter resource
//! contracts may pass stricter bounds via [`SqliteJournal::open_bounded`].
//! Bounds are ceilings, never evictions: evidence is never dropped to keep
//! accepting writes.
//!
//! No secret material can enter this storage: the persisted columns are
//! enumerated identifier/timestamp/token fields only (enforced by tests
//! that scan the raw database bytes), and secret values live solely behind
//! the OS credential-vault boundary ([`crate::vault`], SECURITY.md hard
//! rules).

use super::migrations::{self, MIGRATIONS};
use crate::sqlite::error::StorageError;
use openstream_domain::audit::ExecutionState;
use openstream_engine::limits::{MAX_JOURNAL_ADMISSIONS, MAX_JOURNAL_OPEN_PREPARED};
use openstream_engine::{
    AdmissionEntry, DedupeKey, ExecutionId, ExecutionJournal, JournalError, JournalLifecycle,
    MessageId, NodeKey, PreparedEntry, SourceDeviceId,
};
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// Fail-closed capacity ceilings for one journal database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JournalBounds {
    /// Maximum retained admission records; overflow refuses writes.
    pub max_admissions: usize,
    /// Maximum open (prepared-without-resolution) effect records; overflow
    /// refuses preparation, leaving any further effect undispatched
    /// upstream (the runtime fails those executions closed instead).
    pub max_open_prepared: usize,
}

impl Default for JournalBounds {
    fn default() -> Self {
        Self {
            max_admissions: MAX_JOURNAL_ADMISSIONS,
            max_open_prepared: MAX_JOURNAL_OPEN_PREPARED,
        }
    }
}

/// SQLite-backed execution journal. Owns its database connection; usable
/// directly as the runtime's `Box<dyn ExecutionJournal>` durability port.
#[derive(Debug)]
pub struct SqliteJournal {
    connection: rusqlite::Connection,
    path: PathBuf,
    bounds: JournalBounds,
}

impl SqliteJournal {
    /// Opens (or creates) the journal database at `path`, running integrity
    /// verification and forward-only migrations with verified backup before
    /// any upgrade. Use [`crate::sqlite::recover`] when a prior open
    /// reported corruption.
    ///
    /// # Errors
    /// [`StorageError`] for every fail-closed case of the open pipeline.
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        Self::open_bounded(path, JournalBounds::default())
    }

    /// Like [`Self::open`] with explicit capacity ceilings (engine defaults
    /// via [`Self::open`]).
    ///
    /// # Errors
    /// As [`Self::open`].
    pub fn open_bounded(path: &Path, bounds: JournalBounds) -> Result<Self, StorageError> {
        let connection = migrations::open_with(path, MIGRATIONS)?;
        Ok(Self {
            connection,
            path: path.to_path_buf(),
            bounds,
        })
    }

    /// The database file this journal owns.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Checkpoints and truncates the write-ahead log into the main database
    /// file. Embedding hosts call this before host-side file copies of the
    /// store directory.
    ///
    /// # Errors
    /// [`StorageError::Unavailable`] when the checkpoint could not complete
    /// (a contending connection held the WAL); the data stays safe either
    /// way, but the truncation contract is reported honestly.
    pub fn checkpoint(&mut self) -> Result<(), StorageError> {
        let (busy, _log_frames, _checkpointed): (i64, i64, i64) = self
            .connection
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .map_err(|_| StorageError::Unavailable)?;
        if busy != 0 {
            return Err(StorageError::Unavailable);
        }
        Ok(())
    }
}

/// Maps a backend refusal onto the port taxonomy. Raw SQLite message text
/// is dropped (redaction rules); the engine treats every refusal as
/// "durable evidence unavailable" and fails closed before dispatching.
fn refuse(_: rusqlite::Error) -> JournalError {
    JournalError::Refused
}

fn encode_lifecycle(lifecycle: &JournalLifecycle) -> (&'static str, Option<&str>) {
    match lifecycle {
        JournalLifecycle::Accepted => (ExecutionState::Accepted.as_str(), None),
        JournalLifecycle::Running => (ExecutionState::Running.as_str(), None),
        JournalLifecycle::Succeeded => (ExecutionState::Succeeded.as_str(), None),
        JournalLifecycle::Failed { token } => (ExecutionState::Failed.as_str(), Some(token)),
        JournalLifecycle::Cancelled => (ExecutionState::Cancelled.as_str(), None),
        JournalLifecycle::Expired => (ExecutionState::Expired.as_str(), None),
        JournalLifecycle::OutcomeUnknown => (ExecutionState::OutcomeUnknown.as_str(), None),
    }
}

fn decode_lifecycle(token: &str, failure_token: Option<String>) -> Option<JournalLifecycle> {
    match token {
        "accepted" => Some(JournalLifecycle::Accepted),
        "running" => Some(JournalLifecycle::Running),
        "succeeded" => Some(JournalLifecycle::Succeeded),
        // The schema CHECK guarantees a failure token exactly when the
        // lifecycle token is `failed`; `?` enforces the same invariant on
        // the read side.
        "failed" => Some(JournalLifecycle::Failed {
            token: failure_token?,
        }),
        "cancelled" => Some(JournalLifecycle::Cancelled),
        "expired" => Some(JournalLifecycle::Expired),
        "outcome_unknown" => Some(JournalLifecycle::OutcomeUnknown),
        _ => None,
    }
}

fn decode_execution_id(raw: &str) -> ExecutionId {
    // Rows only ever receive Display output of validated identifiers; a
    // decode failure proves out-of-band modification, which must be loud,
    // never silently swallowed into missing evidence.
    ExecutionId::from_str(raw).expect("stored execution id stays canonical")
}

fn count(connection: &rusqlite::Connection, sql: &str) -> Result<usize, JournalError> {
    let rows: i64 = connection
        .query_row(sql, [], |row| row.get(0))
        .map_err(refuse)?;
    usize::try_from(rows).map_err(|_| JournalError::Refused)
}

fn count_where(
    connection: &rusqlite::Connection,
    sql: &str,
    execution_id: ExecutionId,
) -> Result<usize, JournalError> {
    let rows: i64 = connection
        .query_row(
            sql,
            rusqlite::params![execution_id.as_uuid().to_string()],
            |row| row.get(0),
        )
        .map_err(refuse)?;
    usize::try_from(rows).map_err(|_| JournalError::Refused)
}

fn decode_key(source: String, message: String) -> rusqlite::Result<DedupeKey> {
    Ok(DedupeKey {
        source_device_id: SourceDeviceId::from_str(&source)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        message_id: MessageId::from_str(&message).map_err(|_| rusqlite::Error::InvalidQuery)?,
    })
}

fn admission_from_row(
    source_device_id: String,
    message_id: String,
    execution_id: String,
    accepted_at_wall_ms: i64,
    expires_at_wall_ms: i64,
    lifecycle: String,
    failure_token: Option<String>,
) -> rusqlite::Result<AdmissionEntry> {
    let lifecycle =
        decode_lifecycle(&lifecycle, failure_token).ok_or(rusqlite::Error::InvalidQuery)?;
    Ok(AdmissionEntry {
        key: decode_key(source_device_id, message_id)?,
        execution_id: decode_execution_id(&execution_id),
        accepted_at_wall_ms,
        expires_at_wall_ms,
        lifecycle,
    })
}

const ADMISSION_COLUMNS: &str = "source_device_id, message_id, execution_id,
                 accepted_at_wall_ms, expires_at_wall_ms, lifecycle, failure_token";

fn prepared_from_row(
    execution_id: String,
    node_key: String,
    attempt: u32,
    action_type: String,
    idempotency_key: String,
    prepared_at_monotonic_ms: i64,
) -> rusqlite::Result<PreparedEntry> {
    Ok(PreparedEntry {
        execution_id: decode_execution_id(&execution_id),
        node_key: NodeKey::try_new(&node_key).map_err(|_| rusqlite::Error::InvalidQuery)?,
        attempt,
        action_type,
        idempotency_key,
        prepared_at_monotonic_ms: u64::try_from(prepared_at_monotonic_ms)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
    })
}

impl ExecutionJournal for SqliteJournal {
    fn admit(&mut self, entry: AdmissionEntry) -> Result<(), JournalError> {
        let transaction = self.connection.transaction().map_err(refuse)?;
        if count(&transaction, "SELECT count(*) FROM journal_admissions")?
            >= self.bounds.max_admissions
        {
            return Err(JournalError::Capacity {
                what: "journal admissions",
                limit: self.bounds.max_admissions,
            });
        }
        let (lifecycle, failure_token) = encode_lifecycle(&entry.lifecycle);
        transaction
            .execute(
                "INSERT INTO journal_admissions
                 (source_device_id, message_id, execution_id,
                  accepted_at_wall_ms, expires_at_wall_ms, lifecycle, failure_token)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    entry.key.source_device_id.as_str(),
                    entry.key.message_id.as_uuid().to_string(),
                    entry.execution_id.as_uuid().to_string(),
                    entry.accepted_at_wall_ms,
                    entry.expires_at_wall_ms,
                    lifecycle,
                    failure_token,
                ],
            )
            .map_err(refuse)?;
        transaction.commit().map_err(refuse)
    }

    fn lookup(&self, key: &DedupeKey) -> Option<AdmissionEntry> {
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT {ADMISSION_COLUMNS} FROM journal_admissions
                 WHERE source_device_id = ?1 AND message_id = ?2"
            ))
            .ok()?;
        statement
            .query_row(
                rusqlite::params![
                    key.source_device_id.as_str(),
                    key.message_id.as_uuid().to_string(),
                ],
                |row| {
                    admission_from_row(
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    )
                },
            )
            .ok()
    }

    fn set_lifecycle(
        &mut self,
        execution_id: ExecutionId,
        lifecycle: JournalLifecycle,
    ) -> Result<(), JournalError> {
        let transaction = self.connection.transaction().map_err(refuse)?;
        let known = count_where(
            &transaction,
            "SELECT count(*) FROM journal_admissions WHERE execution_id = ?1",
            execution_id,
        )?;
        if known == 0 {
            return Err(JournalError::UnknownExecution);
        }
        let (token, failure_token) = encode_lifecycle(&lifecycle);
        transaction
            .execute(
                "UPDATE journal_admissions SET lifecycle = ?2, failure_token = ?3
                 WHERE execution_id = ?1",
                rusqlite::params![execution_id.as_uuid().to_string(), token, failure_token],
            )
            .map_err(refuse)?;
        transaction.commit().map_err(refuse)
    }

    fn prepare(&mut self, entry: PreparedEntry) -> Result<(), JournalError> {
        let transaction = self.connection.transaction().map_err(refuse)?;
        if count(&transaction, "SELECT count(*) FROM journal_prepared")?
            >= self.bounds.max_open_prepared
        {
            return Err(JournalError::Capacity {
                what: "open prepared records",
                limit: self.bounds.max_open_prepared,
            });
        }
        let prepared_at_monotonic_ms =
            i64::try_from(entry.prepared_at_monotonic_ms).map_err(|_| JournalError::Refused)?;
        transaction
            .execute(
                "INSERT INTO journal_prepared
                 (execution_id, node_key, attempt, action_type, idempotency_key,
                  prepared_at_monotonic_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    entry.execution_id.as_uuid().to_string(),
                    entry.node_key.as_str(),
                    entry.attempt,
                    entry.action_type,
                    entry.idempotency_key,
                    prepared_at_monotonic_ms,
                ],
            )
            .map_err(refuse)?;
        transaction.commit().map_err(refuse)
    }

    fn resolve_prepared(
        &mut self,
        execution_id: ExecutionId,
        node_key: &NodeKey,
        attempt: u32,
    ) -> Result<(), JournalError> {
        let transaction = self.connection.transaction().map_err(refuse)?;
        transaction
            .execute(
                // Exactly one preparation instance closes per resolution,
                // mirroring the in-memory journal's first-position removal:
                // a duplicate preparation (retry cycles) keeps its sibling
                // open until its own outcome is observed.
                "DELETE FROM journal_prepared WHERE seq = (
                     SELECT seq FROM journal_prepared
                     WHERE execution_id = ?1 AND node_key = ?2 AND attempt = ?3
                     ORDER BY seq ASC LIMIT 1)",
                rusqlite::params![
                    execution_id.as_uuid().to_string(),
                    node_key.as_str(),
                    attempt,
                ],
            )
            .map_err(refuse)?;
        // Zero deleted rows stays honest-but-tolerant, mirroring the
        // in-memory journal: crash recovery may close preparations whose
        // admission rows retention already pruned.
        transaction.commit().map_err(refuse)
    }

    fn unresolved_prepared(&self) -> Vec<PreparedEntry> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT execution_id, node_key, attempt, action_type, idempotency_key,
                        prepared_at_monotonic_ms
                 FROM journal_prepared ORDER BY seq ASC",
            )
            .expect("journal schema present");
        let rows = statement
            .query_map([], |row| {
                prepared_from_row(
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                )
            })
            .expect("journal schema present");
        rows.collect::<Result<Vec<_>, _>>()
            .expect("stored prepared rows stay well-formed")
    }

    fn prune(&mut self, now_wall_ms: i64, retention_ms: i64) {
        let cutoff = now_wall_ms.saturating_sub(retention_ms);
        let Ok(transaction) = self.connection.transaction() else {
            return;
        };
        let _ = transaction.execute(
            "DELETE FROM journal_admissions
             WHERE accepted_at_wall_ms <= ?1 AND lifecycle <> 'outcome_unknown'",
            rusqlite::params![cutoff],
        );
        let _ = transaction.commit();
        // Pruning is best-effort maintenance of the retention bound; a
        // refusal keeps evidence in place rather than dropping it.
    }

    fn snapshot_admissions(&self) -> Vec<AdmissionEntry> {
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT {ADMISSION_COLUMNS} FROM journal_admissions ORDER BY seq ASC"
            ))
            .expect("journal schema present");
        let rows = statement
            .query_map([], |row| {
                admission_from_row(
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                )
            })
            .expect("journal schema present");
        rows.collect::<Result<Vec<_>, _>>()
            .expect("stored admission rows stay well-formed")
    }
}
