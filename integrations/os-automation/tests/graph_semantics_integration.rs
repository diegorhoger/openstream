//! Multi-action graph semantics over the REAL registered OS adapters
//! (issues #10 to #12) through the real engine runtime (issue #14):
//! sequence ordering across adapters, exact virtual-time delays,
//! conditional routing on variables, failure policies
//! `stop`/`continue`, idempotency-gated retry and compensation refused
//! against the adapters' honest declarations, execution deadlines, and
//! cancellation.
//!
//! Everything runs on `FakeClock`; every backend is the recorded fake
//! (`FakeKeyboardBackend`, `FakeLaunchBackend`, `FakeMediaBackend`) so no
//! test here ever synthesizes input, launches a process, or touches an
//! audio device.

use openstream_domain::capability::Capability;
use openstream_domain::grant::{ConsentEvidence, ConsentKind, GrantLedger, SubjectRef};
use openstream_engine::{
    ActionRegistry, Clock as _, EdgeKindInput, ExecuteRequest, ExecutionReceipt, FailurePolicy,
    FailureReason, MessageId, NodeKey as EngineNodeKey, NodeKind, RawGraph, RuntimeBuilder,
    SourceDeviceId, TerminalState, TimeControl, ValidatedGraph, ValidationError,
};
use openstream_os_automation::{
    ACTION_TYPE_AUDIO_VOLUME, ACTION_TYPE_KEYBOARD_SHORTCUT, ACTION_TYPE_LAUNCH_APPLICATION,
    ACTION_TYPE_MEDIA_TRANSPORT, ApplicationTarget, FakeKeyboardBackend, FakeLaunchBackend,
    FakeMediaBackend, KeyboardSynthesizer, LaunchBackend, LaunchBinding, LaunchPolicy,
    MediaCommand, MediaDeviceController, VolumeOperation, register_keyboard_shortcut_action,
    register_launch_actions, register_media_actions,
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

const WALL_START: i64 = 1_700_000_000_000;
const EXPIRY_MARGIN_MS: i64 = 60_000;

fn subject() -> SubjectRef {
    SubjectRef::builtin("deck-actions").expect("fixture subject")
}

fn device() -> SourceDeviceId {
    SourceDeviceId::try_new("peer:test-device").expect("fixture device")
}

/// Per-capability consent evidence exactly as each taxonomy row requires.
fn consent_for(capability: &Capability) -> ConsentEvidence {
    let kinds = match capability.kind_name() {
        "os.keyboard.emit" => vec![ConsentKind::InstallReview, ConsentKind::FirstUse],
        "os.application.launch" => vec![ConsentKind::ExplicitSelection],
        // os.media.emit and audio.control are first-use rows.
        _ => vec![ConsentKind::FirstUse],
    };
    ConsentEvidence::try_new(kinds, WALL_START).expect("fixture consent")
}

fn ledger_with(capabilities: &[Capability]) -> Arc<Mutex<GrantLedger>> {
    let mut ledger = GrantLedger::new();
    for capability in capabilities {
        ledger
            .create_grant(
                subject(),
                capability.clone(),
                consent_for(capability),
                WALL_START,
            )
            .expect("fixture grant satisfies its consent class");
    }
    Arc::new(Mutex::new(ledger))
}

struct Backends {
    keyboard: Arc<FakeKeyboardBackend>,
    launch: Arc<FakeLaunchBackend>,
    media: Arc<FakeMediaBackend>,
}

fn registry_with(backends: &Backends) -> ActionRegistry {
    let mut registry = ActionRegistry::new();
    register_keyboard_shortcut_action(
        &mut registry,
        Arc::clone(&backends.keyboard) as Arc<dyn KeyboardSynthesizer>,
    )
    .expect("keyboard declaration registers");
    register_launch_actions(
        &mut registry,
        Arc::clone(&backends.launch) as Arc<dyn LaunchBackend>,
        LaunchPolicy::standard(),
        &[LaunchBinding::Application(
            ApplicationTarget::try_new("obs-studio").expect("valid identity"),
        )],
    )
    .expect("launch declaration registers");
    register_media_actions(
        &mut registry,
        Arc::clone(&backends.media) as Arc<dyn MediaDeviceController>,
    )
    .expect("media declarations register");
    registry
}

fn action(action_type: &str, capability: Capability, params: Value) -> NodeKind {
    NodeKind::Action {
        action_type: action_type.to_string(),
        capability,
        params,
        deadline_override_ms: None,
    }
}

fn key(raw: &str) -> EngineNodeKey {
    EngineNodeKey::try_new(raw).expect("fixture node key")
}

fn kb_node() -> NodeKind {
    action(
        ACTION_TYPE_KEYBOARD_SHORTCUT,
        Capability::OsKeyboardEmit { app: None },
        json!({ "keys": "ctrl+shift+t" }),
    )
}

fn transport_node() -> NodeKind {
    action(
        ACTION_TYPE_MEDIA_TRANSPORT,
        Capability::OsMediaEmit,
        json!({ "action": "play_pause" }),
    )
}

fn volume_node() -> NodeKind {
    action(
        ACTION_TYPE_AUDIO_VOLUME,
        Capability::AudioControl {
            device: "master".to_string(),
        },
        json!({ "operation": "up", "steps": 3 }),
    )
}

fn launch_node() -> NodeKind {
    action(
        ACTION_TYPE_LAUNCH_APPLICATION,
        Capability::OsApplicationLaunch {
            identity: "obs-studio".to_string(),
        },
        json!({ "identity": "obs-studio" }),
    )
}

struct Harness {
    clock: Arc<openstream_engine::FakeClock>,
}

impl Harness {
    fn new() -> Self {
        Self {
            clock: Arc::new(openstream_engine::FakeClock::new(WALL_START, 0)),
        }
    }

    fn runtime(
        &self,
        registry: ActionRegistry,
        ledger: Arc<Mutex<GrantLedger>>,
    ) -> openstream_engine::ActionRuntime {
        RuntimeBuilder::new()
            .clock(self.clock.clone())
            .time_control(self.clock.clone() as Arc<dyn TimeControl>)
            .registry(registry)
            .grant_ledger(ledger)
            .build()
            .expect("runtime builds")
    }

    fn execute(
        &self,
        runtime: &mut openstream_engine::ActionRuntime,
        graph: &Arc<ValidatedGraph>,
    ) -> ExecutionReceipt {
        runtime
            .execute(ExecuteRequest {
                source_device_id: device(),
                message_id: MessageId::generate(),
                subject: subject(),
                graph: Arc::clone(graph),
                variables: Default::default(),
                expires_at_wall_ms: WALL_START + EXPIRY_MARGIN_MS,
                cancel: None::<openstream_engine::CancelSignal>,
            })
            .expect("admission succeeds for fixture requests")
    }
}

fn failed_code(receipt: &ExecutionReceipt) -> String {
    match &receipt.state {
        TerminalState::Failed {
            reason: FailureReason::AdapterFailed { code },
        } => code.clone(),
        other => panic!("expected typed adapter failure, got {other:?}"),
    }
}

#[test]
fn sequence_orders_real_effects_across_three_adapters() {
    let harness = Harness::new();
    let backends = Backends {
        keyboard: Arc::new(FakeKeyboardBackend::new()),
        launch: Arc::new(FakeLaunchBackend::new()),
        media: Arc::new(FakeMediaBackend::new()),
    };
    let registry = registry_with(&backends);

    // seq[kb -> transport -> volume-step -> launch]: four real adapters in
    // one bounded sequence.
    let mut raw = RawGraph::new(FailurePolicy::Stop);
    for (name, kind) in [
        ("kb", kb_node()),
        ("transport", transport_node()),
        ("volume", volume_node()),
        ("launch", launch_node()),
    ] {
        raw.add_node(key(name), kind).unwrap();
    }
    raw.add_node(key("seq"), NodeKind::Sequence).unwrap();
    for to in ["kb", "transport", "volume", "launch"] {
        raw.add_edge(key("seq"), key(to), EdgeKindInput::Sequence);
    }
    raw.entry(key("seq"));
    let graph = Arc::new(ValidatedGraph::build(&raw, &registry).unwrap());

    let ledger = ledger_with(&[
        Capability::OsKeyboardEmit { app: None },
        Capability::OsMediaEmit,
        Capability::AudioControl {
            device: "master".to_string(),
        },
        Capability::OsApplicationLaunch {
            identity: "obs-studio".to_string(),
        },
    ]);
    let mut runtime = harness.runtime(registry, ledger);
    let receipt = harness.execute(&mut runtime, &graph);

    assert_eq!(receipt.state.token(), "succeeded");
    let order: Vec<&str> = receipt
        .effects
        .iter()
        .map(|effect| effect.node_key.as_str())
        .collect();
    assert_eq!(order, vec!["kb", "transport", "volume", "launch"]);
    assert_eq!(backends.keyboard.count(), 1);
    assert_eq!(backends.launch.count(), 1);
    assert_eq!(
        backends.media.invocations(),
        vec![
            openstream_os_automation::MediaInvocation::Transport(MediaCommand::PlayPause),
            openstream_os_automation::MediaInvocation::Volume(VolumeOperation::Up { steps: 3 }),
        ]
    );
    let action_types: Vec<&str> = receipt
        .effects
        .iter()
        .map(|effect| effect.action_type.as_str())
        .collect();
    assert_eq!(
        action_types,
        vec![
            ACTION_TYPE_KEYBOARD_SHORTCUT,
            ACTION_TYPE_MEDIA_TRANSPORT,
            ACTION_TYPE_AUDIO_VOLUME,
            ACTION_TYPE_LAUNCH_APPLICATION,
        ]
    );
}

#[test]
fn delay_between_real_actions_lands_exactly_on_virtual_marks() {
    let harness = Harness::new();
    let backends = Backends {
        keyboard: Arc::new(FakeKeyboardBackend::new()),
        launch: Arc::new(FakeLaunchBackend::new()),
        media: Arc::new(FakeMediaBackend::new()),
    };
    let registry = registry_with(&backends);

    let mut raw = RawGraph::new(FailurePolicy::Stop);
    raw.add_node(key("launch"), launch_node()).unwrap();
    raw.add_node(key("wait"), NodeKind::Delay { duration_ms: 250 })
        .unwrap();
    raw.add_node(key("transport"), transport_node()).unwrap();
    raw.add_node(key("seq"), NodeKind::Sequence).unwrap();
    for to in ["launch", "wait", "transport"] {
        raw.add_edge(key("seq"), key(to), EdgeKindInput::Sequence);
    }
    raw.entry(key("seq"));
    let graph = Arc::new(ValidatedGraph::build(&raw, &registry).unwrap());

    let ledger = ledger_with(&[
        Capability::OsApplicationLaunch {
            identity: "obs-studio".to_string(),
        },
        Capability::OsMediaEmit,
    ]);
    let mut runtime = harness.runtime(registry, ledger);
    let receipt = harness.execute(&mut runtime, &graph);

    assert_eq!(receipt.state.token(), "succeeded");
    let stamps: Vec<u64> = receipt
        .effects
        .iter()
        .map(|effect| effect.observed_at_monotonic_ms)
        .collect();
    assert_eq!(stamps, vec![0, 250], "delay is exact virtual time");
    assert_eq!(backends.launch.count(), 1);
    assert_eq!(backends.media.count(), 1);
}

#[test]
fn conditional_routes_between_real_adapters_on_variables() {
    let harness = Harness::new();
    let backends = Backends {
        keyboard: Arc::new(FakeKeyboardBackend::new()),
        launch: Arc::new(FakeLaunchBackend::new()),
        media: Arc::new(FakeMediaBackend::new()),
    };
    let registry = registry_with(&backends);

    let mut raw = RawGraph::new(FailurePolicy::Stop);
    raw.add_node(
        key("setup"),
        NodeKind::VariableTransform {
            op: openstream_engine::graph::TransformOp::Set {
                variable: "mode".to_string(),
                value: json!("launch"),
            },
        },
    )
    .unwrap();
    raw.add_node(
        key("branch"),
        NodeKind::Conditional {
            condition: openstream_engine::graph::Condition {
                variable: "mode".to_string(),
                op: openstream_engine::graph::ConditionOp::Equals,
                operand: json!("launch"),
            },
        },
    )
    .unwrap();
    raw.add_node(key("truth_arm"), launch_node()).unwrap();
    raw.add_node(key("false_arm"), kb_node()).unwrap();
    raw.add_node(key("seq"), NodeKind::Sequence).unwrap();
    for to in ["setup", "branch"] {
        raw.add_edge(key("seq"), key(to), EdgeKindInput::Sequence);
    }
    raw.add_edge(
        key("branch"),
        key("truth_arm"),
        EdgeKindInput::Branch { polarity: true },
    );
    raw.add_edge(
        key("branch"),
        key("false_arm"),
        EdgeKindInput::Branch { polarity: false },
    );
    raw.entry(key("seq"));
    let graph = Arc::new(ValidatedGraph::build(&raw, &registry).unwrap());

    let ledger = ledger_with(&[
        Capability::OsApplicationLaunch {
            identity: "obs-studio".to_string(),
        },
        Capability::OsKeyboardEmit { app: None },
    ]);
    let mut runtime = harness.runtime(registry, ledger);
    let receipt = harness.execute(&mut runtime, &graph);

    assert_eq!(receipt.state.token(), "succeeded");
    assert_eq!(receipt.effects.len(), 1);
    assert_eq!(receipt.effects[0].node_key.as_str(), "truth_arm");
    assert_eq!(backends.launch.count(), 1);
    assert_eq!(backends.keyboard.count(), 0, "false arm never dispatches");
}

#[test]
fn retry_of_non_idempotent_real_action_refuses_at_validation() {
    // The keyboard registration declares NonIdempotent honestly
    // (re-sending a shortcut is not safely repeatable): a retry wrapper
    // must reject at graph validation BEFORE anything can run.
    let backends = Backends {
        keyboard: Arc::new(FakeKeyboardBackend::new()),
        launch: Arc::new(FakeLaunchBackend::new()),
        media: Arc::new(FakeMediaBackend::new()),
    };
    let registry = registry_with(&backends);

    let mut raw = RawGraph::new(FailurePolicy::Stop);
    raw.add_node(key("retry"), NodeKind::Retry { attempts: 2 })
        .unwrap();
    raw.add_node(key("body"), kb_node()).unwrap();
    raw.add_edge(key("retry"), key("body"), EdgeKindInput::Sequence);
    raw.entry(key("retry"));

    match ValidatedGraph::build(&raw, &registry) {
        Err(ValidationError::RetryRequiresIdempotency { action }) => {
            assert_eq!(action, ACTION_TYPE_KEYBOARD_SHORTCUT);
        }
        other => panic!("expected idempotency rejection, got {other:?}"),
    }
    assert_eq!(backends.keyboard.count(), 0);
}

#[test]
fn compensate_policy_refused_without_adapter_proof() {
    // None of the shipped OS adapters declares safe compensation (sent
    // keys cannot be unsent; launched processes cannot be un-launched),
    // so a compensate-policy graph must reject at validation naming the
    // offending adapter; compensation is never implied.
    let backends = Backends {
        keyboard: Arc::new(FakeKeyboardBackend::new()),
        launch: Arc::new(FakeLaunchBackend::new()),
        media: Arc::new(FakeMediaBackend::new()),
    };
    let registry = registry_with(&backends);

    let mut raw = RawGraph::new(FailurePolicy::Compensate);
    for (name, kind) in [
        ("kb", kb_node()),
        ("comp_kb", NodeKind::Compensate),
        ("transport", transport_node()),
        ("comp_transport", NodeKind::Compensate),
        ("launch", launch_node()),
        ("comp_launch", NodeKind::Compensate),
    ] {
        raw.add_node(key(name), kind).unwrap();
    }
    raw.add_node(key("seq"), NodeKind::Sequence).unwrap();
    for to in ["kb", "transport", "launch"] {
        raw.add_edge(key("seq"), key(to), EdgeKindInput::Sequence);
    }
    raw.add_edge(key("kb"), key("comp_kb"), EdgeKindInput::CompensationLink);
    raw.add_edge(
        key("transport"),
        key("comp_transport"),
        EdgeKindInput::CompensationLink,
    );
    raw.add_edge(
        key("launch"),
        key("comp_launch"),
        EdgeKindInput::CompensationLink,
    );
    raw.entry(key("seq"));

    match ValidatedGraph::build(&raw, &registry) {
        Err(ValidationError::PolicyCompensateInvalid { .. }) => {}
        other => panic!("expected compensate-policy rejection, got {other:?}"),
    }
    assert_eq!(backends.keyboard.count(), 0);
    assert_eq!(backends.launch.count(), 0);
    assert_eq!(backends.media.count(), 0);
}

#[test]
fn stop_policy_halts_after_typed_media_failure() {
    let harness = Harness::new();
    let backends = Backends {
        keyboard: Arc::new(FakeKeyboardBackend::new()),
        launch: Arc::new(FakeLaunchBackend::new()),
        media: Arc::new(FakeMediaBackend::new()),
    };
    let registry = registry_with(&backends);

    let mut raw = RawGraph::new(FailurePolicy::Stop);
    raw.add_node(
        key("bad_transport"),
        action(
            ACTION_TYPE_MEDIA_TRANSPORT,
            Capability::OsMediaEmit,
            json!({ "action": "eject_everything" }),
        ),
    )
    .unwrap();
    raw.add_node(key("launch"), launch_node()).unwrap();
    raw.add_node(key("seq"), NodeKind::Sequence).unwrap();
    for to in ["bad_transport", "launch"] {
        raw.add_edge(key("seq"), key(to), EdgeKindInput::Sequence);
    }
    raw.entry(key("seq"));
    let graph = Arc::new(ValidatedGraph::build(&raw, &registry).unwrap());

    let ledger = ledger_with(&[
        Capability::OsMediaEmit,
        Capability::OsApplicationLaunch {
            identity: "obs-studio".to_string(),
        },
    ]);
    let mut runtime = harness.runtime(registry, ledger);
    let receipt = harness.execute(&mut runtime, &graph);

    assert_eq!(failed_code(&receipt), "invalid_media_config");
    assert_eq!(
        backends.launch.count(),
        0,
        "stop policy prevents downstream dispatch"
    );
    assert_eq!(receipt.effects.len(), 1);
}

#[test]
fn continue_policy_runs_remaining_siblings_and_reports_failure() {
    let harness = Harness::new();
    let backends = Backends {
        keyboard: Arc::new(FakeKeyboardBackend::new()),
        launch: Arc::new(FakeLaunchBackend::new()),
        media: Arc::new(FakeMediaBackend::new()),
    };
    backends
        .media
        .set_failure(Some(openstream_os_automation::MediaError::PlatformFailure));
    let registry = registry_with(&backends);

    let mut raw = RawGraph::new(FailurePolicy::Continue);
    for (name, kind) in [
        ("kb", kb_node()),
        ("transport", transport_node()),
        ("launch", launch_node()),
    ] {
        raw.add_node(key(name), kind).unwrap();
    }
    raw.add_node(key("seq"), NodeKind::Sequence).unwrap();
    for to in ["kb", "transport", "launch"] {
        raw.add_edge(key("seq"), key(to), EdgeKindInput::Sequence);
    }
    raw.entry(key("seq"));
    let graph = Arc::new(ValidatedGraph::build(&raw, &registry).unwrap());

    let ledger = ledger_with(&[
        Capability::OsKeyboardEmit { app: None },
        Capability::OsMediaEmit,
        Capability::OsApplicationLaunch {
            identity: "obs-studio".to_string(),
        },
    ]);
    let mut runtime = harness.runtime(registry, ledger);
    let receipt = harness.execute(&mut runtime, &graph);

    assert_eq!(failed_code(&receipt), "platform_refused");
    assert_eq!(backends.keyboard.count(), 1);
    assert_eq!(backends.launch.count(), 1, "continue still runs siblings");
    let outcomes: Vec<(String, String)> = receipt
        .effects
        .iter()
        .map(|effect| (effect.node_key.to_string(), effect.outcome.clone()))
        .collect();
    assert_eq!(
        outcomes,
        vec![
            ("kb".to_string(), "succeeded".to_string()),
            ("transport".to_string(), "failed".to_string()),
            ("launch".to_string(), "succeeded".to_string()),
        ]
    );
}

#[test]
fn execution_deadline_bounds_a_delayed_real_sequence() {
    let harness = Harness::new();
    let backends = Backends {
        keyboard: Arc::new(FakeKeyboardBackend::new()),
        launch: Arc::new(FakeLaunchBackend::new()),
        media: Arc::new(FakeMediaBackend::new()),
    };
    let registry = registry_with(&backends);

    let mut raw = RawGraph::new(FailurePolicy::Stop);
    raw.execution_deadline_ms(Some(1_000));
    raw.add_node(key("launch"), launch_node()).unwrap();
    raw.add_node(key("wait"), NodeKind::Delay { duration_ms: 5_000 })
        .unwrap();
    raw.add_node(key("transport"), transport_node()).unwrap();
    raw.add_node(key("seq"), NodeKind::Sequence).unwrap();
    for to in ["launch", "wait", "transport"] {
        raw.add_edge(key("seq"), key(to), EdgeKindInput::Sequence);
    }
    raw.entry(key("seq"));
    let graph = Arc::new(ValidatedGraph::build(&raw, &registry).unwrap());

    let ledger = ledger_with(&[
        Capability::OsApplicationLaunch {
            identity: "obs-studio".to_string(),
        },
        Capability::OsMediaEmit,
    ]);
    let mut runtime = harness.runtime(registry, ledger);
    let receipt = harness.execute(&mut runtime, &graph);

    // The pre-deadline effect dispatched and is recorded honestly; the
    // post-deadline tail never runs; terminal is expired, never success.
    assert_eq!(receipt.state.token(), "expired");
    assert_eq!(harness.clock.monotonic_ms(), 1_000);
    assert_eq!(backends.launch.count(), 1);
    assert_eq!(backends.media.count(), 0);
    let outcomes: Vec<String> = receipt
        .effects
        .iter()
        .map(|effect| effect.outcome.clone())
        .collect();
    assert_eq!(outcomes, vec!["succeeded"]);
}

#[test]
fn cancellation_mid_graph_never_runs_the_pending_tail() {
    let harness = Harness::new();
    let backends = Backends {
        keyboard: Arc::new(FakeKeyboardBackend::new()),
        launch: Arc::new(FakeLaunchBackend::new()),
        media: Arc::new(FakeMediaBackend::new()),
    };
    let registry = registry_with(&backends);

    let mut raw = RawGraph::new(FailurePolicy::Stop);
    raw.add_node(key("kb"), kb_node()).unwrap();
    raw.add_node(key("wait"), NodeKind::Delay { duration_ms: 5_000 })
        .unwrap();
    raw.add_node(key("launch"), launch_node()).unwrap();
    raw.add_node(key("seq"), NodeKind::Sequence).unwrap();
    for to in ["kb", "wait", "launch"] {
        raw.add_edge(key("seq"), key(to), EdgeKindInput::Sequence);
    }
    raw.entry(key("seq"));
    let graph = Arc::new(ValidatedGraph::build(&raw, &registry).unwrap());

    let cancel = openstream_engine::CancelSignal::new();
    let request = ExecuteRequest {
        source_device_id: device(),
        message_id: MessageId::generate(),
        subject: subject(),
        graph: Arc::clone(&graph),
        variables: Default::default(),
        expires_at_wall_ms: WALL_START + EXPIRY_MARGIN_MS,
        cancel: Some(cancel.clone()),
    };

    let ledger = ledger_with(&[
        Capability::OsKeyboardEmit { app: None },
        Capability::OsApplicationLaunch {
            identity: "obs-studio".to_string(),
        },
    ]);
    let mut runtime = harness.runtime(registry, ledger);
    let mut handle = runtime.begin(request).unwrap();

    // First pass completes the shortcut and parks inside the delay.
    assert!(handle.step());
    handle.cancel();

    let receipt = handle.run_to_completion().expect("terminal reached");
    assert_eq!(receipt.state.token(), "cancelled");
    assert_eq!(backends.keyboard.count(), 1);
    assert_eq!(backends.launch.count(), 0, "tail never dispatches");
}
