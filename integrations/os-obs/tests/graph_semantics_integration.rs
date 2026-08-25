//! Multi-action graph semantics over the REAL registered OBS actions
//! (issue #13) through the real engine runtime (issue #14): sequence
//! ordering across scene switches, parallel fan-out join, failure policy
//! `stop`, honest `outcome_unknown` on a lost outcome, the destructive
//! arming gate inside a graph context, execution deadlines across delay
//! nodes, and cancellation.
//!
//! Everything runs on `FakeClock`; the backend is always the recorded
//! `FakeObsController` (documented test double behind the registered
//! ports) — no test here opens any socket or requires OBS. Per the PR
//! #75 gate, no source-visibility or input-mute fixtures appear anywhere.

use openstream_domain::capability::Capability;
use openstream_domain::grant::{ConsentEvidence, ConsentKind, GrantLedger, SubjectRef};
use openstream_engine::{
    ActionRegistry, CancelSignal, Clock as _, EdgeKindInput, ExecuteRequest, ExecutionReceipt,
    FailurePolicy, FailureReason, MessageId, NodeKey as EngineNodeKey, NodeKind, RawGraph,
    RuntimeBuilder, SourceDeviceId, TerminalState, TimeControl, ValidatedGraph,
};
use openstream_os_obs::{
    ACTION_TYPE_OBS_REPLAY_SAVE, ACTION_TYPE_OBS_SCENE_SWITCH, ACTION_TYPE_OBS_STREAM_START,
    CODE_NOT_ARMED, CODE_OBS_REJECTED, FakeObsController, ObsController, ObsFailure, ObsInvocation,
    register_obs_actions,
};
use serde_json::json;
use std::sync::{Arc, Mutex};

const WALL_START: i64 = 1_700_000_000_000;
const EXPIRY_MARGIN_MS: i64 = 60_000;

fn subject() -> SubjectRef {
    SubjectRef::builtin("deck-actions").expect("fixture subject")
}

fn device() -> SourceDeviceId {
    SourceDeviceId::try_new("peer:test-device").expect("fixture device")
}

fn scene_consent() -> ConsentEvidence {
    // Taxonomy row obs.control.scene: install review plus first use.
    ConsentEvidence::try_new(
        vec![ConsentKind::InstallReview, ConsentKind::FirstUse],
        WALL_START,
    )
    .expect("fixture consent")
}

fn stream_consent() -> ConsentEvidence {
    // Taxonomy row obs.control.stream: first use PLUS destructive arming.
    ConsentEvidence::try_new(
        vec![ConsentKind::FirstUse, ConsentKind::DestructiveArming],
        WALL_START,
    )
    .expect("fixture consent")
}

fn ledger_with(capabilities: &[Capability]) -> Arc<Mutex<GrantLedger>> {
    let mut ledger = GrantLedger::new();
    for capability in capabilities {
        let consent = match capability.kind_name() {
            "obs.control.scene" => scene_consent(),
            _ => stream_consent(),
        };
        ledger
            .create_grant(subject(), capability.clone(), consent, WALL_START)
            .expect("fixture grant satisfies its consent class");
    }
    Arc::new(Mutex::new(ledger))
}

fn registry_with(backend: Arc<dyn ObsController>) -> ActionRegistry {
    let mut registry = ActionRegistry::new();
    register_obs_actions(&mut registry, backend).expect("fixed declarations must register");
    registry
}

fn key(raw: &str) -> EngineNodeKey {
    EngineNodeKey::try_new(raw).expect("fixture node key")
}

fn switch_node(scene: &str) -> NodeKind {
    NodeKind::Action {
        action_type: ACTION_TYPE_OBS_SCENE_SWITCH.to_string(),
        capability: Capability::ObsControlScene,
        params: json!({ "scene": scene }),
        deadline_override_ms: None,
    }
}

fn replay_save_node() -> NodeKind {
    NodeKind::Action {
        action_type: ACTION_TYPE_OBS_REPLAY_SAVE.to_string(),
        capability: Capability::ObsControlStream,
        params: json!({}),
        deadline_override_ms: None,
    }
}

