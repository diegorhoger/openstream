//! `SqliteJournal` against the engine's `ExecutionJournal` port (issue #15):
//! round-trip fidelity, fail-closed refusals, atomic autosave durability
//! (reopen after process death), crash-window transaction discipline, and a
//! model-based property comparison against the authoritative in-memory
//! journal (`TECHNICAL_SPEC` §6, OSCP_MESSAGES §7–§8).
//!
//! Gate-review guard: this file must always contribute executable tests.
//! Verify with `cargo test --test sqlite_journal -- --list` (>= 10 tests)
//! before pushing; a newline-stripped copy parses as one comment and runs
//! zero tests while CI stays green.

mod common;

use common::{DEVICE_A, DEVICE_B, ScratchDir, admission, dedupe, prepared};
use openstream_engine::{
    ExecutionId, ExecutionJournal, JournalError, JournalLifecycle, MemoryJournal,
};
use openstream_persistence::sqlite::SqliteJournal;
use proptest::prelude::*;

#[test]
fn admit_lookup_snapshot_round_trip_all_lifecycles() {
    let scratch = ScratchDir::new("round-trip");
    let mut journal = SqliteJournal::open(&scratch.db_path()).expect("open");

    let first = admission(dedupe(DEVICE_A), 1_000);
    let second = admission(dedupe(DEVICE_B), 2_000);
    journal.admit(first.clone()).expect("admit first");
    journal.admit(second.clone()).expect("admit second");

    // Dedupe lookup returns the exact record.
    assert_eq!(journal.lookup(&first.key), Some(first.clone()));
    assert_eq!(journal.lookup(&second.key), Some(second.clone()));
    // Snapshot preserves insertion order.
    assert_eq!(
        journal.snapshot_admissions(),
        vec![first.clone(), second.clone()]
    );

    for lifecycle in [
        JournalLifecycle::Running,
        JournalLifecycle::Succeeded,
        JournalLifecycle::Failed {
            token: "denied".to_string(),
        },
        JournalLifecycle::Cancelled,
        JournalLifecycle::Expired,
        JournalLifecycle::OutcomeUnknown,
    ] {
        journal
            .set_lifecycle(first.execution_id, lifecycle.clone())
            .expect("transition applies");
        let stored = journal.lookup(&first.key).expect("row present");
        assert_eq!(stored.lifecycle, lifecycle);
    }

    // The failure token survives the round trip exactly.
    journal
        .set_lifecycle(
            second.execution_id,
            JournalLifecycle::Failed {
                token: "adapter_unavailable".to_string(),
            },
        )
        .expect("terminal applies");
    assert_eq!(
        journal.lookup(&second.key).unwrap().lifecycle,
        JournalLifecycle::Failed {
            token: "adapter_unavailable".to_string()
        }
    );
}

#[test]
fn set_lifecycle_unknown_execution_fails_closed() {
    let scratch = ScratchDir::new("unknown-execution");
    let mut journal = SqliteJournal::open(&scratch.db_path()).expect("open");
    assert_eq!(
        journal.set_lifecycle(ExecutionId::generate(), JournalLifecycle::Running),
        Err(JournalError::UnknownExecution)
    );
}

#[test]
fn duplicate_dedupe_and_duplicate_execution_refuse() {
    let scratch = ScratchDir::new("duplicates");
    let mut journal = SqliteJournal::open(&scratch.db_path()).expect("open");
    let entry = admission(dedupe(DEVICE_A), 5_000);
    journal.admit(entry.clone()).expect("first insert");

    // Same dedupe key again must fail closed, never shadow evidence.
    assert!(matches!(
        journal.admit(entry.clone()),
        Err(JournalError::Refused)
    ));

    // Same execution id under a different dedupe key is equally refused:
    // execution identity stays Engine-unique.
    let replayed_key = admission(dedupe(DEVICE_B), 6_000);
    let mut same_execution = replayed_key;
    same_execution.execution_id = entry.execution_id;
    assert!(matches!(
        journal.admit(same_execution),
        Err(JournalError::Refused)
    ));
    // The refused writes left exactly one intact row.
    assert_eq!(journal.snapshot_admissions().len(), 1);
}

