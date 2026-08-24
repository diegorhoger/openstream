//! Crash-gap honesty (`PROTOCOL.md`, OSCP_MESSAGES §7–§8): lost outcomes
//! journal `outcome_unknown`, never success; recovery closes orphaned
//! prepared records with corrective unknown evidence; replay is gated on
//! idempotency and reuses the original execution id with a stable
//! adapter-facing key derived from `(source_device_id, message_id)`.

mod common;

use common::*;
use openstream_engine::{
    ActionRegistry, CancelSignal, DedupeKey, ExecutionJournal, FailurePolicy, JournalLifecycle,
    MemoryJournal, MessageId, PreparedEntry, ReplayRejection, TimeControl, ValidatedGraph,
};
use std::sync::Arc;

struct Fixture {
    runtime: openstream_engine::ActionRuntime,
    registry: Arc<ActionRegistry>,
}

/// One action type whose scripted outcomes drive each scenario.
fn fixture(harness: &Harness, steps: Vec<Step>, idempotent: bool) -> Fixture {
    let mut registry = ActionRegistry::new();
    let port = ScriptedPort::new(steps, Arc::clone(&harness.events), harness.clock.clone());
    register_action(
        &mut registry,
        "midi.tap",
        vec![midi("stagepad")],
        idempotent,
        false,
        port.clone(),
    );
    let registry = Arc::new(registry);
    let runtime = harness.runtime_with_journal(
        (*registry).clone(),
        ledger_with(&[midi("stagepad")]),
        InstrumentedJournal::new(Arc::clone(&harness.events), harness.clock.clone()),
    );
    Fixture { runtime, registry }
}

impl Fixture {
    fn graph(&self) -> Arc<ValidatedGraph> {
        single_action_graph(
            "midi.tap",
            midi("stagepad"),
            FailurePolicy::Stop,
            &self.registry,
        )
    }