fn stream_start_unarmed() -> NodeKind {
    NodeKind::Action {
        action_type: ACTION_TYPE_OBS_STREAM_START.to_string(),
        capability: Capability::ObsControlStream,
        // Structurally an object, but missing the exact `"armed": true`
        // confirmation the destructive schema requires.
        params: json!({}),
        deadline_override_ms: None,
    }
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
                cancel: None::<CancelSignal>,
            })
            .expect("admission succeeds for fixture requests")
    }

    /// Builds a sequence-rooted graph over `(node key, kind)` pairs with
    /// explicit `(from, to)` sequence edges.
    fn sequence_graph(
        policy: FailurePolicy,
        nodes: &[(&str, NodeKind)],
        edges: &[(&str, &str)],
        registry: &ActionRegistry,
    ) -> Arc<ValidatedGraph> {
        let mut raw = RawGraph::new(policy);
        for (name, kind) in nodes {
            raw.add_node(key(name), kind.clone()).expect("fixture node");
        }
        raw.add_node(key("seq"), NodeKind::Sequence).expect("root");
        for (from, to) in edges {
            raw.add_edge(key(from), key(to), EdgeKindInput::Sequence);
        }
        raw.entry(key("seq"));
        Arc::new(ValidatedGraph::build(&raw, registry).expect("fixture graph validates"))
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
fn scene_sequence_switches_in_insertion_order() {
    let harness = Harness::new();
    let fake = FakeObsController::new();
    let registry = registry_with(Arc::clone(&fake) as Arc<dyn ObsController>);

    let graph = Harness::sequence_graph(
        FailurePolicy::Stop,
        &[
            ("wide", switch_node("wide")),
            ("closeup", switch_node("closeup")),
        ],
        &[("seq", "wide"), ("seq", "closeup")],
        &registry,
    );

    let mut runtime = harness.runtime(registry, ledger_with(&[Capability::ObsControlScene]));
    let receipt = harness.execute(&mut runtime, &graph);

    assert_eq!(receipt.state.token(), "succeeded");
    let scenes: Vec<String> = fake
        .invocations()
        .into_iter()
        .filter_map(|invocation| match invocation {
            ObsInvocation::SceneSwitch(scene) => Some(scene),
            _ => None,
        })
        .collect();
    assert_eq!(scenes, vec!["wide".to_string(), "closeup".to_string()]);
    let order: Vec<&str> = receipt
        .effects
        .iter()
        .map(|effect| effect.node_key.as_str())
        .collect();
    assert_eq!(order, vec!["wide", "closeup"]);
}

#[test]
fn parallel_scene_fanout_joins_all_branches() {
    let harness = Harness::new();
    let fake = FakeObsController::new();
    let registry = registry_with(Arc::clone(&fake) as Arc<dyn ObsController>);

    let mut raw = RawGraph::new(FailurePolicy::Stop);
    raw.add_node(key("fan"), NodeKind::Parallel).unwrap();
    for name in ["cam_a", "cam_b", "cam_c"] {
        let scene = format!("scene_{name}");
        raw.add_node(key(name), switch_node(&scene)).unwrap();
        raw.add_edge(key("fan"), key(name), EdgeKindInput::Sequence);
    }
    raw.entry(key("fan"));
    let graph = Arc::new(ValidatedGraph::build(&raw, &registry).unwrap());

    let mut runtime = harness.runtime(registry, ledger_with(&[Capability::ObsControlScene]));
    let receipt = harness.execute(&mut runtime, &graph);

    assert_eq!(receipt.state.token(), "succeeded");
    assert_eq!(fake.count(), 3, "every branch reached the adapter");
    assert_eq!(receipt.effects.len(), 3);
    let order: Vec<&str> = receipt
        .effects
        .iter()
        .map(|effect| effect.node_key.as_str())
        .collect();
    assert_eq!(order, vec!["cam_a", "cam_b", "cam_c"]);
}

#[test]
fn stop_policy_blocks_downstream_replay_after_obs_rejection() {
    let harness = Harness::new();
    let fake = FakeObsController::new();
    fake.set_failure(Some(ObsFailure::ObsRejected));
    let registry = registry_with(Arc::clone(&fake) as Arc<dyn ObsController>);

    let graph = Harness::sequence_graph(
        FailurePolicy::Stop,
        &[("wide", switch_node("wide")), ("save", replay_save_node())],
        &[("seq", "wide"), ("seq", "save")],
        &registry,
    );

    let mut runtime = harness.runtime(
        registry,
        ledger_with(&[Capability::ObsControlScene, Capability::ObsControlStream]),
    );
    let receipt = harness.execute(&mut runtime, &graph);

    assert_eq!(failed_code(&receipt), CODE_OBS_REJECTED);
    assert_eq!(
        fake.count(),
        0,
        "rejected effects record nothing and stop blocks downstream work"
    );
    assert_eq!(receipt.effects.len(), 1);
}

#[test]
fn outcome_lost_surfaces_outcome_unknown_and_never_auto_retries() {
    let harness = Harness::new();
    let fake = FakeObsController::new();
    fake.set_failure(Some(ObsFailure::OutcomeLost));
    let registry = registry_with(Arc::clone(&fake) as Arc<dyn ObsController>);

    let graph = Harness::sequence_graph(
        FailurePolicy::Stop,
        &[("wide", switch_node("wide"))],
        &[("seq", "wide")],
        &registry,
    );

    let mut runtime = harness.runtime(registry, ledger_with(&[Capability::ObsControlScene]));
    let receipt = harness.execute(&mut runtime, &graph);

    // Crash-gap honesty through a real registered adapter: an
    // unobservable result journals outcome_unknown, never success, and
    // receives no automatic retry (non-idempotent posture).
    assert_eq!(receipt.state.token(), "outcome_unknown");
    assert_eq!(receipt.effects.len(), 1);
    assert_eq!(receipt.effects[0].outcome, "unknown");

    // The corrective scan has nothing to close: the dispatch resolved its
    // own prepared record before surfacing the unknown outcome.
    assert_eq!(
        runtime.recover_outcome_unknown().unwrap(),
        Vec::<openstream_engine::ExecutionId>::new()
    );
}

#[test]
fn unarmed_stream_start_refuses_before_any_backend_call() {
    let harness = Harness::new();
    let fake = FakeObsController::new();
    let registry = registry_with(Arc::clone(&fake) as Arc<dyn ObsController>);

    let graph = Harness::sequence_graph(
        FailurePolicy::Stop,
        &[("go_live", stream_start_unarmed())],
        &[("seq", "go_live")],
        &registry,
    );

    let mut runtime = harness.runtime(registry, ledger_with(&[Capability::ObsControlStream]));
    let receipt = harness.execute(&mut runtime, &graph);

    assert_eq!(failed_code(&receipt), CODE_NOT_ARMED);
    assert_eq!(
        fake.count(),
        0,
        "the arming gate fires before any wire effect"
    );
}

#[test]
fn execution_deadline_bounds_a_delayed_scene_sequence() {
    let harness = Harness::new();
    let fake = FakeObsController::new();
    let registry = registry_with(Arc::clone(&fake) as Arc<dyn ObsController>);

    let mut raw = RawGraph::new(FailurePolicy::Stop);
    raw.execution_deadline_ms(Some(1_000));
    raw.add_node(key("wide"), switch_node("wide")).unwrap();
    raw.add_node(key("wait"), NodeKind::Delay { duration_ms: 5_000 })
        .unwrap();
    raw.add_node(key("closeup"), switch_node("closeup"))
        .unwrap();
    raw.add_node(key("seq"), NodeKind::Sequence).unwrap();
    for to in ["wide", "wait", "closeup"] {
        raw.add_edge(key("seq"), key(to), EdgeKindInput::Sequence);
    }
    raw.entry(key("seq"));
    let graph = Arc::new(ValidatedGraph::build(&raw, &registry).unwrap());

    let mut runtime = harness.runtime(registry, ledger_with(&[Capability::ObsControlScene]));
    let receipt = harness.execute(&mut runtime, &graph);

    assert_eq!(receipt.state.token(), "expired");
    assert_eq!(harness.clock.monotonic_ms(), 1_000);
    let scenes: Vec<String> = fake
        .invocations()
        .into_iter()
        .filter_map(|invocation| match invocation {
            ObsInvocation::SceneSwitch(scene) => Some(scene),
            _ => None,
        })
        .collect();
    assert_eq!(
        scenes,
        vec!["wide".to_string()],
        "post-deadline tail never runs"
    );
}

#[test]
fn cancellation_before_first_dispatch_never_touches_the_controller() {
    let harness = Harness::new();
    let fake = FakeObsController::new();
    let registry = registry_with(Arc::clone(&fake) as Arc<dyn ObsController>);

    let graph = Harness::sequence_graph(
        FailurePolicy::Stop,
        &[
            ("wide", switch_node("wide")),
            ("closeup", switch_node("closeup")),
        ],
        &[("seq", "wide"), ("seq", "closeup")],
        &registry,
    );

    let cancel = CancelSignal::new();
    cancel.cancel();
    let request = ExecuteRequest {
        source_device_id: device(),
        message_id: MessageId::generate(),
        subject: subject(),
        graph: Arc::clone(&graph),
        variables: Default::default(),
        expires_at_wall_ms: WALL_START + EXPIRY_MARGIN_MS,
        cancel: Some(cancel),
    };

    let mut runtime = harness.runtime(registry, ledger_with(&[Capability::ObsControlScene]));
    let mut handle = runtime.begin(request).unwrap();
    let receipt = handle.run_to_completion().expect("terminal reached");

    assert_eq!(receipt.state.token(), "cancelled");
    assert_eq!(
        fake.count(),
        0,
        "cancellation before the first safe point prevents every dispatch"
    );
}
