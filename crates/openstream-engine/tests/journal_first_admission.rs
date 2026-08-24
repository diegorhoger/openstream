//! Journal-first admission contract (`TECHNICAL_SPEC` §5, OSCP_MESSAGES
//! §6–§7): durable lifecycle/dedupe evidence exists BEFORE any effect can be
//! dispatched; duplicates never re-run; expired commands never queue; a
//! refused journal write blocks dispatch fail-closed; retention bounds hold.

mod common;

use common::*;
use openstream_engine::{
    ActionRegistry, AdmissionRejection, CancelSignal, ConfigError, DEDUPE_DEFAULT_RETENTION_MS,
    DEDUPE_MAX_RETENTION_MS, DEDUPE_MIN_RETENTION_MS, DedupeKey, EngineError, ExecutionJournal,
    FailurePolicy, MessageId, RuntimeBuilder, SourceDeviceId, TimeControl,
};
use std::sync::Arc;

fn registry_single(harness: &Harness) -> (ActionRegistry, Arc<ScriptedPort>) {
    let mut registry = ActionRegistry::new();
    let port = ScriptedPort::new(vec![], Arc::clone(&harness.events), harness.clock.clone());
    register_action(
        &mut registry,
        "midi.tap",
        vec![midi("stagepad")],
        false,
        false,
        port.clone(),
    );
    (registry, port)
}

fn graph_single(registry: &ActionRegistry) -> Arc<openstream_engine::ValidatedGraph> {
    single_action_graph("midi.tap", midi("stagepad"), FailurePolicy::Stop, registry)
}

fn explicit_request(
    harness: &Harness,
    key: DedupeKey,
    graph: &Arc<openstream_engine::ValidatedGraph>,
) -> openstream_engine::ExecuteRequest {
    openstream_engine::ExecuteRequest {
        source_device_id: key.source_device_id.clone(),
        message_id: key.message_id,
        subject: subject(),
        graph: Arc::clone(graph),
        variables: Default::default(),
        expires_at_wall_ms: harness.expires_at(),
        cancel: None::<CancelSignal>,
    }
}

#[test]
fn admission_and_preparation_precede_dispatch() {
    let harness = Harness::new();
    let (registry, _port) = registry_single(&harness);
    let graph = graph_single(&registry);

    let mut runtime = harness.runtime_with_journal(
        registry,
        ledger_with(&[midi("stagepad")]),
        InstrumentedJournal::new(Arc::clone(&harness.events), harness.clock.clone()),
    );
    let receipt = run_ok(&mut runtime, &harness, &graph);

    assert_eq!(receipt.state.token(), "succeeded");
    let events = harness.events.snapshot();

    let position_of = |predicate: &dyn Fn(&Event) -> bool| {
        events
            .iter()
            .position(predicate)
            .unwrap_or_else(|| panic!("missing expected event"))
    };
    let admitted = position_of(&|event| {
        matches!(
            event,
            Event::Admit {
                lifecycle: "accepted",
                ..
            }
        )
    });
    let running = position_of(&|event| {
        matches!(
            event,
            Event::Lifecycle {
                lifecycle: "running",
                ..
            }
        )
    });
    let prepared = position_of(&|event| matches!(event, Event::Prepare { .. }));
    let dispatched = position_of(&|event| matches!(event, Event::Dispatch { .. }));

    // Ordered evidence: accepted -> running -> durable prepare -> dispatch.
    assert!(admitted < running);
    assert!(running < prepared);
    assert!(prepared < dispatched);

    match &events[prepared] {
        Event::Prepare {
            node,
            idempotency_key,
            ..
        } => {
            assert_eq!(node, "a");
            // Deterministic derivation from (source_device_id, message_id).
            assert!(idempotency_key.starts_with("peer:test-device:"));
            assert!(idempotency_key.ends_with(":a:0"));
        }
        other => panic!("expected prepare event, got {other:?}"),
    }

    // Terminal evidence persisted after dispatch.
    assert!(events.iter().any(|event| matches!(
        event,
        Event::Lifecycle {
            lifecycle: "succeeded",
            ..
        }
    )));
}

#[test]
fn duplicate_admission_cites_original_without_rerunning() {
    let harness = Harness::new();
    let (registry, _port) = registry_single(&harness);
    let graph = graph_single(&registry);

    let mut runtime = harness.runtime(registry, ledger_with(&[midi("stagepad")]));

    let key = DedupeKey::new(device(), MessageId::generate());
    let first = runtime
        .execute(explicit_request(&harness, key.clone(), &graph))
        .unwrap();

    let rejection = runtime
        .execute(explicit_request(&harness, key, &graph))
        .unwrap_err();
    match rejection {
        AdmissionRejection::DuplicateSuppressed {
            original_execution_id,
            original_state,
        } => {
            assert_eq!(original_execution_id, first.execution_id);
            assert_eq!(
                original_state,
                openstream_engine::JournalLifecycle::Succeeded
            );
        }
        other => panic!("expected duplicate suppression, got {other}"),
    }

    // Exactly one dispatch total: duplicates never re-run effects.
    let dispatches = harness
        .events
        .snapshot()
        .iter()
        .filter(|event| matches!(event, Event::Dispatch { .. }))
        .count();
    assert_eq!(dispatches, 1);
}

