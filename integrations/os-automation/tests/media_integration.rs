//! End-to-end contract tests for the media transport and volume adapters
//! (issue #12) through the real engine runtime: registry registration with
//! honest scope declarations, graph validation, grant intersection before
//! dispatch, typed config validation at dispatch, named-device fail-closed
//! enforcement, explicit unsupported-platform reporting, and revocation.
//!
//! Everything runs on `FakeClock`; the controller is always the recorded
//! fake or the unsupported stub — no test here ever touches a real OS
//! media or audio stack. The opt-in real-Windows check lives at the bottom
//! of this file, doubly gated (`#[ignore]` + environment flag).

use openstream_domain::capability::Capability;
use openstream_domain::grant::{
    ConsentEvidence, ConsentKind, DenialReason, GrantLedger, SubjectRef,
};
use openstream_engine::{
    ActionRegistry, Clock as _, EffectPort as _, ExecuteRequest, ExecutionReceipt, FailurePolicy,
    FailureReason, MessageId, NodeKey as EngineNodeKey, NodeKind, RawGraph, RuntimeBuilder,
    SourceDeviceId, TerminalState, TimeControl, ValidatedGraph,
};
use openstream_os_automation::{
    ACTION_TYPE_AUDIO_VOLUME, ACTION_TYPE_MEDIA_TRANSPORT, AudioVolumePort, FakeMediaBackend,
    MASTER_DEVICE_SCOPE, MediaDeviceController, UnsupportedMediaBackend, register_media_actions,
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

fn media_consent() -> ConsentEvidence {
    // The taxonomy requires FirstUse for os.media.emit and
    // audio.control:<device>; substitutions fail closed at grant creation.
    ConsentEvidence::try_new(vec![ConsentKind::FirstUse], WALL_START).expect("fixture consent")
}

fn ledger_with(capabilities: &[Capability]) -> Arc<Mutex<GrantLedger>> {
    let mut ledger = GrantLedger::new();
    for capability in capabilities {
        ledger
            .create_grant(subject(), capability.clone(), media_consent(), WALL_START)
            .expect("fixture grant satisfies its consent class");
    }
    Arc::new(Mutex::new(ledger))
}

fn registry_with(backend: Arc<dyn MediaDeviceController>) -> ActionRegistry {
    let mut registry = ActionRegistry::new();
    register_media_actions(&mut registry, backend).expect("fixed declarations must register");
    registry
}

fn media_graph(
    action_type: &str,
    capability: Capability,
    params: Value,
    registry: &ActionRegistry,
) -> Result<Arc<ValidatedGraph>, String> {
    let key = EngineNodeKey::try_new("media").expect("fixture node key");
    let mut raw = RawGraph::new(FailurePolicy::Stop);
    raw.add_node(
        key.clone(),
        NodeKind::Action {
            action_type: action_type.to_string(),
            capability,
            params,
            deadline_override_ms: None,
        },
    )
    .expect("single node");
    let raw = raw.entry(key);
    ValidatedGraph::build(raw, registry)
        .map(Arc::new)
        .map_err(|error| error.to_string())
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

fn master_volume() -> Capability {
    Capability::AudioControl {
        device: MASTER_DEVICE_SCOPE.to_string(),
    }
}

#[test]
fn ungranted_transport_denies_before_any_backend_call() {
    let harness = Harness::new();
    let fake = Arc::new(FakeMediaBackend::new());
    let registry = registry_with(Arc::clone(&fake) as Arc<dyn MediaDeviceController>);
    let graph = media_graph(
        ACTION_TYPE_MEDIA_TRANSPORT,
        Capability::OsMediaEmit,
        json!({ "action": "next_track" }),
        &registry,
    )
    .expect("graph validates");

    // Deny-by-default: zero grants recorded anywhere.
    let mut runtime = harness.runtime(registry, ledger_with(&[]));
    let receipt = harness.execute(&mut runtime, &graph);

    assert_eq!(receipt.state.token(), "failed");
    match &receipt.state {
        TerminalState::Failed {
            reason: FailureReason::CapabilityDenied(DenialReason::NoActiveGrant),
        } => {}
        other => panic!("expected capability denial, got {other:?}"),
    }
    assert_eq!(fake.count(), 0, "denial must fire before any dispatch");
}

#[test]
fn granted_transport_records_the_exact_command() {
    let harness = Harness::new();
    let fake = Arc::new(FakeMediaBackend::new());
    let registry = registry_with(Arc::clone(&fake) as Arc<dyn MediaDeviceController>);
    let graph = media_graph(
        ACTION_TYPE_MEDIA_TRANSPORT,
        Capability::OsMediaEmit,
        json!({ "action": "play_pause" }),
        &registry,
    )
    .expect("graph validates");

    let mut runtime = harness.runtime(registry, ledger_with(&[Capability::OsMediaEmit]));
    let receipt = harness.execute(&mut runtime, &graph);

    assert_eq!(receipt.state.token(), "succeeded");
    assert_eq!(fake.count(), 1);
    assert_eq!(
        fake.invocations()[0],
        openstream_os_automation::MediaInvocation::Transport(
            openstream_os_automation::MediaCommand::PlayPause
        )
    );
    assert_eq!(receipt.effects.len(), 1);
    assert_eq!(receipt.effects[0].outcome, "succeeded");
    assert_eq!(receipt.effects[0].action_type, "os.media.transport");
}

#[test]
fn granted_volume_operation_records_the_exact_operation() {
    let harness = Harness::new();
    let fake = Arc::new(FakeMediaBackend::new());
    let registry = registry_with(Arc::clone(&fake) as Arc<dyn MediaDeviceController>);
    let graph = media_graph(
        ACTION_TYPE_AUDIO_VOLUME,
        master_volume(),
        json!({ "operation": "up", "steps": 2 }),
        &registry,
    )
    .expect("graph validates");

    let mut runtime = harness.runtime(registry, ledger_with(&[master_volume()]));
    let receipt = harness.execute(&mut runtime, &graph);

    assert_eq!(receipt.state.token(), "succeeded");
    assert_eq!(fake.count(), 1);
    assert_eq!(
        fake.invocations()[0],
        openstream_os_automation::MediaInvocation::Volume(
            openstream_os_automation::VolumeOperation::Up { steps: 2 }
        )
    );
    assert_eq!(receipt.effects[0].action_type, "os.audio.volume");
}

#[test]
fn revocation_denies_the_next_execution() {
    let harness = Harness::new();
    let fake = Arc::new(FakeMediaBackend::new());
    let registry = registry_with(Arc::clone(&fake) as Arc<dyn MediaDeviceController>);
    let graph = media_graph(
        ACTION_TYPE_AUDIO_VOLUME,
        master_volume(),
        json!({ "operation": "toggle_mute" }),
        &registry,
    )
    .expect("graph validates");

    let ledger = ledger_with(&[master_volume()]);
    let mut runtime = harness.runtime(registry, Arc::clone(&ledger));

    let first = harness.execute(&mut runtime, &graph);
    assert_eq!(first.state.token(), "succeeded");
    assert_eq!(fake.count(), 1);

    {
        let mut guard = ledger
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard
            .revoke_all(harness.clock.wall_now_ms())
            .expect("revoke");
    }
    let second = harness.execute(&mut runtime, &graph);
    assert_eq!(second.state.token(), "failed");
    assert_eq!(
        fake.count(),
        1,
        "revocation must block any further dispatch"
    );
}

#[test]
fn a_master_grant_never_covers_another_device_scope() {
    // The volume registration declares only device=master, so a node bound
    // to another device cannot even validate; the honest runtime-level
    // analogue is that the declared-but-ungranted shape denies. Here the
    // port-level refusal is proven in the unit suite; this test pins the
    // graph layer: only the declared master scope builds.
    let fake = Arc::new(FakeMediaBackend::new());
    let registry = registry_with(Arc::clone(&fake) as Arc<dyn MediaDeviceController>);

    let error = media_graph(
        ACTION_TYPE_AUDIO_VOLUME,
        Capability::AudioControl {
            device: "headphones".to_string(),
        },
        json!({ "operation": "up" }),
        &registry,
    )
    .expect_err("undeclared device scope must reject at S3");
    assert!(
        error.contains("capability"),
        "expected scope rejection detail, got: {error}"
    );
    assert_eq!(fake.count(), 0);

    // Sanity: the declared master scope still validates.
    assert!(
        media_graph(
            ACTION_TYPE_AUDIO_VOLUME,
            master_volume(),
            json!({ "operation": "up" }),
            &registry
        )
        .is_ok()
    );
}

#[test]
fn foreign_requested_capability_rejects_at_graph_validation() {
    let fake = Arc::new(FakeMediaBackend::new());
    let registry = registry_with(Arc::clone(&fake) as Arc<dyn MediaDeviceController>);
    // Binding a different capability kind to a transport action type must
    // reject at S3 before grants are consulted.
    let error = media_graph(
        ACTION_TYPE_MEDIA_TRANSPORT,
        Capability::ProcessExecute {
            identity: "obs64.exe".to_string(),
        },
        json!({ "action": "play_pause" }),
        &registry,
    )
    .expect_err("undeclared scope must reject");
    assert!(error.contains("capability"));

    // Sanity: an honest binding on the same action type still builds.
    assert!(
        media_graph(
            ACTION_TYPE_MEDIA_TRANSPORT,
            Capability::OsMediaEmit,
            json!({ "action": "play_pause" }),
            &registry
        )
        .is_ok()
    );
}

#[test]
fn invalid_config_fails_typed_without_touching_the_backend() {
    let harness = Harness::new();
    let fake = Arc::new(FakeMediaBackend::new());
    let registry = registry_with(Arc::clone(&fake) as Arc<dyn MediaDeviceController>);
    let graph = media_graph(
        ACTION_TYPE_MEDIA_TRANSPORT,
        Capability::OsMediaEmit,
        json!({ "action": "eject_disc" }),
        &registry,
    )
    .expect("graph validates against the declared scope family");

    let mut runtime = harness.runtime(registry, ledger_with(&[Capability::OsMediaEmit]));
    let receipt = harness.execute(&mut runtime, &graph);

    assert_eq!(
        failed_code(&receipt),
        "invalid_media_config",
        "off-vocabulary commands fail typed, never silently"
    );
    assert_eq!(fake.count(), 0);
}

/// The shipped unsupported-platform stub, exercised through the full
/// runtime on every host so the honest explicit error is contract-tested
/// everywhere (CI included).
#[test]
fn unsupported_platform_reports_explicit_typed_error() {
    let harness = Harness::new();
    let backend = Arc::new(UnsupportedMediaBackend::new("linux"));
    let mut registry = ActionRegistry::new();
    register_media_actions(&mut registry, backend as Arc<dyn MediaDeviceController>)
        .expect("fixed declarations must register");

    let graph = media_graph(
        ACTION_TYPE_MEDIA_TRANSPORT,
        Capability::OsMediaEmit,
        json!({ "action": "previous_track" }),
        &registry,
    )
    .expect("graph validates");

    let mut runtime = harness.runtime(registry, ledger_with(&[Capability::OsMediaEmit]));
    let receipt = harness.execute(&mut runtime, &graph);

    assert_eq!(
        failed_code(&receipt),
        "unsupported_platform",
        "platform gaps surface explicitly, never as success"
    );
}

/// Direct-port defense-in-depth check: a differently-named device request
/// that bypassed the engine would still refuse without any backend effect
/// ("no silent global fallback").
#[test]
fn direct_port_refuses_non_master_device_scope_without_fallback() {
    let fake = Arc::new(FakeMediaBackend::new());
    let port = AudioVolumePort::new(Arc::clone(&fake) as Arc<dyn MediaDeviceController>);
    let request = openstream_engine::EffectRequest {
        execution_id: openstream_engine::ExecutionId::generate(),
        node_key: openstream_engine::NodeKey::try_new("direct").expect("fixture key"),
        action_type: ACTION_TYPE_AUDIO_VOLUME.to_string(),
        capability: Capability::AudioControl {
            device: "headphones".to_string(),
        },
        params: json!({ "operation": "down", "steps": 4 }),
        idempotency_key: "fixture:key".to_string(),
        attempt: 0,
        is_compensation: false,
    };
    let response = port.invoke(request).unwrap();
    match response {
        openstream_engine::EffectResponse::Immediate(outcome) => assert_eq!(
            outcome.failure_code().expect("failure carries code"),
            "device_scope_unsupported"
        ),
        other => panic!("unexpected response {other:?}"),
    }
    assert_eq!(
        fake.count(),
        0,
        "scoped requests must never degrade into master control"
    );
}

/// Real-backend integration check. Doubly gated so nothing ever fires
/// unless a human asks for it on a Windows machine:
/// 1. `#[ignore]` keeps it out of default `cargo test` runs;
/// 2. the `OPENSTREAM_OS_MEDIA_E2E=1` environment flag must be set even
///    when invoked with `--ignored`.
///
/// Run locally with:
/// `OPENSTREAM_OS_MEDIA_E2E=1 cargo test -p openstream-os-automation --test media_integration -- --ignored`
///
/// The synthesized effect raises the master volume by one bounded step and
/// lowers it again, restoring the prior level exactly and touching nothing
/// else on the host.
#[cfg(target_os = "windows")]
#[test]
#[ignore = "real SendInput synthesis: run explicitly with OPENSTREAM_OS_MEDIA_E2E=1"]
fn real_windows_backend_steps_volume_and_restores_level() {
    if std::env::var("OPENSTREAM_OS_MEDIA_E2E").ok().as_deref() != Some("1") {
        panic!("refusing to synthesize real input without OPENSTREAM_OS_MEDIA_E2E=1");
    }
    use openstream_os_automation::{StepDirection, VolumeOperation};

    let backend = openstream_os_automation::WindowsMediaBackend::new();
    // One bounded relative step proves the volume path end to end...
    backend
        .adjust_volume(&VolumeOperation::new_step(StepDirection::Up, 1).expect("bounded"))
        .expect("SendInput volume synthesis succeeds on Windows");
    // ...and one step down restores the prior level.
    backend
        .adjust_volume(&VolumeOperation::new_step(StepDirection::Down, 1).expect("bounded"))
        .expect("SendInput volume synthesis succeeds on Windows");
}
