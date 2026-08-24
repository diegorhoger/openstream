//! End-to-end contract tests for the launch-action adapters (issue #11)
//! through the real engine runtime: registry registration with exact
//! approved-target scopes, graph validation, grant intersection before
//! dispatch, typed config/policy validation at dispatch, explicit
//! missing-target and unsupported-platform reporting, and revocation.
//!
//! Everything runs on `FakeClock`; the launcher is always the recorded
//! fake or the unsupported stub — no test here ever launches anything on
//! the host OS. The opt-in real-Windows check lives at the bottom of this
//! file, doubly gated (`#[ignore]` + environment flag).

use openstream_domain::capability::Capability;
use openstream_domain::grant::{
    ConsentEvidence, ConsentKind, DenialReason, GrantLedger, SubjectRef,
};
use openstream_engine::{
    ActionRegistry, Clock as _, ExecuteRequest, ExecutionReceipt, FailurePolicy, FailureReason,
    MessageId, NodeKey as EngineNodeKey, NodeKind, RawGraph, RuntimeBuilder, SourceDeviceId,
    TerminalState, TimeControl, ValidatedGraph,
};
use openstream_os_automation::{
    ACTION_TYPE_LAUNCH_APPLICATION, ACTION_TYPE_LAUNCH_FILE, ACTION_TYPE_LAUNCH_URL,
    ApplicationTarget, FakeLaunchBackend, FileTarget, LaunchBackend, LaunchBinding, LaunchError,
    LaunchPolicy, UnsupportedLaunchBackend, UrlTarget, register_launch_actions,
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

fn launch_consent() -> ConsentEvidence {
    // The taxonomy requires ExplicitSelection (per-application selection
    // dialog) for os.application.launch; substitutions fail closed at
    // grant creation.
    ConsentEvidence::try_new(vec![ConsentKind::ExplicitSelection], WALL_START)
        .expect("fixture consent")
}

fn fixture_bindings() -> Vec<LaunchBinding> {
    vec![
        LaunchBinding::Application(ApplicationTarget::try_new("obs-studio").unwrap()),
        // Declared but never granted in tests: proves per-target scoping.
        LaunchBinding::Application(ApplicationTarget::try_new("notepad").unwrap()),
        LaunchBinding::File(FileTarget::try_new("/stage/cue.txt").unwrap()),
        LaunchBinding::Url(UrlTarget::try_new("https://example.com/live").unwrap()),
    ]
}

fn ledger_with(capabilities: &[Capability]) -> Arc<Mutex<GrantLedger>> {
    let mut ledger = GrantLedger::new();
    for capability in capabilities {
        ledger
            .create_grant(subject(), capability.clone(), launch_consent(), WALL_START)
            .expect("fixture grant satisfies its consent class");
    }
    Arc::new(Mutex::new(ledger))
}

fn registry_with(backend: Arc<dyn LaunchBackend>) -> ActionRegistry {
    let mut registry = ActionRegistry::new();
    register_launch_actions(
        &mut registry,
        backend,
        LaunchPolicy::standard(),
        &fixture_bindings(),
    )
    .expect("fixed declaration must register");
    registry
}

fn launch_graph(
    action_type: &str,
    capability: Capability,
    params: Value,
    registry: &ActionRegistry,
) -> Result<Arc<ValidatedGraph>, String> {
    let key = EngineNodeKey::try_new("launch").expect("fixture node key");
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

fn app_capability(identity: &str) -> Capability {
    Capability::OsApplicationLaunch {
        identity: identity.to_string(),
    }
}

#[test]
fn ungranted_request_denies_before_any_backend_call() {
    let harness = Harness::new();
    let fake = Arc::new(FakeLaunchBackend::new());
    let registry = registry_with(Arc::clone(&fake) as Arc<dyn LaunchBackend>);
    let graph = launch_graph(
        ACTION_TYPE_LAUNCH_APPLICATION,
        app_capability("obs-studio"),
        json!({ "identity": "obs-studio" }),
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
fn granted_launches_record_the_exact_targets_per_kind() {
    let harness = Harness::new();
    let cases = [
        (
            ACTION_TYPE_LAUNCH_APPLICATION,
            app_capability("obs-studio"),
            json!({ "identity": "obs-studio" }),
        ),
        (
            ACTION_TYPE_LAUNCH_FILE,
            app_capability("/stage/cue.txt"),
            json!({ "path": "/stage/cue.txt" }),
        ),
        (
            ACTION_TYPE_LAUNCH_URL,
            app_capability("https://example.com/live"),
            json!({ "url": "https://example.com/live" }),
        ),
    ];
    for (action_type, capability, params) in cases {
        let fake = Arc::new(FakeLaunchBackend::new());
        let registry = registry_with(Arc::clone(&fake) as Arc<dyn LaunchBackend>);
        let graph =
            launch_graph(action_type, capability, params, &registry).expect("graph validates");
        let mut runtime = harness.runtime(
            registry,
            ledger_with(&[
                app_capability("obs-studio"),
                app_capability("/stage/cue.txt"),
                app_capability("https://example.com/live"),
            ]),
        );
        let receipt = harness.execute(&mut runtime, &graph);

        assert_eq!(receipt.state.token(), "succeeded", "kind {action_type}");
        assert_eq!(fake.count(), 1, "kind {action_type}");
        assert_eq!(receipt.effects.len(), 1);
        assert_eq!(receipt.effects[0].outcome, "succeeded");
        assert_eq!(receipt.effects[0].action_type, action_type);
    }
    // The exact invocation targets are asserted in the port unit suite;
    // this runtime-level pass pins that all three kinds settle succeeded
    // end-to-end under explicit-selection grants.
}

#[test]
fn revocation_denies_the_next_execution() {
    let harness = Harness::new();
    let fake = Arc::new(FakeLaunchBackend::new());
    let registry = registry_with(Arc::clone(&fake) as Arc<dyn LaunchBackend>);
    let graph = launch_graph(
        ACTION_TYPE_LAUNCH_URL,
        app_capability("https://example.com/live"),
        json!({ "url": "https://example.com/live" }),
        &registry,
    )
    .expect("graph validates");

    let ledger = ledger_with(&[app_capability("https://example.com/live")]);
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
fn a_grant_for_one_target_never_covers_another() {
    let harness = Harness::new();
    let fake = Arc::new(FakeLaunchBackend::new());
    let registry = registry_with(Arc::clone(&fake) as Arc<dyn LaunchBackend>);
    // The node requests an approved-but-ungranted target of the same kind.
    let graph = launch_graph(
        ACTION_TYPE_LAUNCH_APPLICATION,
        app_capability("notepad"),
        json!({ "identity": "notepad" }),
        &registry,
    )
    .expect("declared binding validates");

    let mut runtime = harness.runtime(registry, ledger_with(&[app_capability("obs-studio")]));
    let receipt = harness.execute(&mut runtime, &graph);

    assert_eq!(receipt.state.token(), "failed");
    match &receipt.state {
        TerminalState::Failed {
            reason: FailureReason::CapabilityDenied(DenialReason::NoActiveGrant),
        } => {}
        other => panic!("expected per-target denial, got {other:?}"),
    }
    assert_eq!(fake.count(), 0);
}

#[test]
fn undeclared_identity_rejects_at_graph_validation() {
    let fake = Arc::new(FakeLaunchBackend::new());
    let registry = registry_with(Arc::clone(&fake) as Arc<dyn LaunchBackend>);
    // The registration declares exactly the approved identities; any other
    // target must reject at S3 before grants are even consulted.
    let error = launch_graph(
        ACTION_TYPE_LAUNCH_APPLICATION,
        app_capability("unapproved-app"),
        json!({ "identity": "unapproved-app" }),
        &registry,
    )
    .expect_err("undeclared scope must reject");
    assert!(
        error.contains("capability"),
        "expected scope rejection detail, got: {error}"
    );
    assert_eq!(fake.count(), 0);
}

#[test]
fn foreign_requested_capability_rejects_at_graph_validation() {
    let fake = Arc::new(FakeLaunchBackend::new());
    let registry = registry_with(Arc::clone(&fake) as Arc<dyn LaunchBackend>);
    // Binding a different capability kind to a launch action type must
    // reject at S3.
    let error = launch_graph(
        ACTION_TYPE_LAUNCH_URL,
        Capability::ProcessExecute {
            identity: "obs64.exe".to_string(),
        },
        json!({ "url": "https://example.com/live" }),
        &registry,
    )
    .expect_err("undeclared scope must reject");
    assert!(error.contains("capability"));

    // Sanity: an honest URL binding on the same action type still builds.
    assert!(
        launch_graph(
            ACTION_TYPE_LAUNCH_URL,
            app_capability("https://example.com/live"),
            json!({ "url": "https://example.com/live" }),
            &registry
        )
        .is_ok()
    );
}

#[test]
fn invalid_config_fails_typed_without_touching_the_backend() {
    let harness = Harness::new();
    let fake = Arc::new(FakeLaunchBackend::new());
    let registry = registry_with(Arc::clone(&fake) as Arc<dyn LaunchBackend>);
    let graph = launch_graph(
        ACTION_TYPE_LAUNCH_FILE,
        app_capability("/stage/cue.txt"),
        json!({ "path": "/stage/../secrets.txt" }),
        &registry,
    )
    .expect("graph validates against the declared scope family");

    let mut runtime = harness.runtime(registry, ledger_with(&[app_capability("/stage/cue.txt")]));
    let receipt = harness.execute(&mut runtime, &graph);

    assert_eq!(failed_code(&receipt), "invalid_launch_config");
    assert_eq!(fake.count(), 0);
}

#[test]
fn missing_target_surfaces_an_explicit_typed_error() {
    let harness = Harness::new();
    let fake = Arc::new(FakeLaunchBackend::new());
    fake.set_failure(Some(LaunchError::MissingTarget));
    let registry = registry_with(Arc::clone(&fake) as Arc<dyn LaunchBackend>);
    let graph = launch_graph(
        ACTION_TYPE_LAUNCH_APPLICATION,
        app_capability("obs-studio"),
        json!({ "identity": "obs-studio" }),
        &registry,
    )
    .expect("graph validates");

    let mut runtime = harness.runtime(registry, ledger_with(&[app_capability("obs-studio")]));
    let receipt = harness.execute(&mut runtime, &graph);

    assert_eq!(
        failed_code(&receipt),
        "missing_target",
        "missing targets surface explicitly, never as success"
    );
    assert_eq!(fake.count(), 0);
}

/// The shipped unsupported-platform stub, exercised through the full
/// runtime on every host so the honest explicit error is contract-tested
/// everywhere (CI included).
#[test]
fn unsupported_platform_reports_explicit_typed_error() {
    let harness = Harness::new();
    let registry =
        registry_with(Arc::new(UnsupportedLaunchBackend::new("linux")) as Arc<dyn LaunchBackend>);
    let graph = launch_graph(
        ACTION_TYPE_LAUNCH_URL,
        app_capability("https://example.com/live"),
        json!({ "url": "https://example.com/live" }),
        &registry,
    )
    .expect("graph validates");

    let mut runtime = harness.runtime(
        registry,
        ledger_with(&[app_capability("https://example.com/live")]),
    );
    let receipt = harness.execute(&mut runtime, &graph);

    assert_eq!(
        failed_code(&receipt),
        "unsupported_platform",
        "platform gaps surface explicitly, never as success"
    );
}

/// Real-backend integration check. Doubly gated so nothing ever launches
/// unless a human asks for it on a Windows machine:
/// 1. `#[ignore]` keeps it out of default `cargo test` runs;
/// 2. the `OPENSTREAM_OS_LAUNCH_E2E=1` environment flag must be set even
///    when invoked with `--ignored`.
///
/// Run locally with:
/// `OPENSTREAM_OS_LAUNCH_E2E=1 cargo test -p openstream-os-automation --test launch_integration -- --ignored`
///
/// The launched binary is `%SystemRoot%\System32\hostname.exe`, which
/// prints its output to nulled handles and exits immediately, minimizing
/// side effects on the host.
#[cfg(target_os = "windows")]
#[test]
#[ignore = "real process launch: run explicitly with OPENSTREAM_OS_LAUNCH_E2E=1"]
fn real_windows_backend_launches_harmless_system_binary() {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    if std::env::var("OPENSTREAM_OS_LAUNCH_E2E").ok().as_deref() != Some("1") {
        panic!("refusing to launch a real process without OPENSTREAM_OS_LAUNCH_E2E=1");
    }
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".to_string());
    let exe = PathBuf::from(windir).join("System32").join("hostname.exe");
    assert!(exe.is_file(), "fixture binary missing: {}", exe.display());

    let mut applications = BTreeMap::new();
    applications.insert("hostname".to_string(), exe);
    let backend = openstream_os_automation::WindowsLaunchBackend::new(applications);
    let target = ApplicationTarget::try_new("hostname").expect("valid identity");
    backend
        .launch_application(&target)
        .expect("direct CreateProcess-class spawn succeeds on Windows");
}
