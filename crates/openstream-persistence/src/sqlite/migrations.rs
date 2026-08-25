//! Open pipeline for OpenStream SQLite databases: WAL configuration,
//! integrity verification, forward-only migrations, verified
//! backup-before-migrate, and the corruption recovery path.
//!
//! Every database passes through the same sequence when opened
//! ([`open_with`]):
//!
//! 1. **Configure** â€” `journal_mode=WAL`, `synchronous=FULL` (a returned
//!    commit is durable across process death and power loss), bounded busy
//!    timeout, `trusted_schema=OFF`.
//! 2. **Verify** â€” full `PRAGMA integrity_check` before any content is
//!    trusted. A damaged database fails closed with
//!    [`StorageError::Corrupted`] instead of being touched; callers invoke
//!    [`recover`] for the documented remedy.
//! 3. **Classify** â€” no tables means a fresh install; the OpenStream anchor
//!    table (`openstream_schema`) carries the schema version; anything else
//!    is rejected as [`StorageError::UnrecognizedSchema`].
//! 4. **Migrate forward only** â€” one transaction per step, strictly
//!    `v -> v+1` along [`MIGRATIONS`]. Upgrading an existing database first
//!    takes a **verified backup** through the SQLite online-backup API
//!    (`<db>.backup-pre-v<target>`); if that backup cannot be produced or
//!    verified the upgrade aborts before any DDL runs. A newer-than-build
//!    database refuses to downgrade unconditionally.
//!
//! [`recover`] is the corruption remedy: prefer the newest restorable
//! pre-migration backup (validated for integrity and version before use);
//! otherwise quarantine every database file under a `.corrupt-<ms>` suffix
//! (originals are preserved for forensics, never destroyed) and recreate a
//! fresh schema-current database. The report states honestly which path
//! was taken.

use super::error::{CorruptionStage, SchemaStage, StorageError};
use core::fmt;
use std::cmp::Reverse;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Highest schema version this build implements. v1 shipped the
/// execution-journal evidence tables; v2 added the authored workspace
/// documents table (issue #17). Future versions extend [`MIGRATIONS`] with
/// exactly one forward step each.
pub const SCHEMA_VERSION: u32 = 2;

/// One forward-only migration step. Steps form a contiguous chain
/// `0 -> 1 -> ... -> SCHEMA_VERSION`; each executes once inside a single
/// transaction together with its version bump, so a crash mid-step leaves
/// the previous version fully intact.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Migration {
    /// Version this step applies to.
    pub from: u32,
    /// Version produced by this step.
    pub to: u32,
    /// SQL executed verbatim inside the step transaction.
    pub sql: &'static str,
}

impl fmt::Display for Migration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{} -> v{}", self.from, self.to)
    }
}

/// The released migration chain. v1 ships the execution-journal evidence
/// tables (`migrations/sqlite/0001_initial.sql`); v2 ships the authored
/// workspace documents table (`migrations/sqlite/0002_workspace_documents.sql`,
/// issue #17). New releases append exactly one step per schema bump; nothing
/// here is ever edited or reordered after release.
pub(crate) const MIGRATIONS: &[Migration] = &[
    Migration {
        from: 0,
        to: 1,
        sql: include_str!("../../../../migrations/sqlite/0001_initial.sql"),
    },
    Migration {
        from: 1,
        to: 2,
        sql: include_str!("../../../../migrations/sqlite/0002_workspace_documents.sql"),
    },
];

/// Busy timeout granted to contending writers. Single-process ownership is
/// the product model; this only smooths transient contention (backup
/// probes, concurrent recovery), it never papers over deadlock.
const BUSY_TIMEOUT_MS: u64 = 2_000;

/// Outcome of a [`recover`] invocation, stated honestly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryOutcome {
    /// The database passed verification untouched.
    AlreadyHealthy,
    /// A restorable pre-migration backup was validated and restored; the
    /// damaged originals were quarantined first.
    RestoredFromBackup,
    /// No restorable backup existed; the damaged files were quarantined and
    /// a fresh schema-current database was created.
    QuarantinedAndRecreated,
}

/// What [`recover`] did, including the exact on-disk artifacts.
#[derive(Debug, Clone)]
pub struct RecoveryReport {
    outcome: RecoveryOutcome,
    quarantined: Vec<PathBuf>,
    restored_from: Option<PathBuf>,
}

impl RecoveryReport {
    /// The remedy path taken.
    #[must_use]
    pub const fn outcome(&self) -> RecoveryOutcome {
        self.outcome
    }

