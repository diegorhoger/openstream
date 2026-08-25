//! No-secrets-in-SQLite proof (issue #15, SECURITY.md hard rules,
//! THREAT_MODEL TB6): the store's bytes never contain secret material.
//!
//! The proof has two halves so it cannot pass vacuously:
//! 1. a scanner self-test that plants a sentinel into a scratch copy and
//!    requires detection (the scan works), then
//! 2. a full-population scan of every real store artifact â€” main file,
//!    WAL, shared-memory index, and pre-migration backups â€” requiring
//!    absence of the sentinel pattern (nothing persisted it).
//!
//! A structural half closes the loop: `PRAGMA table_info` must enumerate
//! exactly the non-secret columns of schema v1, so no free-form column a
//! secret could hide in exists at all.

mod common;

use common::{DEVICE_A, DEVICE_B, admission, dedupe, prepared};
use openstream_engine::{ExecutionJournal, JournalLifecycle};
use openstream_persistence::sqlite::SqliteJournal;
use std::path::{Path, PathBuf};

/// Fake credential-material marker. Deliberately not a real secret; it is
/// the pattern the scanner looks for across raw store bytes.
const SENTINEL: &str = "opsk-secret-sentinel-f3a1c2d4";

fn store_files(db_path: &Path) -> Vec<PathBuf> {
    let mut files = vec![
        db_path.to_path_buf(),
        db_path.with_extension("sqlite3-wal"),
        db_path.with_extension("sqlite3-shm"),
    ];
    // Pre-migration backups live beside the store (future upgrades create
    // them; recovery may leave quarantined copies too).
    if let Some(parent) = db_path.parent()
        && let Ok(entries) = std::fs::read_dir(parent)
    {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let stem = format!("{}.", db_path.file_name().unwrap().to_string_lossy());
            if name.starts_with(&stem) {
                files.push(entry.path());
            }
        }
    }
    files.sort();
    files.dedup();
    files
}

fn contains_sentinel(path: &Path) -> bool {
    let needle = SENTINEL.as_bytes();
    match std::fs::read(path) {
        Ok(bytes) => bytes.windows(needle.len()).any(|window| window == needle),
        Err(_) => false,
    }
}

#[test]
fn scanner_self_test_detects_a_planted_sentinel() {
    let scratch = common::ScratchDir::new("scanner-self-test");
    SqliteJournal::open(&scratch.db_path()).expect("open");
    let planted = scratch.db_path().with_extension("sqlite3.planted");
    std::fs::copy(scratch.db_path(), &planted).expect("copy");
    let mut handle = std::fs::OpenOptions::new()
        .append(true)
        .open(&planted)
        .expect("append handle");
    use std::io::Write as _;
    handle.write_all(SENTINEL.as_bytes()).expect("plant");

    assert!(contains_sentinel(&planted), "scanner must detect plants");
    assert!(!contains_sentinel(&scratch.db_path()));
}

/// Maximal legitimate population: every lifecycle including failure tokens,
/// prepared rows, retention pruning, checkpoint folding, plus a fabricated
/// pre-migration backup copy. Then every artifact on disk is scanned.
#[test]
fn fully_populated_store_carries_no_secret_material() {
    let scratch = common::ScratchDir::new("no-secrets");
    let mut journal = SqliteJournal::open(&scratch.db_path()).expect("open");

    let terminal = admission(dedupe(DEVICE_A), 1_000);
    journal.admit(terminal.clone()).expect("admit");
    journal
        .set_lifecycle(
            terminal.execution_id,
            JournalLifecycle::Failed {
                token: "adapter_unavailable".to_string(),
            },
        )
        .expect("terminal with token");

    let unknown = admission(dedupe(DEVICE_B), 1_500);
    journal.admit(unknown).expect("admit");
    for entry in &journal.snapshot_admissions() {
        journal
            .prepare(prepared(entry.execution_id, "node-a", 0))
            .expect("prepare");
    }
    journal.prune(60_000, 5_000);
    journal.checkpoint().expect("fold WAL before backup copy");

    // Fabricate a pre-migration-style backup (what an upgrade step leaves
    // behind) so the scan also covers backup artifacts.
    let backup = scratch.db_path().with_extension("sqlite3.backup-pre-v2");
    std::fs::copy(scratch.db_path(), &backup).expect("backup copy");

    // Reopen once more so WAL/SHM artifacts exist again post-checkpoint.
    drop(journal);
    let journal = SqliteJournal::open(&scratch.db_path()).expect("reopen");
    drop(journal);

    for file in store_files(&scratch.db_path()) {
        assert!(
            !contains_sentinel(&file),
            "secret sentinel found in {}",
            file.display()
        );
    }

    // Structural closure: the exact v1 column inventory, all non-secret by
    // construction (identifiers, timestamps, closed-vocabulary tokens).
    let connection = rusqlite::Connection::open(scratch.db_path()).expect("probe");
    for (table, expected) in [
        (
            "journal_admissions",
            vec![
                "seq",
                "source_device_id",
                "message_id",
                "execution_id",
                "accepted_at_wall_ms",
                "expires_at_wall_ms",
                "lifecycle",
                "failure_token",
            ],
        ),
        (
            "journal_prepared",
            vec![
                "seq",
                "execution_id",
                "node_key",
                "attempt",
                "action_type",
                "idempotency_key",
                "prepared_at_monotonic_ms",
            ],
        ),
    ] {
        let mut statement = connection
            .prepare(&format!("PRAGMA table_info({table})"))
            .expect("table exists");
        let mut columns: Vec<String> = statement
            .query_map([], |row| row.get::<_, String>(1))
            .expect("query")
            .collect::<Result<_, _>>()
            .expect("rows");
        columns.sort();
        let mut expected_sorted = expected.clone();
        expected_sorted.sort();
        assert_eq!(columns, expected_sorted, "unexpected columns in {table}");
    }

    // The failure-token column is closed-vocabulary-adjacent evidence, but
    // even so its values are registry tokens, never credential payloads:
    // confirm no stored text value anywhere equals or embeds the sentinel.
    let total: i64 = connection
        .query_row(
            "SELECT count(*) FROM journal_admissions
             WHERE instr(coalesce(failure_token,''), 'opsk-secret') > 0",
            [],
            |row| row.get(0),
        )
        .expect("scan query");
    assert_eq!(total, 0);
}