    fn request(
        &self,
        harness: &Harness,
        key: DedupeKey,
        graph: &Arc<ValidatedGraph>,
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
}

fn dispatch_count(harness: &Harness) -> usize {
    harness
        .events
        .snapshot()
        .iter()
        .filter(|event| matches!(event, Event::Dispatch { .. }))
        .count()
}

#[test]
fn unknown_outcome_journals_unknown_and_never_retries() {
    let harness = Harness::new();
    let mut fixture = fixture(&harness, vec![Step::Unknown], false);
    let graph = fixture.graph();

    let receipt = run_ok(&mut fixture.runtime, &harness, &graph);

    assert_eq!(receipt.state.token(), "outcome_unknown");
    // Exactly one dispatch: no automatic retry after an unknown outcome.
    assert_eq!(dispatch_count(&harness), 1);

    // A re-delivery of a DIFFERENT envelope still runs normally (dedupe is
    // keyed per message); the unknown state lives on the original record.
    let second_key = DedupeKey::new(device(), MessageId::generate());
    let second = fixture
        .runtime
        .execute(fixture.request(&harness, second_key, &graph))
        .unwrap();
    assert_eq!(second.state.token(), "outcome_unknown");
    assert_eq!(dispatch_count(&harness), 2);
}

#[test]
fn crash_recovery_scan_marks_orphans_outcome_unknown() {
    let harness = Harness::new();

    // Simulated post-crash journal: prepared without any terminal record.
    let mut journal = MemoryJournal::new();
    let execution = openstream_engine::ExecutionId::generate();
    journal
        .admit(openstream_engine::AdmissionEntry {
            key: DedupeKey::new(device(), MessageId::generate()),
            execution_id: execution,
            accepted_at_wall_ms: WALL_START,
            expires_at_wall_ms: WALL_START + 1_000,
            lifecycle: JournalLifecycle::Running,
        })
        .unwrap();
    journal
        .prepare(PreparedEntry {
            execution_id: execution,
            node_key: node_key("a"),
            attempt: 0,
            action_type: "midi.tap".to_string(),
            idempotency_key: "peer:test-device:m:a:0".to_string(),
            prepared_at_monotonic_ms: 5,
        })
        .unwrap();
    assert_eq!(journal.unresolved_prepared().len(), 1);

    let mut registry = ActionRegistry::new();
    register_action(
        &mut registry,
        "midi.tap",
        vec![midi("stagepad")],
        true,
        false,
        ScriptedPort::new(vec![], Arc::clone(&harness.events), harness.clock.clone()),
    );
    let mut runtime = openstream_engine::RuntimeBuilder::new()
        .clock(harness.clock.clone())
        .time_control(harness.clock.clone() as Arc<dyn TimeControl>)
        .registry(registry)
        .grant_ledger(ledger_with(&[midi("stagepad")]))
        .journal(Box::new(journal))
        .build()
        .unwrap();

    // Corrective unknown evidence closes the crash window exactly once.
    assert_eq!(runtime.recover_outcome_unknown().unwrap(), vec![execution]);
    assert!(runtime.recover_outcome_unknown().unwrap().is_empty());
}

#[test]
fn replay_requires_a_known_dedupe_key() {
    let harness = Harness::new();
    let mut fixture = fixture(&harness, vec![Step::Ok], true);
    let graph = fixture.graph();

    let refusal = fixture.runtime.replay(
        device(),
        MessageId::generate(),
        subject(),
        graph,
        Default::default(),
        harness.expires_at(),
    );
    assert_eq!(refusal.unwrap_err(), ReplayRejection::UnknownKey);
}

#[test]
fn replay_gates_on_declared_idempotency_before_dispatch() {
    let harness = Harness::new();
    let mut fixture = fixture(&harness, vec![Step::Unknown], false);
    let graph = fixture.graph();

    let key = DedupeKey::new(device(), MessageId::generate());
    let stuck = fixture
        .runtime
        .execute(fixture.request(&harness, key.clone(), &graph))
        .unwrap();
    assert_eq!(stuck.state.token(), "outcome_unknown");
    harness.events.clear();

    let refusal = fixture.runtime.replay(
        device(),
        key.message_id,
        subject(),
        Arc::clone(&graph),
        Default::default(),
        harness.expires_at(),
    );
    assert_eq!(
        refusal.unwrap_err(),
        ReplayRejection::RequiresIdempotentAdapters
    );
    assert_eq!(
        dispatch_count(&harness),
        0,
        "refused replays never dispatch"
    );
}

#[test]
fn idempotent_replay_reuses_execution_and_stable_effect_keys() {
    let harness = Harness::new();
    // Idempotent adapter: first attempt loses its result, replay succeeds.
    let mut fixture = fixture(&harness, vec![Step::Unknown, Step::Ok], true);
    let graph = fixture.graph();

    let message = MessageId::generate();
    let stuck = fixture
        .runtime
        .execute(fixture.request(&harness, DedupeKey::new(device(), message), &graph))
        .unwrap();
    assert_eq!(stuck.state.token(), "outcome_unknown");
    harness.events.clear();

    let receipt = fixture
        .runtime
        .replay(
            device(),
            message,
            subject(),
            Arc::clone(&graph),
            Default::default(),
            harness.expires_at(),
        )
        .expect("idempotent replay must be admitted");

    // Corrective terminal links the SAME execution id.
    assert_eq!(receipt.execution_id, stuck.execution_id);
    assert_eq!(receipt.state.token(), "succeeded");

    // Adapter-facing keys stay stable across attempts (source+message
    // derived with node/attempt disambiguation), enabling adapter-side
    // collapse under replay (OSCP_MESSAGES §7).
    let keys: Vec<String> = harness
        .events
        .snapshot()
        .iter()
        .filter_map(|event| match event {
            Event::Dispatch {
                idempotency_key, ..
            } => Some(idempotency_key.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(keys.len(), 1);
    assert!(keys[0].starts_with("peer:test-device:"));
    assert!(keys[0].ends_with(":a:0"));
}

#[test]
fn replay_refuses_when_original_is_not_outcome_unknown() {
    let harness = Harness::new();
    let mut fixture = fixture(&harness, vec![Step::Ok], true);
    let graph = fixture.graph();

    let message = MessageId::generate();
    let done = fixture
        .runtime
        .execute(fixture.request(&harness, DedupeKey::new(device(), message), &graph))
        .unwrap();
    assert_eq!(done.state.token(), "succeeded");

    assert_eq!(
        fixture
            .runtime
            .replay(
                device(),
                message,
                subject(),
                Arc::clone(&graph),
                Default::default(),
                harness.expires_at()
            )
            .unwrap_err(),
        ReplayRejection::NotOutcomeUnknown
    );
}