    /// Files moved aside as `.corrupt-<ms>` (preserved, never deleted).
    #[must_use]
    pub fn quarantined(&self) -> &[PathBuf] {
        &self.quarantined
    }

    /// Backup file restored, when the restore path was taken.
    #[must_use]
    pub const fn restored_from(&self) -> Option<&PathBuf> {
        self.restored_from.as_ref()
    }
}

/// Layout classification of an opened connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layout {
    /// No user tables exist: a fresh install.
    Fresh,
    /// An OpenStream database anchored at the given schema version.
    Versioned(u32),
}

/// Opens (creating when absent) and prepares a database at `path` through
/// the supplied chain. The chain's final step defines the target version,
/// so the same pipeline serves both production opens ([`MIGRATIONS`]) and
/// synthetic chains under test.
///
/// # Errors
/// [`StorageError`] for every fail-closed case; see module docs for the
/// pipeline order.
pub(crate) fn open_with(
    path: &Path,
    chain: &[Migration],
) -> Result<rusqlite::Connection, StorageError> {
    let target = chain.last().map(|step| step.to).unwrap_or(0);
    let mut connection = connect(path)?;
    prepare(&mut connection, path, target, chain)?;
    Ok(connection)
}

fn connect(path: &Path) -> Result<rusqlite::Connection, StorageError> {
    // NOFOLLOW refuses symlinked database paths: the store is owned at a
    // fixed location, not wherever a local attacker re-points it.
    let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
        | rusqlite::OpenFlags::SQLITE_OPEN_CREATE
        | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW;
    let connection = rusqlite::Connection::open_with_flags(path, flags)
        .map_err(|_| StorageError::Unavailable)?;
    configure(&connection)?;
    Ok(connection)
}

fn configure(connection: &rusqlite::Connection) -> Result<(), StorageError> {
    let mode: String = connection
        .query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))
        .map_err(|_| StorageError::Corrupted {
            stage: CorruptionStage::Header,
        })?;
    if !mode.eq_ignore_ascii_case("wal") {
        // WAL is the durability contract; refusing non-WAL backends keeps
        // atomicity/durability claims honest everywhere.
        return Err(StorageError::Unavailable);
    }
    connection
        .pragma_update(None, "synchronous", "FULL")
        .map_err(|_| StorageError::Unavailable)?;
    connection
        .pragma_update(None, "trusted_schema", "OFF")
        .map_err(|_| StorageError::Unavailable)?;
    connection
        .busy_timeout(Duration::from_millis(BUSY_TIMEOUT_MS))
        .map_err(|_| StorageError::Unavailable)?;
    Ok(())
}

fn prepare(
    connection: &mut rusqlite::Connection,
    path: &Path,
    target: u32,
    chain: &[Migration],
) -> Result<(), StorageError> {
    verify_integrity(connection)?;
    let layout = classify(connection)?;
    // A fresh install has nothing to lose: multi-step creation must not
    // produce pre-migration backup artifacts for databases that did not
    // exist before this open. Only genuine upgrades of existing stores
    // take the verified-backup detour.
    let existed = matches!(layout, Layout::Versioned(_));
    if let Layout::Versioned(version) = layout
        && version > target
    {
        return Err(StorageError::SchemaTooNew {
            found: version,
            supported: target,
        });
    }
    upgrade(connection, path, target, chain, existed)
}

fn verify_integrity(connection: &rusqlite::Connection) -> Result<(), StorageError> {
    let verdict: String = connection
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(|_| StorageError::Corrupted {
            stage: CorruptionStage::Probe,
        })?;
    if verdict.eq_ignore_ascii_case("ok") {
        Ok(())
    } else {
        // The verdict text can embed fragments of stored bytes; it is
        // dropped, only the structural fact is reported.
        Err(StorageError::Corrupted {
            stage: CorruptionStage::Content,
        })
    }
}

fn classify(connection: &rusqlite::Connection) -> Result<Layout, StorageError> {
    let table_count: i64 = connection
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table'
             AND name NOT LIKE 'sqlite_%'",
            [],
            |row| row.get(0),
        )
        .map_err(|_| StorageError::Corrupted {
            stage: CorruptionStage::Probe,
        })?;
    if table_count == 0 {
        return Ok(Layout::Fresh);
    }
    let anchored: i64 = connection
        .query_row(
            "SELECT count(*) FROM sqlite_master
             WHERE type = 'table' AND name = 'openstream_schema'",
            [],
            |row| row.get(0),
        )
        .map_err(|_| StorageError::Corrupted {
            stage: CorruptionStage::Probe,
        })?;
    if anchored == 0 {
        return Err(StorageError::UnrecognizedSchema {
            stage: SchemaStage::Foreign,
        });
    }
    read_version(connection).map(Layout::Versioned)
}