#[test]
fn prepared_records_resolve_and_preserve_order() {
    let scratch = ScratchDir::new("prepared-order");
    let mut journal = SqliteJournal::open(&scratch.db_path()).expect("open");
    let entry = admission(dedupe(DEVICE_A), 1_000);
    journal.admit(entry.clone()).expect("admit");

    journal
        .prepare(prepared(entry.execution_id, "node-a", 0))
        .expect("prepare a0");
    journal
        .prepare(prepared(entry.execution_id, "node-b", 0))
        .expect("prepare b0");

    let unresolved = journal.unresolved_prepared();
    assert_eq!(unresolved.len(), 2);
    assert_eq!(unresolved[0].node_key.as_str(), "node-a");
    assert_eq!(unresolved[1].node_key.as_str(), "node-b");

    let (_, node_key_a, _) = unresolved[0].identity();
    journal
        .resolve_prepared(entry.execution_id, &node_key_a, 0)
        .expect("resolve node-a");
    let unresolved = journal.unresolved_prepared();
    assert_eq!(unresolved.len(), 1);
    assert_eq!(unresolved[0].node_key.as_str(), "node-b");

    // Resolving an already-closed or unknown preparation stays tolerant,
    // mirroring the in-memory journal's crash-recovery semantics.
    let (_, node_key_b, _) = unresolved[0].identity();
    journal
        .resolve_prepared(entry.execution_id, &node_key_b, 7)
        .expect("tolerant resolve");
}

/// Regression for the gate-caught divergence: resolution closes exactly ONE
/// preparation instance per call. A duplicate preparation (retry cycles can
/// re-stage the same identity) keeps its sibling open until its own outcome
/// is observed — matching `MemoryJournal`'s first-position removal, never a
/// blanket multi-delete.
#[test]
fn resolve_prepared_closes_exactly_one_duplicate_preparation() {
    let scratch = ScratchDir::new("duplicate-preparation");
    let mut journal = SqliteJournal::open(&scratch.db_path()).expect("open");
    let entry = admission(dedupe(DEVICE_A), 1_000);
    journal.admit(entry.clone()).expect("admit");

    let staging = prepared(entry.execution_id, "node-a", 0);
    journal.prepare(staging.clone()).expect("first staging");
    journal.prepare(staging.clone()).expect("duplicate staging");
    assert_eq!(journal.unresolved_prepared().len(), 2);

    journal
        .resolve_prepared(entry.execution_id, &staging.node_key, staging.attempt)
        .expect("first resolution");
    let unresolved = journal.unresolved_prepared();
    assert_eq!(
        unresolved.len(),
        1,
        "exactly one sibling must remain after one resolution"
    );
    assert_eq!(unresolved[0], staging);

    journal
        .resolve_prepared(entry.execution_id, &staging.node_key, staging.attempt)
        .expect("second resolution");
    assert!(journal.unresolved_prepared().is_empty());
}

#[test]
fn capacity_bounds_fail_closed_without_dropping_evidence() {
    let scratch = ScratchDir::new("capacity");
    let bounds = common::JournalBounds {
        max_admissions: 2,
        max_open_prepared: 1,
    };
    let mut journal =
        SqliteJournal::open_bounded(&scratch.db_path(), bounds).expect("open bounded");

    let first = admission(dedupe(DEVICE_A), 1_000);
    let second = admission(dedupe(DEVICE_A), 2_000);
    let third = admission(dedupe(DEVICE_A), 3_000);
    journal.admit(first.clone()).expect("admit 1");
    journal.admit(second.clone()).expect("admit 2");
    assert_eq!(
        journal.admit(third),
        Err(JournalError::Capacity {
            what: "journal admissions",
            limit: 2,
        })
    );

    journal
        .prepare(prepared(first.execution_id, "node-a", 0))
        .expect("prepare 1");
    assert_eq!(
        journal.prepare(prepared(second.execution_id, "node-b", 0)),
        Err(JournalError::Capacity {
            what: "open prepared records",
            limit: 1,
        })
    );
    // Refusals kept every accepted record readable and unchanged.
    assert_eq!(journal.snapshot_admissions().len(), 2);
    assert_eq!(journal.unresolved_prepared().len(), 1);
}

#[test]
fn prune_keeps_recent_and_outcome_unknown_exempt() {
    let scratch = ScratchDir::new("prune");
    let mut journal = SqliteJournal::open(&scratch.db_path()).expect("open");

    let old_terminal = admission(dedupe(DEVICE_A), 1_000);
    let old_unknown = admission(dedupe(DEVICE_A), 1_100);
    let recent = admission(dedupe(DEVICE_B), 9_000);
    for entry in [&old_terminal, &old_unknown, &recent] {
        journal.admit(entry.clone()).expect("admit");
    }
    journal
        .set_lifecycle(old_terminal.execution_id, JournalLifecycle::Succeeded)
        .expect("terminal");
    journal
        .set_lifecycle(old_unknown.execution_id, JournalLifecycle::OutcomeUnknown)
        .expect("corrective unknown");

    // Retention window of 5s at t=10s prunes only the old terminal row.
    journal.prune(10_000, 5_000);

    let snapshot = journal.snapshot_admissions();
    let ids: Vec<_> = snapshot.iter().map(|e| e.execution_id).collect();
    assert!(!ids.contains(&old_terminal.execution_id));
    assert!(ids.contains(&old_unknown.execution_id));
    assert!(ids.contains(&recent.execution_id));
}

