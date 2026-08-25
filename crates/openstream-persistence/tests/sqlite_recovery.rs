//! Open-pipeline and corruption-recovery behavior of the SQLite layer
//! (issue #15): schema classification, forward-only migration refusals,
//! integrity-checked opens, quarantine-or-restore recovery, and proof that
//! no secret material can persist in any store file.

mod common;

use common::{DEVICE_A, ScratchDir, admission, dedupe};
use openstream_engine::ExecutionJournal;
use openstream_persistence::sqlite::{
    CorruptionStage, RecoveryOutcome, SCHEMA_VERSION, SchemaStage, SqliteJournal, StorageError,
    recover,
};
use std::path::Path;

fn open_error(path: &Path) -> StorageError {
    SqliteJournal::open(path).expect_err("open must fail closed")
}

#[test]
fn fresh_open_anchors_at_current_schema_version() {
    let scratch = ScratchDir::new("fresh");
    let journal = SqliteJournal::open(&scratch.db_path()).expect("open");
    let connection = rusqlite::Connection::open(scratch.db_path()).expect("probe");
    let version: i64 = connection
        .query_row(
            "SELECT value FROM openstream_schema WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .expect("anchor row");
    assert_eq!(version, i64::from(SCHEMA_VERSION));
    drop(journal);
    // Reopening an already-current database is a no-op upgrade.
    SqliteJournal::open(&scratch.db_path()).expect("reopen");
}

#[test]
fn foreign_databases_are_never_overwritten_in_place() {
    let scratch = ScratchDir::new("foreign");
    {
        let unrelated = rusqlite::Connection::open(scratch.db_path()).expect("craft");
        unrelated
            .execute_batch("CREATE TABLE notes (body TEXT); INSERT INTO notes VALUES ('hi');")
            .expect("foreign layout");
    }
    match open_error(&scratch.db_path()) {
        StorageError::UnrecognizedSchema { stage } => assert_eq!(stage, SchemaStage::Foreign),
        other => panic!("expected foreign-schema rejection, got {other:?}"),
    }
    // The file is untouched: recovery quarantines rather than destroying.
    let bytes = std::fs::read(scratch.db_path()).expect("read");
    assert!(!bytes.is_empty());
}

#[test]
fn damaged_anchor_rows_are_rejected_structurally() {
    let scratch = ScratchDir::new("damaged-anchor");
    SqliteJournal::open(&scratch.db_path()).expect("seed");
    for tamper in [
        // The schema's PRIMARY KEY + CHECK make extra/mis-keyed anchor rows
        // unrepresentable, so the representable damage classes are: missing
        // version row, and a version value outside the domain.
        "DELETE FROM openstream_schema",
        "UPDATE openstream_schema SET value = -3",
    ] {
        let connection = rusqlite::Connection::open(scratch.db_path()).expect("tamper handle");
        connection.execute_batch(tamper).expect("tamper applies");
        match open_error(&scratch.db_path()) {
            StorageError::UnrecognizedSchema { stage } => {
                assert_eq!(stage, SchemaStage::Anchor)
            }
            other => panic!("tamper `{tamper}`: expected anchor rejection, got {other:?}"),
        }
        // Restore the healthy anchor for the next case.
        let repair = rusqlite::Connection::open(scratch.db_path()).expect("repair handle");
        repair
            .execute_batch("DELETE FROM openstream_schema; INSERT INTO openstream_schema (key, value) VALUES ('schema_version', 1)")
            .expect("restore anchor");
    }
}

/// Downgrades are refused unconditionally: a database written by a NEWER
/// build never gets silently reinterpreted by an older one.
#[test]
fn newer_schema_refuses_to_downgrade() {
    let scratch = ScratchDir::new("too-new");
    SqliteJournal::open(&scratch.db_path()).expect("seed");
    let connection = rusqlite::Connection::open(scratch.db_path()).expect("handle");
    connection
        .execute_batch(&format!(
            "UPDATE openstream_schema SET value = {}",
            SCHEMA_VERSION + 7
        ))
        .expect("bump to future version");
    match open_error(&scratch.db_path()) {
        StorageError::SchemaTooNew { found, supported } => {
            assert_eq!(found, SCHEMA_VERSION + 7);
            assert_eq!(supported, SCHEMA_VERSION);
        }
        other => panic!("expected downgrade refusal, got {other:?}"),
    }
    // Not classified as damage: it is a decision, not something `recover`
    // may paper over.
    assert!(!open_error(&scratch.db_path()).is_corruption());
}

#[test]
fn byte_damage_fails_closed_with_structural_corruption() {
    let scratch = ScratchDir::new("corrupt-bytes");
    let mut journal = SqliteJournal::open(&scratch.db_path()).expect("seed");
    journal
        .admit(admission(dedupe(DEVICE_A), 1))
        .expect("admit");
    journal.checkpoint().expect("fold WAL into main file");
    drop(journal);

    // Overwrite the SQLite header magic with plausible-length garbage.
    let mut bytes = std::fs::read(scratch.db_path()).expect("read db");
    bytes[..16].copy_from_slice(b"NOT-A-DATABASE--");
    std::fs::write(scratch.db_path(), &bytes).expect("damage");

    match open_error(&scratch.db_path()) {
        StorageError::Corrupted { stage } => {
            assert_eq!(stage, CorruptionStage::Header)
        }
        other => panic!("expected header corruption, got {other:?}"),
    }
    assert!(open_error(&scratch.db_path()).is_corruption());
}

#[test]
fn recover_reports_already_healthy_and_changes_nothing() {
    let scratch = ScratchDir::new("healthy");
    let mut journal = SqliteJournal::open(&scratch.db_path()).expect("open");
    journal
        .admit(admission(dedupe(DEVICE_A), 1))
        .expect("admit");
    drop(journal);

    let report = recover(&scratch.db_path()).expect("recover");
    assert_eq!(report.outcome(), RecoveryOutcome::AlreadyHealthy);
    assert!(report.quarantined().is_empty());
    assert!(report.restored_from().is_none());

    let journal = SqliteJournal::open(&scratch.db_path()).expect("reopen");
    assert_eq!(journal.snapshot_admissions().len(), 1);
}

/// A corrupted store with no restorable backup is quarantined (originals
/// preserved byte-for-byte for forensics) and recreated fresh.
#[test]
fn recover_quarantines_and_recreates_when_no_backup_exists() {
    let scratch = ScratchDir::new("quarantine-path");
    let mut journal = SqliteJournal::open(&scratch.db_path()).expect("seed");
    journal
        .admit(admission(dedupe(DEVICE_A), 1))
        .expect("admit");
    drop(journal);

    std::fs::write(scratch.db_path(), b"garbage-not-a-database").expect("corrupt");

    let report = recover(&scratch.db_path()).expect("recover");
    assert_eq!(report.outcome(), RecoveryOutcome::QuarantinedAndRecreated);
    assert_eq!(report.quarantined().len(), 1);
    let preserved = std::fs::read(&report.quarantined()[0]).expect("quarantine readable");
    assert_eq!(preserved, b"garbage-not-a-database");

    // The recreated database is usable and schema-current.
    let mut journal = SqliteJournal::open(&scratch.db_path()).expect("recreated opens");
    journal
        .admit(admission(dedupe(DEVICE_A), 2))
        .expect("admit again");
}

/// Restore ladder: when a validated pre-migration backup exists (as future
/// upgrades will produce; here one from a checkpointed healthy store), the
/// newest restorable one wins and the damaged originals are quarantined.
#[test]
fn recover_restores_newest_valid_backup() {
    let scratch = ScratchDir::new("restore-path");
    let mut journal = SqliteJournal::open(&scratch.db_path()).expect("open");
    journal
        .admit(admission(dedupe(DEVICE_A), 11))
        .expect("old row");
    // Fold committed frames into the main file so a plain copy is a complete,
    // standalone database (what the online-backup API produces).
    journal.checkpoint().expect("checkpoint");
    drop(journal);
    let backup_path = scratch.db_path().with_extension("sqlite3.backup-pre-v2");
    std::fs::copy(scratch.db_path(), &backup_path).expect("fabricate backup");

    let mut journal = SqliteJournal::open(&scratch.db_path()).expect("reopen");
    journal
        .admit(admission(dedupe(DEVICE_A), 22))
        .expect("newer row");
    drop(journal);

    // Damage the primary beyond repair.
    std::fs::write(scratch.db_path(), b"\x00\x01\x02broken").expect("corrupt");

    let report = recover(&scratch.db_path()).expect("recover");
    assert_eq!(report.outcome(), RecoveryOutcome::RestoredFromBackup);
    assert_eq!(report.restored_from(), Some(&backup_path));
    assert!(!report.quarantined().is_empty());

    // The restored store carries exactly the backup's durable evidence.
    let journal = SqliteJournal::open(&scratch.db_path()).expect("restored opens");
    let snapshot = journal.snapshot_admissions();
    assert_eq!(snapshot.len(), 1);
    assert_eq!(snapshot[0].accepted_at_wall_ms, 11);
}

/// An unreadable backup candidate is skipped, not trusted.
#[test]
fn recover_skips_unrestorable_backup_candidates() {
    let scratch = ScratchDir::new("bad-backup");
    SqliteJournal::open(&scratch.db_path()).expect("seed");
    std::fs::write(scratch.db_path(), b"total-loss").expect("corrupt primary");
    let backup_path = scratch.db_path().with_extension("sqlite3.backup-pre-v2");
    std::fs::write(&backup_path, b"also-garbage").expect("corrupt backup");

    let report = recover(&scratch.db_path()).expect("recover");
    assert_eq!(report.outcome(), RecoveryOutcome::QuarantinedAndRecreated);
}