fn read_version(connection: &rusqlite::Connection) -> Result<u32, StorageError> {
    // Exactly one anchor row must exist and carry a non-negative version;
    // anything else is an out-of-band or damaged anchor, never guessed at.
    let row_count: i64 = connection
        .query_row("SELECT count(*) FROM openstream_schema", [], |row| {
            row.get(0)
        })
        .map_err(|_| StorageError::UnrecognizedSchema {
            stage: SchemaStage::Anchor,
        })?;
    if row_count != 1 {
        return Err(StorageError::UnrecognizedSchema {
            stage: SchemaStage::Anchor,
        });
    }
    let value: i64 = connection
        .query_row(
            "SELECT value FROM openstream_schema WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .map_err(|_| StorageError::UnrecognizedSchema {
            stage: SchemaStage::Anchor,
        })?;
    u32::try_from(value).map_err(|_| StorageError::UnrecognizedSchema {
        stage: SchemaStage::Anchor,
    })
}

fn upgrade(
    connection: &mut rusqlite::Connection,
    path: &Path,
    target: u32,
    chain: &[Migration],
    existed: bool,
) -> Result<(), StorageError> {
    let mut version = match classify(connection)? {
        Layout::Fresh => 0,
        Layout::Versioned(found) => found,
    };
    while version < target {
        let step = chain
            .iter()
            .find(|step| step.from == version && step.to == version + 1)
            .ok_or(StorageError::MigrationMissing { from: version })?;
        // Existing data is backed up and the backup verified BEFORE any DDL
        // runs. Fresh installs (nothing yet to lose) skip this.
        if existed && version > 0 {
            take_verified_backup(connection, path, step.to, version)?;
        }
        let transaction = connection
            .transaction()
            .map_err(|_| StorageError::MigrationFailed {
                from: step.from,
                to: step.to,
            })?;
        transaction
            .execute_batch(step.sql)
            .map_err(|_| StorageError::MigrationFailed {
                from: step.from,
                to: step.to,
            })?;
        transaction
            .commit()
            .map_err(|_| StorageError::MigrationFailed {
                from: step.from,
                to: step.to,
            })?;
        version = step.to;
    }
    Ok(())
}

/// Produces `<db>.backup-pre-v<target>` via the SQLite online-backup API
/// (WAL-aware), then verifies the copy opens independently, passes
/// integrity, and anchors exactly at the source version. Any failure
/// aborts the pending upgrade fail-closed.
fn take_verified_backup(
    connection: &rusqlite::Connection,
    path: &Path,
    target_version: u32,
    source_version: u32,
) -> Result<PathBuf, StorageError> {
    let unavailable = || StorageError::BackupUnavailable { target_version };
    let backup_path = backup_path_for(path, target_version);
    let mut destination = rusqlite::Connection::open(&backup_path).map_err(|_| unavailable())?;
    let backup =
        rusqlite::backup::Backup::new(connection, &mut destination).map_err(|_| unavailable())?;
    backup
        .run_to_completion(64, Duration::from_millis(1), Some(|_| {}))
        .map_err(|_| unavailable())?;
    drop(backup);
    drop(destination);

    // Independent verification pass over the produced file.
    let probe = rusqlite::Connection::open(&backup_path).map_err(|_| unavailable())?;
    if verify_integrity(&probe).is_err() {
        return Err(unavailable());
    }
    let matches_source = matches!(
        classify(&probe),
        Ok(Layout::Versioned(found)) if found == source_version
    );
    drop(probe);
    if matches_source {
        Ok(backup_path)
    } else {
        Err(unavailable())
    }
}

fn backup_path_for(path: &Path, target_version: u32) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".backup-pre-v{target_version}"));
    PathBuf::from(name)
}

fn sidecar_paths(path: &Path) -> Vec<PathBuf> {
    vec![
        path.to_path_buf(),
        suffix_path(path, "-wal"),
        suffix_path(path, "-shm"),
    ]
}