/// Atomic autosave evidence: dropping the journal object simulates process
/// death after committed writes. Reopening replays the WAL; every commit
/// that returned `Ok` is present, without any explicit flush step.
#[test]
fn reopen_after_process_death_recovers_every_commit() {
    let scratch = ScratchDir::new("process-death");
    let entry = admission(dedupe(DEVICE_A), 42_000);

    let resolved_execution;
    {
        let mut journal = SqliteJournal::open(&scratch.db_path()).expect("open");
        journal.admit(entry.clone()).expect("admit");
        journal
            .set_lifecycle(entry.execution_id, JournalLifecycle::Running)
            .expect("running");
        journal
            .prepare(prepared(entry.execution_id, "obs-scene-set", 0))
            .expect("prepare");
        resolved_execution = entry.execution_id;
        // No checkpoint call: the connection closes with live WAL frames.
    }

    // "New process": fresh connection over the same files.
    let mut journal = SqliteJournal::open(&scratch.db_path()).expect("reopen");
    let mut expected = entry.clone();
    expected.lifecycle = JournalLifecycle::Running;
    assert_eq!(journal.snapshot_admissions(), vec![expected]);
    assert_eq!(journal.unresolved_prepared().len(), 1);

    // Recovery closes the crash gap with corrective evidence — exactly
    // what the runtime's recovery paths do: relabel from durable facts and
    // resolve every open preparation for the execution.
    journal
        .set_lifecycle(resolved_execution, JournalLifecycle::OutcomeUnknown)
        .expect("relabel unknown");
    let unresolved = journal.unresolved_prepared();
    assert_eq!(unresolved.len(), 1);
    let (execution_id, node_key, attempt) = unresolved[0].identity();
    journal
        .resolve_prepared(execution_id, &node_key, attempt)
        .expect("close crash gap");
    drop(journal);

    let journal = SqliteJournal::open(&scratch.db_path()).expect("reopen 2");
    assert_eq!(
        journal.lookup(&entry.key).unwrap().lifecycle,
        JournalLifecycle::OutcomeUnknown
    );
    assert!(journal.unresolved_prepared().is_empty());
}

/// Crash-window discipline (OSCP_MESSAGES §7): an abandoned in-flight
/// write can never surface. The prior committed state stands untouched —
/// there is no torn hybrid row for recovery to guess about.
#[test]
fn abandoned_inflight_write_leaves_committed_state_intact() {
    let scratch = ScratchDir::new("torn-write");
    let committed = admission(dedupe(DEVICE_A), 7_000);
    let phantom_key = dedupe(DEVICE_B);

    {
        let mut journal = SqliteJournal::open(&scratch.db_path()).expect("open");
        journal.admit(committed.clone()).expect("commit admission");
    }
    {
        // A writer that dies mid-statement: uncommitted INSERT then the
        // connection drops without any commit call.
        let doomed = rusqlite::Connection::open(scratch.db_path()).expect("second handle");
        doomed.execute_batch("BEGIN IMMEDIATE").expect("begin");
        doomed
            .execute(
                "INSERT INTO journal_admissions
                 (source_device_id, message_id, execution_id, accepted_at_wall_ms,
                  expires_at_wall_ms, lifecycle, failure_token)
                 VALUES (?1, ?2, ?3, 1, 1, 'accepted', NULL)",
                rusqlite::params![
                    phantom_key.source_device_id.as_str(),
                    phantom_key.message_id.as_uuid().to_string(),
                    "not-even-a-uuid",
                ],
            )
            .expect("in-flight insert staged");
        // Drop with the transaction still open: rollback on close.
    }

    let journal = SqliteJournal::open(&scratch.db_path()).expect("reopen");
    assert_eq!(journal.snapshot_admissions(), vec![committed]);
    assert_eq!(journal.lookup(&phantom_key), None);
}

