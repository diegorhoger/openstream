//! Grant enforcement before dispatch (taxonomy §2, S5) and registry
//! capability-scope declarations gating graph validation (S3).

mod common;

use common::*;
use openstream_engine::{ActionRegistry, Clock as _, ConfigError, FailurePolicy, RawGraph};
use std::sync::Arc;

#[test]
fn empty_ledger_denies_before_any_dispatch() {
    let harness = Harness::new();
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
    let graph = single_action_graph("midi.tap", midi("stagepad"), FailurePolicy::Stop, &registry);

    // Deny-by-default: zero grants recorded.
    let mut runtime = harness.runtime(registry, ledger_with(&[]));
    let receipt = run_ok(&mut runtime, &harness, &graph);

    assert_eq!(receipt.state.token(), "failed");
    match receipt.state {
        openstream_engine::TerminalState::Failed { reason } => {
            assert!(matches!(
                reason,
                openstream_engine::FailureReason::CapabilityDenied(
                    openstream_domain::grant::DenialReason::NoActiveGrant
                )
            ));
        }
        other => panic!("expected failed terminal, got {other:?}"),
    }
    // The denial fired BEFORE dispatch: the adapter never saw the effect.
    assert!(
        !harness
            .events
            .snapshot()
            .iter()
            .any(|event| matches!(event, Event::Dispatch { .. }))
    );
}

#[test]
fn wrong_qualifier_scope_denies_like_a_missing_grant() {
    let harness = Harness::new();
    let mut registry = ActionRegistry::new();
    register_action(
        &mut registry,
        "midi.tap",
        vec![midi("stagepad")],
        false,
        false,
        ScriptedPort::new(vec![], Arc::clone(&harness.events), harness.clock.clone()),
    );
    let graph = single_action_graph("midi.tap", midi("stagepad"), FailurePolicy::Stop, &registry);

    // Grant exists but covers a different device value only.
    let mut runtime = harness.runtime(registry, ledger_with(&[midi("backup")]));

    let receipt = run_ok(&mut runtime, &harness, &graph);
    assert_eq!(receipt.state.token(), "failed");
    assert!(
        !harness
            .events
            .snapshot()
            .iter()
            .any(|event| matches!(event, Event::Dispatch { .. }))
    );
}

#[test]
fn revocation_applies_at_the_next_execution_without_restart() {
    let harness = Harness::new();
    let mut registry = ActionRegistry::new();
    register_action(
        &mut registry,
        "midi.tap",
        vec![midi("stagepad")],
        false,
        false,
        ScriptedPort::new(vec![], Arc::clone(&harness.events), harness.clock.clone()),
    );
    let graph = single_action_graph("midi.tap", midi("stagepad"), FailurePolicy::Stop, &registry);

    let ledger = ledger_with(&[midi("stagepad")]);
    let mut runtime = harness.runtime(registry, Arc::clone(&ledger));

    // Granted run dispatches.
    let first = run_ok(&mut runtime, &harness, &graph);
    assert_eq!(first.state.token(), "succeeded");
    assert_eq!(dispatches(&harness), 1);

    // Revoke every grant of the subject; the very next execution denies.
    let revoked = {
        let mut guard = ledger
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.revoke_all(harness.clock.wall_now_ms()).unwrap()
    };
    assert_eq!(revoked, 1);

    let second = run_ok(&mut runtime, &harness, &graph);
    assert_eq!(second.state.token(), "failed");
    assert_eq!(dispatches(&harness), 1, "revocation must block dispatch");
}

fn dispatches(harness: &Harness) -> usize {
    harness
        .events
        .snapshot()
        .iter()
        .filter(|event| matches!(event, Event::Dispatch { .. }))
        .count()
}

#[test]
fn undeclared_capability_scope_rejects_validation() {
    let harness = Harness::new();
    let mut registry = ActionRegistry::new();
    register_action(
        &mut registry,
        "midi.tap",
        vec![midi("stagepad")],
        false,
        false,
        ScriptedPort::new(vec![], Arc::clone(&harness.events), harness.clock.clone()),
    );

    // The node requests a capability the registration never declared.
    let mut raw = RawGraph::new(FailurePolicy::Stop);
    raw.add_node(
        node_key("a"),
        openstream_engine::NodeKind::Action {
            action_type: "midi.tap".to_string(),
            capability: midi("undevice"),
            params: serde_json::Value::Null,
            deadline_override_ms: None,
        },
    )
    .unwrap();
    let raw = raw.entry(node_key("a"));

    let error = openstream_engine::ValidatedGraph::build(raw, &registry).unwrap_err();
    assert!(
        matches!(
            error,
            openstream_engine::ValidationError::CapabilityNotDeclared { .. }
        ),
        "expected S3 scope rejection, got {error}"
    );
}

#[test]
fn unknown_action_type_rejects_validation() {
    let registry = ActionRegistry::new();

    let mut raw = RawGraph::new(FailurePolicy::Stop);
    raw.add_node(
        node_key("a"),
        openstream_engine::NodeKind::Action {
            action_type: "never.registered".to_string(),
            capability: notify(),
            params: serde_json::Value::Null,
            deadline_override_ms: None,
        },
    )
    .unwrap();
    let raw = raw.entry(node_key("a"));

    let error = openstream_engine::ValidatedGraph::build(raw, &registry).unwrap_err();
    assert!(matches!(
        error,
        openstream_engine::ValidationError::UnknownActionType { .. }
    ));
}

#[test]
fn internal_capabilities_never_register_as_scopes() {
    let harness = Harness::new();
    let secret_read = openstream_domain::capability::Capability::SecretRead {
        secret_ref: openstream_domain::secret::SecretRef::try_new("obs.scene.notes").unwrap(),
    };
    let error = openstream_engine::ActionRegistration::try_new(
        "secret.scraper",
        vec![secret_read],
        openstream_engine::IdempotencyClass::NonIdempotent,
        false,
        ScriptedPort::new(vec![], Arc::clone(&harness.events), harness.clock.clone()),
    )
    .unwrap_err();
    assert_eq!(error, ConfigError::InternalCapabilityScope);
}

#[test]
fn duplicate_and_invalid_registrations_fail_closed() {
    let harness = Harness::new();
    let mut registry = ActionRegistry::new();
    let port = ScriptedPort::new(vec![], Arc::clone(&harness.events), harness.clock.clone());
    register_action(
        &mut registry,
        "midi.tap",
        vec![midi("a")],
        false,
        false,
        port.clone(),
    );
    let duplicate = openstream_engine::ActionRegistration::try_new(
        "midi.tap",
        vec![midi("b")],
        openstream_engine::IdempotencyClass::NonIdempotent,
        false,
        port,
    )
    .unwrap();
    assert_eq!(
        registry.register(duplicate).unwrap_err(),
        ConfigError::DuplicateActionName
    );

    let bad_name = openstream_engine::ActionRegistration::try_new(
        "Bad Name!",
        vec![midi("a")],
        openstream_engine::IdempotencyClass::NonIdempotent,
        false,
        ScriptedPort::new(vec![], Arc::clone(&harness.events), harness.clock.clone()),
    );
    assert!(matches!(
        bad_name.unwrap_err(),
        ConfigError::InvalidActionName
    ));
}