pub(crate) fn suffix_path(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn quarantine(paths: &[PathBuf]) -> Result<Vec<PathBuf>, StorageError> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| StorageError::Unavailable)?
        .as_millis();
    let mut moved = Vec::new();
    for path in paths {
        if !path.exists() {
            continue;
        }
        let mut target = path.clone().into_os_string();
        target.push(format!(".corrupt-{stamp}"));
        let target = PathBuf::from(target);
        std::fs::rename(path, &target).map_err(|_| StorageError::Unavailable)?;
        moved.push(target);
    }
    Ok(moved)
}

fn backup_candidates(path: &Path) -> Vec<(u32, PathBuf)> {
    // Candidate matching is by sibling FILE NAME ("{store}.backup-pre-v<N>"),
    // never by the store's directory prefix: recovery must find backups next
    // to the store wherever it lives.
    let Some(store_name) = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
    else {
        return Vec::new();
    };
    let prefix = format!("{store_name}.backup-pre-v");
    let mut candidates = Vec::new();
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let entries = match std::fs::read_dir(parent) {
        Ok(entries) => entries,
        Err(_) => return candidates,
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(text) = name.to_str() else {
            continue;
        };
        let Some(digits) = text.strip_prefix(&prefix) else {
            continue;
        };
        if let Ok(version) = digits.parse::<u32>() {
            candidates.push((version, entry.path()));
        }
    }
    candidates.sort_by_key(|&(version, _)| Reverse(version));
    candidates
}

/// Probes whether `candidate` is a restorable database backup: opens
/// without creating, integrity-clean, anchored at a version this build can
/// carry forward. Read-write access (never read-only) is required because
/// SQLite cannot attach a WAL-mode database without its `-shm` sidecar;
/// the probe stays inside our own store directory and never creates files.
fn restorable(candidate: &Path) -> bool {
    let flags =
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW;
    let Ok(probe) = rusqlite::Connection::open_with_flags(candidate, flags) else {
        return false;
    };
    if verify_integrity(&probe).is_err() {
        return false;
    }
    matches!(
        classify(&probe),
        Ok(Layout::Versioned(version)) if version <= SCHEMA_VERSION
    )
}