#[test]
fn checkpoint_truncates_the_write_ahead_log() {
    let scratch = ScratchDir::new("checkpoint");
    let mut journal = SqliteJournal::open(&scratch.db_path()).expect("open");
    journal
        .admit(admission(dedupe(DEVICE_A), 1))
        .expect("admit");
    journal.checkpoint().expect("checkpoint completes");
}

// Model-based equivalence: a randomized operation sequence applied to both
// the SQLite journal and the authoritative `MemoryJournal` produces
// identical observable snapshots at every step. Admits always mint fresh
// message ids because the runtime answers replays through `lookup`
// (dedupe) rather than re-admission; duplicate-insert refusal divergence
// is pinned separately by `duplicate_dedupe_and_duplicate_execution_refuse`,
// and duplicate-preparation resolution by
// `resolve_prepared_closes_exactly_one_duplicate_preparation`.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn sqlite_matches_in_memory_model_under_random_sequences(
        seed in any::<u64>(),
        steps in 8usize..40,
    ) {
        let mut lcg = seed;
        let mut next_u64 = move || {
            lcg = lcg
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            lcg >> 33
        };

        let scratch = ScratchDir::new("property-model");
        let mut sqlite = SqliteJournal::open(&scratch.db_path()).expect("open");
        let mut memory = MemoryJournal::new();

        // Admissions created so far, indexed for lifecycle/prepare ops.
        let mut admitted: Vec<ExecutionId> = Vec::new();
        const FAILURE_TOKENS: [&str; 3] = ["denied", "timeout", "adapter_unavailable"];

        for _ in 0..steps {
            let choice = next_u64() % 6;
            match choice {
                0 | 1 => {
                    let wall = 1_000i64 + next_u64() as i64 % 50_000;
                    let key = dedupe(if next_u64() % 2 == 0 { DEVICE_A } else { DEVICE_B });
                    let entry = admission(key, wall);
                    let result_memory = memory.admit(entry.clone());
                    let result_sqlite = sqlite.admit(entry.clone());
                    prop_assert_eq!(result_memory, result_sqlite);
                    admitted.push(entry.execution_id);
                }
                2 => {
                    if admitted.is_empty() {
                        continue;
                    }
                    let index = (next_u64() as usize) % admitted.len();
                    let execution_id = admitted[index];
                    let lifecycle = match next_u64() % 6 {
                        0 => JournalLifecycle::Running,
                        1 => JournalLifecycle::Succeeded,
                        2 => JournalLifecycle::Failed {
                            token: FAILURE_TOKENS[(next_u64() as usize) % FAILURE_TOKENS.len()]
                                .to_string(),
                        },
                        3 => JournalLifecycle::Cancelled,
                        4 => JournalLifecycle::Expired,
                        _ => JournalLifecycle::OutcomeUnknown,
                    };
                    let memory_result = memory.set_lifecycle(execution_id, lifecycle.clone());
                    let sqlite_result = sqlite.set_lifecycle(execution_id, lifecycle);
                    prop_assert_eq!(memory_result, sqlite_result);
                }
                3 => {
                    if admitted.is_empty() {
                        continue;
                    }
                    let index = (next_u64() as usize) % admitted.len();
                    let execution_id = admitted[index];
                    let node = format!("node-{}", next_u64() % 4);
                    let attempt = (next_u64() % 3) as u32;
                    let entry = prepared(execution_id, &node, attempt);
                    let memory_result = memory.prepare(entry.clone());
                    let sqlite_result = sqlite.prepare(entry);
                    prop_assert_eq!(memory_result, sqlite_result);
                }
                4 => {
                    let unresolved = memory.unresolved_prepared();
                    if unresolved.is_empty() {
                        continue;
                    }
                    let index = (next_u64() as usize) % unresolved.len();
                    let target = unresolved[index].clone();
                    let (execution_id, node_key, attempt) = target.identity();
                    let memory_result = memory.resolve_prepared(execution_id, &node_key, attempt);
                    let sqlite_result = sqlite.resolve_prepared(execution_id, &node_key, attempt);
                    prop_assert_eq!(memory_result, sqlite_result);
                }
                _ => {
                    let now = 60_000i64 + next_u64() as i64 % 20_000;
                    let retention = 1_000i64 + next_u64() as i64 % 30_000;
                    memory.prune(now, retention);
                    sqlite.prune(now, retention);
                }
            }

            // Observable state matches exactly, including stable order.
            prop_assert_eq!(memory.snapshot_admissions(), sqlite.snapshot_admissions());
            prop_assert_eq!(memory.unresolved_prepared(), sqlite.unresolved_prepared());
        }
    }
}