#[test]
fn expired_commands_journal_expired_and_never_execute() {
    let harness = Harness::new();
    let (registry, port) = registry_single(&harness);
    let graph = graph_single(&registry);

    let mut runtime = harness.runtime_with_journal(
        registry,
        ledger_with(&[midi("stagepad")]),
        InstrumentedJournal::new(Arc::clone(&harness.events), harness.clock.clone()),
    );

    // Expired by one wall-clock millisecond at admission.
    harness.advance(1_000);
    let key = DedupeKey::new(device(), MessageId::generate());
    let mut request = explicit_request(&harness, key, &graph);
    request.expires_at_wall_ms = WALL_START + 500;
    let rejection = runtime.execute(request).unwrap_err();
    assert!(
        matches!(rejection, AdmissionRejection::Expired { .. }),
        "expected expiry rejection, got {rejection}"
    );

    let journaled_expired = harness.events.snapshot().iter().any(|event| {
        matches!(
            event,
            Event::Admit {
                lifecycle: "expired",
                ..
            }
        )
    });
    assert!(journaled_expired, "expired commands must journal `expired`");
    assert!(
        !harness
            .events
            .snapshot()
            .iter()
            .any(|event| matches!(event, Event::Dispatch { .. }))
    );
    let _ = port;
}

#[test]
fn refused_prepare_blocks_dispatch_fail_closed() {
    let harness = Harness::new();
    let (registry, _port) = registry_single(&harness);
    let graph = graph_single(&registry);

    let journal = InstrumentedJournal::new(Arc::clone(&harness.events), harness.clock.clone());
    journal.set_refuse_prepare(true);
    let mut runtime =
        harness.runtime_with_journal(registry, ledger_with(&[midi("stagepad")]), journal);

    let rejection = runtime.execute(request_for(&harness, &graph)).unwrap_err();
    assert!(matches!(
        rejection,
        AdmissionRejection::JournalRefused {
            source: openstream_engine::JournalError::Refused
        }
    ));

    // No effect may ride on missing durable preparation.
    assert!(
        !harness
            .events
            .snapshot()
            .iter()
            .any(|event| matches!(event, Event::Dispatch { .. }))
    );

    // Refusal left no orphan prepared records behind.
    assert!(runtime.recover_outcome_unknown().unwrap().is_empty());
}

#[test]
fn retention_bounds_are_enforced_at_build() {
    let harness = Harness::new();
    let build = |retention: i64| {
        RuntimeBuilder::new()
            .clock(harness.clock.clone())
            .time_control(harness.clock.clone() as Arc<dyn TimeControl>)
            .dedupe_retention_ms(retention)
            .build()
    };
    // Bounds are inclusive per ADR-0005 decision item 3.
    assert!(matches!(
        build(DEDUPE_MIN_RETENTION_MS - 1),
        Err(EngineError::Config(
            ConfigError::RetentionOutOfBounds { .. }
        ))
    ));
    assert!(matches!(
        build(DEDUPE_MAX_RETENTION_MS + 1),
        Err(EngineError::Config(
            ConfigError::RetentionOutOfBounds { .. }
        ))
    ));
    assert!(build(DEDUPE_MIN_RETENTION_MS).is_ok());
    assert!(build(DEDUPE_MAX_RETENTION_MS).is_ok());
    // Default (24 h) sits inside the window.
    assert!(build(DEDUPE_DEFAULT_RETENTION_MS).is_ok());
}

#[test]
fn prune_is_oldest_first_and_exempts_outcome_unknown() {
    use openstream_engine::{AdmissionEntry, JournalLifecycle, MemoryJournal};

    let mut journal = MemoryJournal::new();
    let device_id = SourceDeviceId::try_new("peer:prune-test").unwrap();

    let old = WALL_START - 10 * DEDUPE_DEFAULT_RETENTION_MS;
    let fresh = WALL_START;

    let admit = |journal: &mut MemoryJournal, at: i64, lifecycle: JournalLifecycle| {
        journal
            .admit(AdmissionEntry {
                key: DedupeKey::new(device_id.clone(), MessageId::generate()),
                execution_id: openstream_engine::ExecutionId::generate(),
                accepted_at_wall_ms: at,
                expires_at_wall_ms: at,
                lifecycle,
            })
            .expect("fixture admission must fit capacity");
    };

    admit(&mut journal, old, JournalLifecycle::Succeeded); // prunable
    admit(&mut journal, old, JournalLifecycle::OutcomeUnknown); // exempt
    admit(&mut journal, fresh, JournalLifecycle::Accepted); // inside window

    journal.prune(WALL_START, DEDUPE_DEFAULT_RETENTION_MS);

    let survivors = journal.snapshot_admissions();
    assert_eq!(
        survivors.len(),
        2,
        "oldest prunable entry goes, others stay"
    );
    assert!(
        survivors
            .iter()
            .any(|entry| { entry.lifecycle == JournalLifecycle::OutcomeUnknown })
    );
    assert!(
        survivors
            .iter()
            .any(|entry| { entry.accepted_at_wall_ms == fresh })
    );
}