/// Corruption recovery for the database at `path`. See module docs for the
/// remedy ladder. Idempotent: a healthy database is reported, not modified.
///
/// # Errors
/// Propagates non-corruption open failures unchanged (schema-too-new and
/// friends are decisions, not damage); reports [`StorageError`] when even
/// the recreated database cannot be opened.
pub fn recover(path: &Path) -> Result<RecoveryReport, StorageError> {
    match open_with(path, MIGRATIONS) {
        Ok(_) => {
            return Ok(RecoveryReport {
                outcome: RecoveryOutcome::AlreadyHealthy,
                quarantined: Vec::new(),
                restored_from: None,
            });
        }
        Err(error) if !error.is_corruption() => return Err(error),
        Err(_) => {}
    }

    let sides = sidecar_paths(path);
    for (_, candidate) in backup_candidates(path) {
        if !restorable(&candidate) {
            continue;
        }
        let quarantined = quarantine(&sides)?;
        std::fs::copy(&candidate, path).map_err(|_| StorageError::Unavailable)?;
        // Stale WAL/SHM of the damaged database must not bleed into the
        // restored file; quarantine already removed them.
        open_with(path, MIGRATIONS)?;
        return Ok(RecoveryReport {
            outcome: RecoveryOutcome::RestoredFromBackup,
            quarantined,
            restored_from: Some(candidate),
        });
    }

    let quarantined = quarantine(&sides)?;
    open_with(path, MIGRATIONS)?;
    Ok(RecoveryReport {
        outcome: RecoveryOutcome::QuarantinedAndRecreated,
        quarantined,
        restored_from: None,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        Layout, MIGRATIONS, Migration, SCHEMA_VERSION, backup_path_for, classify, open_with,
    };
    use crate::sqlite::error::{SchemaStage, StorageError};
    use std::path::PathBuf;

    fn scratch(label: &str) -> (tempfile::TempDir, PathBuf) {
        let temp = tempfile::tempdir().expect("temp dir");
        let path = temp.path().join(format!("{label}.sqlite3"));
        (temp, path)
    }

    /// Structure contract: the chain is contiguous, strictly forward, ends
    /// exactly at `SCHEMA_VERSION`, and every step ships non-empty SQL.
    /// A new release appends one step; nothing else about the shape may
    /// drift or older databases lose their upgrade path.
    #[test]
    fn migration_chain_is_contiguous_and_forward_only() {
        assert!(!MIGRATIONS.is_empty());
        let mut expected_from = 0u32;
        for step in MIGRATIONS {
            assert_eq!(step.from, expected_from);
            assert_eq!(step.to, expected_from + 1);
            assert!(!step.sql.trim().is_empty());
            expected_from = step.to;
        }
        assert_eq!(expected_from, SCHEMA_VERSION);
        // The initial release schema is itself part of the chain.
        assert!(MIGRATIONS.iter().any(|step| step.to == 1));
    }

    #[test]
    fn fresh_install_lands_at_current_version_idempotently() {
        let (_dir, path) = scratch("fresh");
        open_with(&path, MIGRATIONS).expect("create");
        // Second open sees Versioned(SCHEMA_VERSION) and runs zero steps;
        // no backup artifacts appear for an already-current database.
        open_with(&path, MIGRATIONS).expect("reopen");
        let parent = path.parent().unwrap();
        let leftovers: Vec<_> = std::fs::read_dir(parent)
            .expect("dir")
            .collect::<Result<Vec<_>, _>>()
            .expect("entries");
        assert!(
            leftovers
                .iter()
                .all(|entry| !entry.file_name().to_string_lossy().contains("backup-pre")),
            "no pre-migration backup should exist without an upgrade"
        );
    }

    /// Upgrade-from-every-released-version harness. For each released
    /// schema version `v`, build a database anchored exactly at `v` (as if
    /// written by the build that shipped it), then open through the FULL
    /// chain and require: final version current, anchor intact. Future
    /// releases add a step and this loop widens automatically.
    #[test]
    fn upgrades_apply_from_every_released_schema_version() {
        for released in 0..=SCHEMA_VERSION {
            let (_dir, path) = scratch("upgrade-from");
            if released > 0 {
                // Materialize the database as its release build left it:
                // every step up to and including v, inside transactions,
                // plus the matching anchor row.
                let connection = rusqlite::Connection::open(&path).expect("craft handle");
                for step in MIGRATIONS.iter().filter(|step| step.to <= released) {
                    connection.execute_batch(step.sql).expect("prefix step");
                    connection
                        .execute(
                            "UPDATE openstream_schema SET value = ?1",
                            rusqlite::params![step.to],
                        )
                        .expect("anchor bump");
                }
                drop(connection);
            }
            open_with(&path, MIGRATIONS)
                .unwrap_or_else(|error| panic!("upgrade from v{released}: {error}"));
            let connection = rusqlite::Connection::open(&path).expect("verify");
            let version: u32 = connection
                .query_row(
                    "SELECT value FROM openstream_schema WHERE key = 'schema_version'",
                    [],
                    |row| row.get(0),
                )
                .expect("anchor readable");
            assert_eq!(version, SCHEMA_VERSION, "v{released} reached current");
        }
    }

    /// The v(N-1) -> vN shape every future upgrade takes: existing data gets
    /// a VERIFIED backup before DDL runs, the step applies atomically, and
    /// evidence survives with its history intact. Anchored to the REAL
    /// released chain (upgrades a store at `SCHEMA_VERSION - 1`), so a new
    /// release exercises this path against its actual step.
    #[test]
    fn upgrading_existing_data_backs_up_first_and_preserves_rows() {
        let (_dir, path) = scratch("upgrade-with-backup");
        let prior_version = SCHEMA_VERSION - 1;
        // Build a populated store anchored at the previous release schema,
        // exactly as the prior build left it.
        {
            let connection = open_with(&path, &MIGRATIONS[..prior_version as usize])
                .expect("seed at previous release");
            connection
                .execute_batch(
                    "INSERT INTO journal_admissions
                     (source_device_id, message_id, execution_id, accepted_at_wall_ms,
                      expires_at_wall_ms, lifecycle, failure_token)
                     VALUES ('device-1', 'm-1', 'e-1', 10, 20, 'accepted', NULL)",
                )
                .expect("seed row");
        }
        // Stand-in chain: the real steps plus one synthetic next step, the
        // pipeline target derives from the chain's last step, exactly as a
        // future release would drive it.
        let mut extended = MIGRATIONS.to_vec();
        extended.push(Migration {
            from: SCHEMA_VERSION,
            to: SCHEMA_VERSION + 1,
            sql: "ALTER TABLE journal_admissions
                  ADD COLUMN note TEXT NOT NULL DEFAULT '';
                  UPDATE openstream_schema SET value = 3;",
        });
        let connection = open_with(&path, &extended).expect("upgrade");
        let note: String = connection
            .query_row(
                "SELECT note FROM journal_admissions WHERE execution_id = 'e-1'",
                [],
                |row| row.get(0),
            )
            .expect("migrated row carries the new column default");
        assert_eq!(note, "");
        let version: u32 = connection
            .query_row("SELECT value FROM openstream_schema", [], |row| row.get(0))
            .expect("anchor");
        assert_eq!(version, SCHEMA_VERSION + 1);

        // The backup was produced BEFORE the final step and still anchors
        // at the version that step upgraded from.
        let backup = rusqlite::Connection::open(backup_path_for(&path, SCHEMA_VERSION + 1))
            .expect("backup opens");
        let backup_version: u32 = backup
            .query_row("SELECT value FROM openstream_schema", [], |row| row.get(0))
            .expect("backup anchor");
        assert_eq!(backup_version, SCHEMA_VERSION);
        let seeded: i64 = backup
            .query_row("SELECT count(*) FROM journal_admissions", [], |row| {
                row.get(0)
            })
            .expect("backup rows");
        assert_eq!(seeded, 1);
    }

    /// A failing step rolls back atomically: the database stays at its old
    /// version with all prior content intact, and the refusal is typed.
    #[test]
    fn failed_step_rolls_back_to_prior_version() {
        let (_dir, path) = scratch("failed-step");
        open_with(&path, MIGRATIONS).expect("seed at current schema");
        let mut broken = MIGRATIONS.to_vec();
        broken.push(Migration {
            from: SCHEMA_VERSION,
            to: SCHEMA_VERSION + 1,
            sql: "CREATE TABLE doomed (id INTEGER PRIMARY KEY); INSERT INTO missing VALUES (1);",
        });
        match open_with(&path, &broken) {
            Err(StorageError::MigrationFailed { from, to }) => {
                assert_eq!((from, to), (SCHEMA_VERSION, SCHEMA_VERSION + 1))
            }
            other => panic!("expected typed rollback, got {other:?}"),
        }
        let connection = rusqlite::Connection::open(&path).expect("post-mortem");
        let version: u32 = connection
            .query_row("SELECT value FROM openstream_schema", [], |row| row.get(0))
            .expect("anchor intact");
        assert_eq!(version, SCHEMA_VERSION);
        let doomed_tables: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name = 'doomed'",
                [],
                |row| row.get(0),
            )
            .expect("probe");
        assert_eq!(doomed_tables, 0, "partial DDL must not survive");
    }

    /// If the pre-migration backup cannot even be produced, the upgrade
    /// aborts before any DDL runs (fail closed, nothing touched).
    #[test]
    fn unavailable_backup_aborts_the_upgrade_before_ddl() {
        let (_dir, path) = scratch("backup-blocked");
        open_with(&path, MIGRATIONS).expect("seed at current schema");
        // Occupy the exact backup target with a directory: the online-backup
        // destination cannot be opened there.
        std::fs::create_dir(backup_path_for(&path, SCHEMA_VERSION + 1)).expect("occupy target");
        let mut extended = MIGRATIONS.to_vec();
        extended.push(Migration {
            from: SCHEMA_VERSION,
            to: SCHEMA_VERSION + 1,
            sql: "ALTER TABLE journal_admissions
                  ADD COLUMN note TEXT NOT NULL DEFAULT '';
                  UPDATE openstream_schema SET value = 3;",
        });
        match open_with(&path, &extended) {
            Err(StorageError::BackupUnavailable { target_version }) => {
                assert_eq!(target_version, SCHEMA_VERSION + 1)
            }
            other => panic!("expected backup abort, got {other:?}"),
        }
        let connection = rusqlite::Connection::open(&path).expect("post-mortem");
        let version: u32 = connection
            .query_row("SELECT value FROM openstream_schema", [], |row| row.get(0))
            .expect("anchor intact");
        assert_eq!(version, SCHEMA_VERSION, "DDL must not have started");
    }

    #[test]
    fn layout_classification_covers_foreign_anchor_shapes() {
        let (_dir, path) = scratch("classification");
        let connection = rusqlite::Connection::open(&path).expect("handle");
        connection
            .execute_batch("CREATE TABLE unrelated (x INTEGER)")
            .expect("foreign");
        assert_eq!(
            classify(&connection),
            Err(StorageError::UnrecognizedSchema {
                stage: SchemaStage::Foreign
            })
        );
    }

    #[test]
    fn versioned_layout_reads_the_anchor() {
        let (_dir, path) = scratch("layout-versioned");
        let connection = open_with(&path, MIGRATIONS).expect("open");
        assert_eq!(classify(&connection), Ok(Layout::Versioned(SCHEMA_VERSION)));
    }
}
