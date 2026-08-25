//! Deterministic scheduler contract: bounded nodes/depth/concurrency/time,
//! exact typed outcomes, failure policies, retry with deterministic
//! backoff, conditional/transform semantics, cancellation propagation, and
//! run-to-run determinism. All timing assertions are exact virtual
//! milliseconds on the fake clock; no real sleeps exist anywhere.

mod common;

use common::Step;
use common::*;
use openstream_engine::{
    ActionRegistry, Clock as _, EdgeKindInput, ExecuteRequest, ExecutionReceipt, FailurePolicy,
    MessageId, NodeKind, RawGraph, TerminalState, ValidatedGraph,
    graph::{Condition, ConditionOp, TransformOp},
};
use std::sync::Arc;

fn ok_port(harness: &Harness) -> Arc<ScriptedPort> {
    ScriptedPort::new(vec![], Arc::clone(&harness.events), harness.clock.clone())
}

/// `sequence(a -> b)` of two instant actions.
#[test]
fn sequence_runs_children_in_insertion_order() {
    let harness = Harness::new();
    let mut registry = ActionRegistry::new();
    register_action(
        &mut registry,
        "midi.tap",
        vec![midi("stagepad")],
        false,
        false,
        ok_port(&harness),
    );

    let mut raw = RawGraph::new(FailurePolicy::Stop);
    for key in ["a", "b"] {
        raw.add_node(
            node_key(key),
            NodeKind::Action {
                action_type: "midi.tap".to_string(),
                capability: midi("stagepad"),
                params: serde_json::Value::Null,
                deadline_override_ms: None,
            },
        )
        .unwrap();
    }
    let raw = raw
        .add_edge(node_key("seq"), node_key("a"), EdgeKindInput::Sequence)
        .add_edge(node_key("seq"), node_key("b"), EdgeKindInput::Sequence)
        .add_node(node_key("seq"), NodeKind::Sequence)
        .unwrap()
        .entry(node_key("seq"));
    let graph = Arc::new(ValidatedGraph::build(raw, &registry).unwrap());

    let mut runtime = harness.runtime(registry, ledger_with(&[midi("stagepad")]));
    let receipt = run_ok(&mut runtime, &harness, &graph);

    assert_eq!(receipt.state.token(), "succeeded");
    let snapshot = harness.events.snapshot();
    let order: Vec<&str> = snapshot
        .iter()
        .filter_map(|event| match event {
            Event::Dispatch { node, .. } => Some(node.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(order, vec!["a", "b"]);
}

#[test]
fn delay_nodes_advance_exactly_their_duration() {
    let harness = Harness::new();
    let mut registry = ActionRegistry::new();
    register_action(
        &mut registry,
        "midi.tap",
        vec![midi("stagepad")],
        false,
        false,
        ok_port(&harness),
    );

    let mut raw = RawGraph::new(FailurePolicy::Stop);
    raw.add_node(node_key("a"), action("midi.tap", midi("stagepad")))
        .unwrap();
    raw.add_node(node_key("wait"), NodeKind::Delay { duration_ms: 250 })
        .unwrap();
    raw.add_node(node_key("b"), action("midi.tap", midi("stagepad")))
        .unwrap();
    raw.add_node(node_key("seq"), NodeKind::Sequence).unwrap();
    raw.add_edge(node_key("seq"), node_key("a"), EdgeKindInput::Sequence);
    raw.add_edge(node_key("seq"), node_key("wait"), EdgeKindInput::Sequence);
    raw.add_edge(node_key("seq"), node_key("b"), EdgeKindInput::Sequence);
    raw.entry(node_key("seq"));
    let graph = Arc::new(ValidatedGraph::build(&raw, &registry).unwrap());

    let mut runtime = harness.runtime(registry, ledger_with(&[midi("stagepad")]));
    let receipt = run_ok(&mut runtime, &harness, &graph);
    assert_eq!(receipt.state.token(), "succeeded");

    // Second dispatch lands exactly at the 250 ms virtual mark.
    let stamps: Vec<u64> = harness
        .events
        .snapshot()
        .iter()
        .filter_map(|event| match event {
            Event::Dispatch { at_mono, .. } => Some(*at_mono),
            _ => None,
        })
        .collect();
    assert_eq!(stamps, vec![0, 250]);
}

fn action(action_type: &str, capability: openstream_domain::capability::Capability) -> NodeKind {
    NodeKind::Action {
        action_type: action_type.to_string(),
        capability,
        params: serde_json::Value::Null,
        deadline_override_ms: None,
    }
}

#[test]
fn per_action_concurrency_cap_is_four_and_slots_reuse_in_order() {
    let harness = Harness::new();
    let mut registry = ActionRegistry::new();
    // Six delayed effects (100 ms each) on ONE adapter: cap 4 forces waves.
    let port = IntervalPort::new(100, Arc::clone(&harness.events), harness.clock.clone());
    register_action(
        &mut registry,
        "midi.tap",
        vec![midi("stagepad")],
        false,
        false,
        port.clone(),
    );

    let mut raw = RawGraph::new(FailurePolicy::Stop);
    raw.add_node(node_key("fan"), NodeKind::Parallel).unwrap();
    for index in 0..6 {
        let key = format!("n{index}");
        raw.add_node(node_key(&key), action("midi.tap", midi("stagepad")))
            .unwrap();
        raw.add_edge(node_key("fan"), node_key(&key), EdgeKindInput::Sequence);
    }
    raw.entry(node_key("fan"));
    let graph = Arc::new(ValidatedGraph::build(&raw, &registry).unwrap());

    let mut runtime = harness.runtime(registry, ledger_with(&[midi("stagepad")]));
    let receipt = run_ok(&mut runtime, &harness, &graph);

    assert_eq!(receipt.state.token(), "succeeded");
    assert_eq!(port.interval_count(), 6);
    // Never more than the declared per-adapter cap in flight.
    assert_eq!(
        port.peak_concurrent(),
        openstream_engine::MAX_CONCURRENCY_PER_ACTION,
        "peak in-flight must equal the per-action cap under saturation"
    );
    // Two deterministic waves of four then two, completing at 200 ms.
    assert_eq!(receipt.effects.len(), 6);
    let completion_times: Vec<u64> = receipt
        .effects
        .iter()
        .map(|effect| effect.observed_at_monotonic_ms)
        .collect();
    assert_eq!(completion_times.first().copied(), Some(100));
    assert_eq!(completion_times.last().copied(), Some(200));
}

#[test]
fn execution_deadline_expires_mid_run_and_never_reports_success() {
    let harness = Harness::new();
    let mut registry = ActionRegistry::new();
    register_action(
        &mut registry,
        "midi.tap",
        vec![midi("stagepad")],
        false,
        false,
        ScriptedPort::new(
            vec![Step::Delay(5_000)],
            Arc::clone(&harness.events),
            harness.clock.clone(),
        ),
    );

    let mut raw = RawGraph::new(FailurePolicy::Stop);
    raw.execution_deadline_ms(Some(1_000));
    raw.add_node(node_key("a"), action("midi.tap", midi("stagepad")))
        .unwrap();
    raw.entry(node_key("a"));
    let graph = Arc::new(ValidatedGraph::build(&raw, &registry).unwrap());
    assert_eq!(graph.execution_deadline_ms(), 1_000);

    let mut runtime = harness.runtime(registry, ledger_with(&[midi("stagepad")]));
    let receipt = run_ok(&mut runtime, &harness, &graph);

    // The delayed effect was dispatched; its outcome is recorded honestly,
    // but the terminal state is expired — never success.
    assert_eq!(receipt.state.token(), "expired");
    assert_eq!(receipt.effects.len(), 1);
    assert_eq!(harness.clock.monotonic_ms(), 1_000);
}

#[test]
fn macro_cap_rejects_oversized_deadlines_at_validation() {
    let harness = Harness::new();
    let mut registry = ActionRegistry::new();
    register_action(
        &mut registry,
        "midi.tap",
        vec![midi("stagepad")],
        false,
        false,
        ok_port(&harness),
    );

    let mut raw = RawGraph::new(FailurePolicy::Stop);
    raw.execution_deadline_ms(Some(openstream_engine::MACRO_MAX_DEADLINE_MS + 1));
    raw.add_node(node_key("a"), action("midi.tap", midi("stagepad")))
        .unwrap();
    raw.entry(node_key("a"));

    assert!(matches!(
        ValidatedGraph::build(&raw, &registry),
        Err(openstream_engine::ValidationError::DeadlineOutOfRange { node: None })
    ));
    let _ = harness;
}

#[test]
fn stop_policy_aborts_pending_siblings() {
    let harness = Harness::new();
    let mut registry = ActionRegistry::new();
    let failing = ScriptedPort::new(
        vec![Step::Fail("scene-missing")],
        Arc::clone(&harness.events),
        harness.clock.clone(),
    );
    register_action(
        &mut registry,
        "obs.go",
        vec![notify()],
        false,
        false,
        failing,
    );
    let passing = ScriptedPort::new(vec![], Arc::clone(&harness.events), harness.clock.clone());
    register_action(
        &mut registry,
        "midi.tap",
        vec![midi("stagepad")],
        false,
        false,
        passing,
    );

    let mut raw = RawGraph::new(FailurePolicy::Stop);
    raw.add_node(node_key("boom"), action("obs.go", notify()))
        .unwrap();
    raw.add_node(node_key("after"), action("midi.tap", midi("stagepad")))
        .unwrap();
    raw.add_node(node_key("seq"), NodeKind::Sequence).unwrap();
    raw.add_edge(node_key("seq"), node_key("boom"), EdgeKindInput::Sequence);
    raw.add_edge(node_key("seq"), node_key("after"), EdgeKindInput::Sequence);
    raw.entry(node_key("seq"));
    let graph = Arc::new(ValidatedGraph::build(&raw, &registry).unwrap());

    let mut runtime = harness.runtime(registry, ledger_with(&[notify(), midi("stagepad")]));
    let receipt = run_ok(&mut runtime, &harness, &graph);

    match receipt.state {
        TerminalState::Failed { reason } => {
            assert!(matches!(
                reason,
                openstream_engine::FailureReason::AdapterFailed { code } if code == "scene-missing"
            ));
        }
        other => panic!("expected typed adapter failure, got {other:?}"),
    }
    // Stop policy: the successor never dispatches.
    let snapshot = harness.events.snapshot();
    let nodes: Vec<&str> = snapshot
        .iter()
        .filter_map(|event| match event {
            Event::Dispatch { node, .. } => Some(node.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(nodes, vec!["boom"]);
}

#[test]
fn continue_policy_completes_remaining_work_but_fails_terminal() {
    let harness = Harness::new();
    let mut registry = ActionRegistry::new();
    let failing = ScriptedPort::new(
        vec![Step::Fail("scene-missing")],
        Arc::clone(&harness.events),
        harness.clock.clone(),
    );
    register_action(
        &mut registry,
        "obs.go",
        vec![notify()],
        false,
        false,
        failing,
    );
    register_action(
        &mut registry,
        "midi.tap",
        vec![midi("stagepad")],
        false,
        false,
        ok_port(&harness),
    );

    let mut raw = RawGraph::new(FailurePolicy::Continue);
    raw.add_node(node_key("boom"), action("obs.go", notify()))
        .unwrap();
    raw.add_node(node_key("after"), action("midi.tap", midi("stagepad")))
        .unwrap();
    raw.add_node(node_key("seq"), NodeKind::Sequence).unwrap();
    raw.add_edge(node_key("seq"), node_key("boom"), EdgeKindInput::Sequence);
    raw.add_edge(node_key("seq"), node_key("after"), EdgeKindInput::Sequence);
    raw.entry(node_key("seq"));
    let graph = Arc::new(ValidatedGraph::build(&raw, &registry).unwrap());

    let mut runtime = harness.runtime(registry, ledger_with(&[notify(), midi("stagepad")]));
    let receipt = run_ok(&mut runtime, &harness, &graph);

    assert_eq!(receipt.state.token(), "failed");
    let snapshot = harness.events.snapshot();
    let nodes: Vec<&str> = snapshot
        .iter()
        .filter_map(|event| match event {
            Event::Dispatch { node, .. } => Some(node.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(nodes, vec!["boom", "after"], "continue still runs siblings");
}

#[test]
fn compensate_policy_unwinds_succeeded_effects_in_reverse() {
    let harness = Harness::new();
    let mut registry = ActionRegistry::new();

    // a succeeds instantly; b fails; both adapters declare safe compensation.
    let steps_a = vec![Step::Ok, Step::Ok];
    let port_a = ScriptedPort::new(steps_a, Arc::clone(&harness.events), harness.clock.clone());
    register_action(&mut registry, "act.a", vec![notify()], false, true, port_a);
    let steps_b = vec![Step::Fail("late-failure"), Step::Ok];
    let port_b = ScriptedPort::new(steps_b, Arc::clone(&harness.events), harness.clock.clone());
    register_action(
        &mut registry,
        "act.b",
        vec![midi("stagepad")],
        false,
        true,
        port_b,
    );

    let mut raw = RawGraph::new(FailurePolicy::Compensate);
    raw.add_node(node_key("a"), action("act.a", notify()))
        .unwrap();
    raw.add_node(node_key("ca"), NodeKind::Compensate).unwrap();
    raw.add_node(node_key("b"), action("act.b", midi("stagepad")))
        .unwrap();
    raw.add_node(node_key("cb"), NodeKind::Compensate).unwrap();
    raw.add_node(node_key("seq"), NodeKind::Sequence).unwrap();
    raw.add_edge(node_key("seq"), node_key("a"), EdgeKindInput::Sequence);
    raw.add_edge(node_key("seq"), node_key("b"), EdgeKindInput::Sequence);
    raw.add_edge(
        node_key("a"),
        node_key("ca"),
        EdgeKindInput::CompensationLink,
    );
    raw.add_edge(
        node_key("b"),
        node_key("cb"),
        EdgeKindInput::CompensationLink,
    );
    raw.entry(node_key("seq"));
    let graph = Arc::new(ValidatedGraph::build(&raw, &registry).unwrap());

    let mut runtime = harness.runtime(registry, ledger_with(&[notify(), midi("stagepad")]));
    let receipt = run_ok(&mut runtime, &harness, &graph);

    assert_eq!(receipt.state.token(), "failed");
    let flow: Vec<(String, bool)> = receipt
        .effects
        .iter()
        .map(|effect| (effect.node_key.to_string(), effect.is_compensation))
        .collect();
    // Normal: a, b(failed) → compensation unwinds a only (reverse order).
    assert_eq!(
        flow,
        vec![
            ("a".to_string(), false),
            ("b".to_string(), false),
            ("ca".to_string(), true),
        ]
    );
    // The compensation invocation carried the compensation idempotency key.
    let snapshot = harness.events.snapshot();
    let comp_keys: Vec<&str> = snapshot
        .iter()
        .filter_map(|event| match event {
            Event::Dispatch {
                is_compensation: true,
                idempotency_key,
                ..
            } => Some(idempotency_key.as_str()),
            _ => None,
        })
        .collect();
    assert!(comp_keys[0].ends_with(":compensation"));
}

/// Regression for gate finding F1: under cap saturation, slot waiters hold
/// durable prepared records; the compensation drain must resolve them
/// exactly like `abort_all`, or `recover_outcome_unknown()` later flips the
/// persisted terminal to `outcome_unknown` (one-terminal-state integrity,
/// OSCP_MESSAGES §8).
#[test]
fn compensation_drain_resolves_saturated_slot_waiters_before_recovery() {
    let harness = Harness::new();
    let mut registry = ActionRegistry::new();

    // Six delayed effects on ONE adapter saturate the per-action cap of 4:
    // n0..n3 dispatch immediately, n4/n5 park as slot waiters holding
    // prepared records. n3 fails late; Compensate policy drains.
    let port = ScriptedPort::new(
        vec![
            Step::Delay(50),
            Step::Delay(50),
            Step::Delay(50),
            Step::DelayFail(50, "boom"),
            Step::Delay(50),
            Step::Delay(50),
            Step::Delay(50),
        ],
        Arc::clone(&harness.events),
        harness.clock.clone(),
    );
    register_action(
        &mut registry,
        "act.x",
        vec![midi("stagepad")],
        false,
        true, // safe-compensation declared
        port,
    );

    let mut raw = RawGraph::new(FailurePolicy::Compensate);
    raw.add_node(node_key("fan"), NodeKind::Parallel).unwrap();
    for index in 0..6 {
        let key = format!("n{index}");
        raw.add_node(node_key(&key), action("act.x", midi("stagepad")))
            .unwrap();
        raw.add_edge(node_key("fan"), node_key(&key), EdgeKindInput::Sequence);
        // Every action carries its dedicated compensate node + link.
        let comp_key = format!("c{index}");
        raw.add_node(node_key(&comp_key), NodeKind::Compensate)
            .unwrap();
        raw.add_edge(
            node_key(&key),
            node_key(&comp_key),
            EdgeKindInput::CompensationLink,
        );
    }
    raw.entry(node_key("fan"));
    let graph = Arc::new(ValidatedGraph::build(&raw, &registry).unwrap());

    // Instrumented journal so persisted lifecycle transitions are visible.
    let mut runtime = harness.runtime_with_journal(
        registry,
        ledger_with(&[midi("stagepad")]),
        InstrumentedJournal::new(Arc::clone(&harness.events), harness.clock.clone()),
    );
    let receipt = run_ok(&mut runtime, &harness, &graph);

    // Terminal stays the typed original failure — not outcome_unknown.
    assert_eq!(receipt.state.token(), "failed");
    match receipt.state {
        TerminalState::Failed {
            reason: openstream_engine::FailureReason::AdapterFailed { ref code },
        } => assert_eq!(code, "boom"),
        other => panic!("expected adapter failure, got {other:?}"),
    }

    // Saturation held: only the first wave dispatched (4), the waiters
    // never did; three succeeded effects were compensated in reverse.
    let snapshot = harness.events.snapshot();
    let dispatched: Vec<&str> = snapshot
        .iter()
        .filter_map(|event| match event {
            Event::Dispatch { node, .. } => Some(node.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(dispatched.len(), 7, "4 first-wave + 3 compensations");
    assert!(!dispatched.iter().any(|node| *node == "n4" || *node == "n5"));
    let comps: Vec<String> = receipt
        .effects
        .iter()
        .filter(|effect| effect.is_compensation)
        .map(|effect| effect.node_key.to_string())
        .collect();
    assert_eq!(
        comps,
        vec!["c2".to_string(), "c1".to_string(), "c0".to_string()],
        "reverse completion order unwind"
    );

    // THE REGRESSION: recovery must find nothing to flip. Before the fix
    // the drained waiters' prepared records surfaced here as
    // outcome_unknown and overwrote the persisted `failed` terminal.
    assert_eq!(
        runtime.recover_outcome_unknown().unwrap(),
        Vec::<openstream_engine::ExecutionId>::new(),
        "drained slot waiters must not leave orphan prepared records"
    );
    let lifecycle_tokens: Vec<&str> = snapshot
        .iter()
        .filter_map(|event| match event {
            Event::Lifecycle { lifecycle, .. } => Some(*lifecycle),
            _ => None,
        })
        .collect();
    assert_eq!(
        lifecycle_tokens
            .iter()
            .filter(|token| **token == "outcome_unknown")
            .count(),
        0,
        "no corrective unknown evidence may exist for a failed execution"
    );
    assert_eq!(
        lifecycle_tokens
            .iter()
            .filter(|token| **token == "failed")
            .count(),
        1,
        "exactly one persisted terminal state"
    );
}

#[test]
fn compensate_policy_requires_links_and_safe_declaration() {
    let harness = Harness::new();
    let mut registry = ActionRegistry::new();
    // Adapter does NOT declare safe compensation.
    register_action(
        &mut registry,
        "act.a",
        vec![notify()],
        false,
        false,
        ok_port(&harness),
    );

    let mut raw = RawGraph::new(FailurePolicy::Compensate);
    raw.add_node(node_key("a"), action("act.a", notify()))
        .unwrap();
    raw.add_node(node_key("ca"), NodeKind::Compensate).unwrap();
    let raw = raw
        .add_edge(
            node_key("a"),
            node_key("ca"),
            EdgeKindInput::CompensationLink,
        )
        .entry(node_key("a"));

    let error = ValidatedGraph::build(raw, &registry).unwrap_err();
    assert!(matches!(
        error,
        openstream_engine::ValidationError::PolicyCompensateInvalid { .. }
    ));
    let _ = std::hint::black_box(harness);
}

#[test]
fn retry_requires_declared_idempotency_at_validation() {
    let harness = Harness::new();
    let mut registry = ActionRegistry::new();
    register_action(
        &mut registry,
        "midi.tap",
        vec![midi("stagepad")],
        false, // non-idempotent
        false,
        ok_port(&harness),
    );

    let mut raw = RawGraph::new(FailurePolicy::Stop);
    raw.add_node(node_key("retry"), NodeKind::Retry { attempts: 3 })
        .unwrap();
    raw.add_node(node_key("body"), action("midi.tap", midi("stagepad")))
        .unwrap();
    let raw = raw
        .add_edge(node_key("retry"), node_key("body"), EdgeKindInput::Sequence)
        .entry(node_key("retry"));

    assert!(matches!(
        ValidatedGraph::build(raw, &registry),
        Err(openstream_engine::ValidationError::RetryRequiresIdempotency { .. })
    ));
    let _ = std::hint::black_box(harness);
}

#[test]
fn retry_retries_until_success_with_deterministic_backoff() {
    let harness = Harness::new();
    let mut registry = ActionRegistry::new();
    // Fail twice with distinct codes, then succeed.
    let port = ScriptedPort::new(
        vec![Step::Fail("busy-1"), Step::Fail("busy-2"), Step::Ok],
        Arc::clone(&harness.events),
        harness.clock.clone(),
    );
    register_action(
        &mut registry,
        "midi.tap",
        vec![midi("stagepad")],
        true, // retry requires this declaration
        false,
        port,
    );

    let mut raw = RawGraph::new(FailurePolicy::Stop);
    raw.add_node(node_key("retry"), NodeKind::Retry { attempts: 4 })
        .unwrap();
    raw.add_node(node_key("body"), action("midi.tap", midi("stagepad")))
        .unwrap();
    let raw = raw
        .add_edge(node_key("retry"), node_key("body"), EdgeKindInput::Sequence)
        .entry(node_key("retry"));
    let graph = Arc::new(ValidatedGraph::build(raw, &registry).unwrap());

    let mut runtime = harness.runtime(registry, ledger_with(&[midi("stagepad")]));
    let receipt = run_ok(&mut runtime, &harness, &graph);

    assert_eq!(receipt.state.token(), "succeeded");
    let stamps: Vec<u64> = harness
        .events
        .snapshot()
        .iter()
        .filter_map(|event| match event {
            Event::Dispatch {
                at_mono, attempt, ..
            } => {
                // Attempts are zero-based and strictly ordered.
                let _ = attempt;
                Some(*at_mono)
            }
            _ => None,
        })
        .collect();
    // Backoff: 50 ms after attempt 0, 100 ms after attempt 1.
    assert_eq!(stamps, vec![0, 50, 150]);
    // Attempt counters recorded in evidence.
    let attempts: Vec<u32> = receipt
        .effects
        .iter()
        .map(|effect| effect.attempt)
        .collect();
    assert_eq!(attempts, vec![0, 1, 2]);
}

#[test]
fn exhausted_retry_fails_with_last_adapter_code() {
    let harness = Harness::new();
    let mut registry = ActionRegistry::new();
    let port = ScriptedPort::new(
        vec![Step::Fail("still-busy")],
        Arc::clone(&harness.events),
        harness.clock.clone(),
    );
    register_action(
        &mut registry,
        "midi.tap",
        vec![midi("stagepad")],
        true,
        false,
        port,
    );

    let mut raw = RawGraph::new(FailurePolicy::Stop);
    raw.add_node(node_key("retry"), NodeKind::Retry { attempts: 2 })
        .unwrap();
    raw.add_node(node_key("body"), action("midi.tap", midi("stagepad")))
        .unwrap();
    let raw = raw
        .add_edge(node_key("retry"), node_key("body"), EdgeKindInput::Sequence)
        .entry(node_key("retry"));
    let graph = Arc::new(ValidatedGraph::build(raw, &registry).unwrap());

    let mut runtime = harness.runtime(registry, ledger_with(&[midi("stagepad")]));
    let receipt = run_ok(&mut runtime, &harness, &graph);

    match receipt.state {
        TerminalState::Failed { reason } => match reason {
            openstream_engine::FailureReason::AdapterFailed { code } => {
                assert_eq!(code, "still-busy");
            }
            other => panic!("expected adapter failure, got {other:?}"),
        },
        other => panic!("expected failed, got {other:?}"),
    }
    assert_eq!(dispatch_count_total(&harness), 2);
}

fn dispatch_count_total(harness: &Harness) -> usize {
    harness
        .events
        .snapshot()
        .iter()
        .filter(|event| matches!(event, Event::Dispatch { .. }))
        .count()
}

#[test]
fn conditional_routes_on_variables_and_transforms_mutate_them() {
    let harness = Harness::new();
    let mut registry = ActionRegistry::new();
    register_action(
        &mut registry,
        "midi.tap",
        vec![midi("stagepad")],
        false,
        false,
        ok_port(&harness),
    );
    register_action(
        &mut registry,
        "obs.go",
        vec![notify()],
        false,
        false,
        ok_port(&harness),
    );

    let mut raw = RawGraph::new(FailurePolicy::Stop);
    raw.add_node(
        node_key("setup"),
        NodeKind::VariableTransform {
            op: TransformOp::Set {
                variable: "mode".to_string(),
                value: serde_json::json!("midi"),
            },
        },
    )
    .unwrap();
    raw.add_node(
        node_key("branch"),
        NodeKind::Conditional {
            condition: Condition {
                variable: "mode".to_string(),
                op: ConditionOp::Equals,
                operand: serde_json::json!("midi"),
            },
        },
    )
    .unwrap();
    raw.add_node(node_key("true_arm"), action("midi.tap", midi("stagepad")))
        .unwrap();
    raw.add_node(node_key("false_arm"), action("obs.go", notify()))
        .unwrap();
    raw.add_node(
        node_key("count"),
        NodeKind::VariableTransform {
            op: TransformOp::AddInt {
                variable: "runs".to_string(),
                delta: 1,
            },
        },
    )
    .unwrap();
    // Truth arm wraps its follow-up work so both live under the branch.
    raw.add_node(node_key("tseq"), NodeKind::Sequence).unwrap();
    raw.add_node(node_key("seq"), NodeKind::Sequence).unwrap();

    raw.add_edge(node_key("seq"), node_key("setup"), EdgeKindInput::Sequence);
    raw.add_edge(node_key("seq"), node_key("branch"), EdgeKindInput::Sequence);
    raw.add_edge(
        node_key("branch"),
        node_key("tseq"),
        EdgeKindInput::Branch { polarity: true },
    );
    raw.add_edge(
        node_key("branch"),
        node_key("false_arm"),
        EdgeKindInput::Branch { polarity: false },
    );
    raw.add_edge(
        node_key("tseq"),
        node_key("true_arm"),
        EdgeKindInput::Sequence,
    );
    raw.add_edge(node_key("tseq"), node_key("count"), EdgeKindInput::Sequence);
    raw.entry(node_key("seq"));
    let graph = Arc::new(ValidatedGraph::build(&raw, &registry).unwrap());

    let request = ExecuteRequest {
        source_device_id: device(),
        message_id: MessageId::generate(),
        subject: subject(),
        graph: Arc::clone(&graph),
        variables: [("runs".to_string(), serde_json::json!(0))]
            .into_iter()
            .collect(),
        expires_at_wall_ms: harness.expires_at(),
        cancel: None::<openstream_engine::CancelSignal>,
    };

    let mut runtime = harness.runtime(registry, ledger_with(&[midi("stagepad"), notify()]));
    let receipt = runtime.execute(request).unwrap();

    assert_eq!(receipt.state.token(), "succeeded");
    // Truth arm taken; fall-through arm skipped.
    assert_eq!(receipt.effects.len(), 1);
    assert_eq!(receipt.effects[0].node_key.to_string(), "true_arm");
    assert_eq!(receipt.variables.get("runs"), Some(&serde_json::json!(1)));
    assert_eq!(
        receipt.variables.get("mode"),
        Some(&serde_json::json!("midi"))
    );
}

#[test]
fn cancellation_propagates_across_parallel_branches() {
    let harness = Harness::new();
    let mut registry = ActionRegistry::new();
    let slow = ScriptedPort::new(
        vec![Step::Delay(10_000)],
        Arc::clone(&harness.events),
        harness.clock.clone(),
    );
    register_action(
        &mut registry,
        "slow.act",
        vec![midi("stagepad")],
        false,
        false,
        slow,
    );
    register_action(
        &mut registry,
        "quick.act",
        vec![notify()],
        false,
        false,
        ok_port(&harness),
    );

    let mut raw = RawGraph::new(FailurePolicy::Stop);
    raw.add_node(node_key("fan"), NodeKind::Parallel).unwrap();
    raw.add_node(node_key("slow"), action("slow.act", midi("stagepad")))
        .unwrap();
    raw.add_node(node_key("fast"), action("quick.act", notify()))
        .unwrap();
    let raw = raw
        .add_edge(node_key("fan"), node_key("slow"), EdgeKindInput::Sequence)
        .add_edge(node_key("fan"), node_key("fast"), EdgeKindInput::Sequence)
        .entry(node_key("fan"));
    let graph = Arc::new(ValidatedGraph::build(raw, &registry).unwrap());

    let cancel = openstream_engine::CancelSignal::new();
    let request = ExecuteRequest {
        source_device_id: device(),
        message_id: MessageId::generate(),
        subject: subject(),
        graph: Arc::clone(&graph),
        variables: Default::default(),
        expires_at_wall_ms: harness.expires_at(),
        cancel: Some(cancel.clone()),
    };

    let mut runtime = harness.runtime(registry, ledger_with(&[midi("stagepad"), notify()]));
    let mut handle = runtime.begin(request).unwrap();

    // Drive until both branches dispatched, then cancel mid-flight.
    while dispatch_count_total(&harness) < 2 && !handle.is_terminal() {
        if !handle.step() {
            handle.step();
            break;
        }
    }
    cancel.cancel();
    while !handle.is_terminal() {
        if !handle.step() {
            break;
        }
    }
    let receipt = handle
        .run_to_completion()
        .unwrap_or_else(|error| panic!("cancellation run must reach terminal: {error}"));

    assert_eq!(receipt.state.token(), "cancelled");
    assert_eq!(dispatch_count_total(&harness), 2);
}

#[test]
fn identical_inputs_produce_identical_receipts_modulo_ids() {
    let build = |harness: &Harness| {
        let mut registry = ActionRegistry::new();
        let port = ScriptedPort::new(
            vec![Step::Delay(40)],
            Arc::clone(&harness.events),
            harness.clock.clone(),
        );
        register_action(
            &mut registry,
            "midi.tap",
            vec![midi("stagepad")],
            false,
            false,
            port,
        );
        let mut raw = RawGraph::new(FailurePolicy::Continue);
        raw.add_node(node_key("a"), action("midi.tap", midi("stagepad")))
            .unwrap();
        raw.add_node(node_key("b"), action("midi.tap", midi("stagepad")))
            .unwrap();
        let raw = raw
            .add_edge(node_key("seq"), node_key("a"), EdgeKindInput::Sequence)
            .add_edge(node_key("seq"), node_key("b"), EdgeKindInput::Sequence)
            .add_node(node_key("seq"), NodeKind::Sequence)
            .unwrap()
            .entry(node_key("seq"));
        let graph = Arc::new(ValidatedGraph::build(raw, &registry).unwrap());
        (registry, graph)
    };

    let receipts: Vec<ExecutionReceipt> = (0..2)
        .map(|_| {
            let harness = Harness::new();
            let (registry, _) = build(&harness);
            // Graph rebuilt identically each iteration.
            let (_, graph) = build(&harness);
            let mut runtime = harness.runtime(registry, ledger_with(&[midi("stagepad")]));
            run_ok(&mut runtime, &harness, &graph)
        })
        .collect();

    let strip = |receipt: &ExecutionReceipt| {
        (
            receipt.state.token().to_string(),
            receipt
                .effects
                .iter()
                .map(|effect| {
                    (
                        effect.node_key.to_string(),
                        effect.attempt,
                        effect.outcome.clone(),
                        effect.observed_at_monotonic_ms,
                    )
                })
                .collect::<Vec<_>>(),
            receipt.variables.clone(),
        )
    };
    let (first, second) = (&receipts[0], &receipts[1]);
    assert_eq!(strip(first), strip(second));
    assert_ne!(first.execution_id, second.execution_id);
}

// ---------------------------------------------------------------------------
// Issue #14: multi-action graph semantics — residual-work settlement,
// bounded retry attempts, cancellation during sleeps, deadline-vs-backoff,
// conditional fall-through.
// ---------------------------------------------------------------------------

/// Regression for issue #14 (one-terminal-state integrity, OSCP_MESSAGES
/// §8): a `stop`-policy failure firing mid-pass while a sibling branch
/// still holds an in-flight delayed effect must settle that effect exactly
/// once. Before the fix the sibling's prepared record stayed open and
/// `recover_outcome_unknown()` later flipped the persisted `failed`
/// terminal to `outcome_unknown`.
#[test]
fn stop_policy_with_parallel_inflight_settles_evidence_and_recovery_finds_nothing() {
    let harness = Harness::new();
    let mut registry = ActionRegistry::new();
    // Invocation order is branch insertion order: n_slow takes the
    // delayed-success step, n_boom the immediate failure.
    register_action(
        &mut registry,
        "act.x",
        vec![midi("stagepad")],
        false,
        false,
        ScriptedPort::new(
            vec![Step::Delay(5_000), Step::Fail("boom")],
            Arc::clone(&harness.events),
            harness.clock.clone(),
        ),
    );

    let mut raw = RawGraph::new(FailurePolicy::Stop);
    raw.add_node(node_key("fan"), NodeKind::Parallel).unwrap();
    raw.add_node(node_key("n_slow"), action("act.x", midi("stagepad")))
        .unwrap();
    raw.add_node(node_key("n_boom"), action("act.x", midi("stagepad")))
        .unwrap();
    let raw = raw
        .add_edge(node_key("fan"), node_key("n_slow"), EdgeKindInput::Sequence)
        .add_edge(node_key("fan"), node_key("n_boom"), EdgeKindInput::Sequence)
        .entry(node_key("fan"));
    let graph = Arc::new(ValidatedGraph::build(raw, &registry).unwrap());

    let mut runtime = harness.runtime_with_journal(
        registry,
        ledger_with(&[midi("stagepad")]),
        InstrumentedJournal::new(Arc::clone(&harness.events), harness.clock.clone()),
    );
    let receipt = run_ok(&mut runtime, &harness, &graph);

    match receipt.state {
        TerminalState::Failed {
            reason: openstream_engine::FailureReason::AdapterFailed { ref code },
        } => assert_eq!(code, "boom"),
        other => panic!("expected typed adapter failure, got {other:?}"),
    }
    // The abandoned in-flight effect settles honestly into evidence even
    // though the execution terminated failed (no success inference, no
    // dropped prepared record).
    let flow: Vec<(String, String)> = receipt
        .effects
        .iter()
        .map(|effect| (effect.node_key.to_string(), effect.outcome.clone()))
        .collect();
    assert_eq!(
        flow,
        vec![
            ("n_boom".to_string(), "failed".to_string()),
            ("n_slow".to_string(), "succeeded".to_string()),
        ]
    );

    // THE REGRESSION: recovery must find nothing to flip.
    assert_eq!(
        runtime.recover_outcome_unknown().unwrap(),
        Vec::<openstream_engine::ExecutionId>::new(),
        "settled siblings must not surface as crash-gap orphans"
    );
    let lifecycle_tokens: Vec<&str> = harness
        .events
        .snapshot()
        .iter()
        .filter_map(|event| match event {
            Event::Lifecycle { lifecycle, .. } => Some(*lifecycle),
            _ => None,
        })
        .collect();
    assert_eq!(
        lifecycle_tokens
            .iter()
            .filter(|token| **token == "outcome_unknown")
            .count(),
        0,
        "no corrective unknown evidence may exist for a failed execution"
    );
    assert_eq!(
        lifecycle_tokens
            .iter()
            .filter(|token| **token == "failed")
            .count(),
        1,
        "exactly one persisted terminal state"
    );
}

/// Same integrity contract when the mid-pass terminal is an unknown
/// outcome instead of a stop-policy failure: the surviving sibling
/// settles honestly and recovery finds no orphan.
#[test]
fn unknown_outcome_with_parallel_inflight_still_settles_siblings() {
    let harness = Harness::new();
    let mut registry = ActionRegistry::new();
    register_action(
        &mut registry,
        "act.x",
        vec![midi("stagepad")],
        true,
        false,
        ScriptedPort::new(
            vec![Step::Delay(5_000), Step::Unknown],
            Arc::clone(&harness.events),
            harness.clock.clone(),
        ),
    );

    let mut raw = RawGraph::new(FailurePolicy::Stop);
    raw.add_node(node_key("fan"), NodeKind::Parallel).unwrap();
    raw.add_node(node_key("n_slow"), action("act.x", midi("stagepad")))
        .unwrap();
    raw.add_node(node_key("n_lost"), action("act.x", midi("stagepad")))
        .unwrap();
    let raw = raw
        .add_edge(node_key("fan"), node_key("n_slow"), EdgeKindInput::Sequence)
        .add_edge(node_key("fan"), node_key("n_lost"), EdgeKindInput::Sequence)
        .entry(node_key("fan"));
    let graph = Arc::new(ValidatedGraph::build(raw, &registry).unwrap());

    let mut runtime = harness.runtime_with_journal(
        registry,
        ledger_with(&[midi("stagepad")]),
        InstrumentedJournal::new(Arc::clone(&harness.events), harness.clock.clone()),
    );
    let receipt = run_ok(&mut runtime, &harness, &graph);

    assert_eq!(receipt.state.token(), "outcome_unknown");
    let outcomes: Vec<(String, String)> = receipt
        .effects
        .iter()
        .map(|effect| (effect.node_key.to_string(), effect.outcome.clone()))
        .collect();
    assert_eq!(
        outcomes,
        vec![
            ("n_lost".to_string(), "unknown".to_string()),
            ("n_slow".to_string(), "succeeded".to_string()),
        ],
        "the lost outcome stays unknown and the sibling settles honestly"
    );
    assert_eq!(
        runtime.recover_outcome_unknown().unwrap(),
        Vec::<openstream_engine::ExecutionId>::new(),
        "settled siblings must not surface as crash-gap orphans"
    );
}

#[test]
fn retry_attempts_above_cap_reject_at_validation() {
    let harness = Harness::new();
    let mut registry = ActionRegistry::new();
    register_action(
        &mut registry,
        "midi.tap",
        vec![midi("stagepad")],
        true,
        false,
        ok_port(&harness),
    );

    let mut raw = RawGraph::new(FailurePolicy::Stop);
    raw.add_node(
        node_key("retry"),
        NodeKind::Retry {
            attempts: openstream_engine::MAX_RETRY_ATTEMPTS + 1,
        },
    )
    .unwrap();
    raw.add_node(node_key("body"), action("midi.tap", midi("stagepad")))
        .unwrap();
    let raw = raw
        .add_edge(node_key("retry"), node_key("body"), EdgeKindInput::Sequence)
        .entry(node_key("retry"));

    assert!(matches!(
        ValidatedGraph::build(raw, &registry),
        Err(openstream_engine::ValidationError::MalformedRetry { .. })
    ));
    let _ = std::hint::black_box(harness);
}

#[test]
fn cancellation_during_delay_sleep_aborts_pending_tail() {
    let harness = Harness::new();
    let mut registry = ActionRegistry::new();
    register_action(
        &mut registry,
        "midi.tap",
        vec![midi("stagepad")],
        false,
        false,
        ok_port(&harness),
    );

    let mut raw = RawGraph::new(FailurePolicy::Stop);
    raw.add_node(node_key("a"), action("midi.tap", midi("stagepad")))
        .unwrap();
    raw.add_node(node_key("wait"), NodeKind::Delay { duration_ms: 5_000 })
        .unwrap();
    raw.add_node(node_key("b"), action("midi.tap", midi("stagepad")))
        .unwrap();
    raw.add_node(node_key("seq"), NodeKind::Sequence).unwrap();
    for to in ["a", "wait", "b"] {
        raw.add_edge(node_key("seq"), node_key(to), EdgeKindInput::Sequence);
    }
    raw.entry(node_key("seq"));
    let graph = Arc::new(ValidatedGraph::build(&raw, &registry).unwrap());

    let mut runtime = harness.runtime(registry, ledger_with(&[midi("stagepad")]));
    let request = ExecuteRequest {
        source_device_id: device(),
        message_id: MessageId::generate(),
        subject: subject(),
        graph: Arc::clone(&graph),
        variables: Default::default(),
        expires_at_wall_ms: harness.expires_at(),
        cancel: None::<openstream_engine::CancelSignal>,
    };
    let mut handle = runtime.begin(request).unwrap();

    // One pass completes `a` and parks the branch inside the delay sleep.
    assert!(handle.step());
    handle.cancel();

    let receipt = handle.run_to_completion().expect("terminal reached");
    assert_eq!(receipt.state.token(), "cancelled");
    let snapshot = harness.events.snapshot();
    let dispatched: Vec<&str> = snapshot
        .iter()
        .filter_map(|event| match event {
            Event::Dispatch { node, .. } => Some(node.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(dispatched, vec!["a"], "pending tail never dispatches");
}

#[test]
fn execution_deadline_during_retry_backoff_expires_without_more_attempts() {
    let harness = Harness::new();
    let mut registry = ActionRegistry::new();
    register_action(
        &mut registry,
        "midi.tap",
        vec![midi("stagepad")],
        true,
        false,
        ScriptedPort::new(
            vec![Step::Fail("busy")],
            Arc::clone(&harness.events),
            harness.clock.clone(),
        ),
    );

    let mut raw = RawGraph::new(FailurePolicy::Stop);
    raw.execution_deadline_ms(Some(20));
    raw.add_node(node_key("retry"), NodeKind::Retry { attempts: 3 })
        .unwrap();
    raw.add_node(node_key("body"), action("midi.tap", midi("stagepad")))
        .unwrap();
    let raw = raw
        .add_edge(node_key("retry"), node_key("body"), EdgeKindInput::Sequence)
        .entry(node_key("retry"));
    let graph = Arc::new(ValidatedGraph::build(raw, &registry).unwrap());

    let mut runtime = harness.runtime(registry, ledger_with(&[midi("stagepad")]));
    let receipt = run_ok(&mut runtime, &harness, &graph);

    // The 50 ms backoff sleeps past the 20 ms deadline; the scheduler
    // wakes exactly at the deadline and expires instead of attempting
    // again (bounded time beats unbounded persistence).
    assert_eq!(receipt.state.token(), "expired");
    assert_eq!(harness.clock.monotonic_ms(), 20);
    assert_eq!(dispatch_count_total(&harness), 1);
    let attempts: Vec<u32> = receipt
        .effects
        .iter()
        .map(|effect| effect.attempt)
        .collect();
    assert_eq!(attempts, vec![0]);
}

#[test]
fn conditional_false_without_fallthrough_completes_past_the_condition() {
    let harness = Harness::new();
    let mut registry = ActionRegistry::new();
    register_action(
        &mut registry,
        "midi.tap",
        vec![midi("stagepad")],
        false,
        false,
        ok_port(&harness),
    );

    let mut raw = RawGraph::new(FailurePolicy::Stop);
    raw.add_node(
        node_key("branch"),
        NodeKind::Conditional {
            condition: Condition {
                variable: "mode".to_string(),
                op: ConditionOp::Equals,
                operand: serde_json::json!("go"),
            },
        },
    )
    .unwrap();
    raw.add_node(node_key("truth_arm"), action("midi.tap", midi("stagepad")))
        .unwrap();
    let raw = raw
        .add_edge(
            node_key("branch"),
            node_key("truth_arm"),
            EdgeKindInput::Branch { polarity: true },
        )
        .entry(node_key("branch"));
    let graph = Arc::new(ValidatedGraph::build(raw, &registry).unwrap());

    let mut runtime = harness.runtime(registry, ledger_with(&[midi("stagepad")]));
    let receipt = run_ok(&mut runtime, &harness, &graph);

    // No fall-through arm authored and the condition is false: the
    // conditional completes silently; the truth arm never dispatches.
    assert_eq!(receipt.state.token(), "succeeded");
    assert!(receipt.effects.is_empty());
}
