//! End-to-end contract tests for the keyboard shortcut adapter through the
//! real engine runtime: registry registration, graph validation, grant
//! intersection before dispatch, typed config validation at dispatch, and
//! honest unsupported-platform reporting.
//!
//! Everything runs on `FakeClock`; the synthesizer is always the recorded
//! fake or a scripted stub — no test here ever touches a real OS input
//! stack. The opt-in real-Windows check lives at the bottom of this file,
//! doubly gated (`#[ignore]` + environment flag).

use openstream_domain::capability::Capability;
use openstream_domain::grant::{
    ConsentEvidence, ConsentKind, DenialReason, GrantLedger, SubjectRef,
};
use openstream_engine::{
    ActionRegistry, Clock as _, ExecuteRequest, ExecutionReceipt, FailurePolicy, FailureReason,
    MessageId, NodeKind, RawGraph, RuntimeBuilder, SourceDeviceId, TerminalState, TimeControl,
    ValidatedGraph,
};
use openstream_os_automation::{
    FakeKeyboardBackend, KeyboardSynthesizer, UnsupportedKeyboardBackend, parse_shortcut_params,
    register_keyboard_shortcut_action,
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

fn keyboard_consent() -> ConsentEvidence {
    // The taxonomy requires InstallReview + FirstUse for os.keyboard.emit;
    // substitutions fail closed at grant creation.
    ConsentEvidence::try_new(
        vec![ConsentKind::InstallReview, ConsentKind::FirstUse],
        WALL_START,
    )
    .expect("fixture consent")
}

fn ledger_with(capabilities: &[Capability]) -> Arc<Mutex<GrantLedger>> {
    let mut ledger = GrantLedger::new();
    for capability in capabilities {
        ledger
            .create_grant(
                subject(),
                capability.clone(),
                keyboard_consent(),
                WALL_START,
            )
            .expect("fixture grant satisfies its consent class");
    }
    Arc::new(Mutex::new(ledger))
}

fn registry_with(backend: Arc<dyn KeyboardSynthesizer>) -> ActionRegistry {
    let mut registry = ActionRegistry::new();
    register_keyboard_shortcut_action(&mut registry, backend)
        .expect("fixed declaration must register");
    registry
}

fn shortcut_graph(
    capability: Capability,
    params: Value,
    registry: &ActionRegistry,
) -> Result<Arc<ValidatedGraph>, String> {
    let mut raw = RawGraph::new(FailurePolicy::Stop);
    raw.add_node(
        openstream_engine::NodeKey::try_new("kb").expect("fixture node key"),
        NodeKind::Action {
            action_type: openstream_os_automation::ACTION_TYPE_KEYBOARD_SHORTCUT.to_string(),
            capability,
            params,
            deadline_override_ms: None,
        },
    )
    .expect("single node");
    let raw = raw.entry(openstream_engine::NodeKey::try_new("kb").expect("fixture node key"));
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

fn emission_count(fake: &FakeKeyboardBackend) -> usize {
    fake.count()
}

#[test]
fn ungranted_request_denies_before_any_backend_call() {
    let harness = Harness::new();
    let fake = Arc::new(FakeKeyboardBackend::new());
    let registry = registry_with(Arc::clone(&fake) as Arc<dyn KeyboardSynthesizer>);
    let graph = shortcut_graph(
        Capability::OsKeyboardEmit { app: None },
        json!({ "keys": "ctrl+shift+t" }),
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
    assert_eq!(
        emission_count(&fake),
        0,
        "denial must fire before any dispatch"
    );
}

#[test]
fn granted_shortcut_emits_the_exact_parsed_spec() {
    let harness = Harness::new();
    let fake = Arc::new(FakeKeyboardBackend::new());
    let registry = registry_with(Arc::clone(&fake) as Arc<dyn KeyboardSynthesizer>);
    let graph = shortcut_graph(
        Capability::OsKeyboardEmit { app: None },
        json!({ "keys": ["ctrl+k", "s"] }),
        &registry,
    )
    .expect("graph validates");

    let mut runtime = harness.runtime(
        registry,
        ledger_with(&[Capability::OsKeyboardEmit { app: None }]),
    );
    let receipt = harness.execute(&mut runtime, &graph);

    assert_eq!(receipt.state.token(), "succeeded");
    assert_eq!(emission_count(&fake), 1);
    let expected = parse_shortcut_params(&json!({ "keys": ["ctrl+k", "s"] })).expect("parses");
    assert_eq!(fake.emissions(), vec![expected]);
    assert_eq!(receipt.effects.len(), 1);
    assert_eq!(receipt.effects[0].outcome, "succeeded");
    assert_eq!(receipt.effects[0].action_type, "os.keyboard.shortcut");
}

#[test]
fn invalid_config_fails_typed_without_touching_the_backend() {
    let harness = Harness::new();
    let fake = Arc::new(FakeKeyboardBackend::new());
    let registry = registry_with(Arc::clone(&fake) as Arc<dyn KeyboardSynthesizer>);
    let graph = shortcut_graph(
        Capability::OsKeyboardEmit { app: None },
        json!({ "keys": "ctrl+drop+table" }),
        &registry,
    )
    .expect("graph validates");

    let mut runtime = harness.runtime(
        registry,
        ledger_with(&[Capability::OsKeyboardEmit { app: None }]),
    );
    let receipt = harness.execute(&mut runtime, &graph);

    assert_eq!(failed_code(&receipt), "invalid_shortcut_config");
    assert_eq!(emission_count(&fake), 0);
}

#[test]
fn revocation_denies_the_next_execution() {
    let harness = Harness::new();
    let fake = Arc::new(FakeKeyboardBackend::new());
    let registry = registry_with(Arc::clone(&fake) as Arc<dyn KeyboardSynthesizer>);
    let graph = shortcut_graph(
        Capability::OsKeyboardEmit { app: None },
        json!({ "keys": "ctrl+alt+del" }),
        &registry,
    )
    .expect("graph validates");

    let ledger = ledger_with(&[Capability::OsKeyboardEmit { app: None }]);
    let mut runtime = harness.runtime(registry, Arc::clone(&ledger));

    let first = harness.execute(&mut runtime, &graph);
    assert_eq!(first.state.token(), "succeeded");
    assert_eq!(emission_count(&fake), 1);

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
        emission_count(&fake),
        1,
        "revocation must block any further dispatch"
    );
}

#[test]
fn scoped_grant_never_covers_an_unqualified_request() {
    let harness = Harness::new();
    let fake = Arc::new(FakeKeyboardBackend::new());
    let registry = registry_with(Arc::clone(&fake) as Arc<dyn KeyboardSynthesizer>);
    let graph = shortcut_graph(
        Capability::OsKeyboardEmit { app: None },
        json!({ "keys": "ctrl+s" }),
        &registry,
    )
    .expect("graph validates");

    // Grant exists but narrows to app=editor; the request drops that
    // restriction, which would widen authority and must deny.
    let mut runtime = harness.runtime(
        registry,
        ledger_with(&[Capability::OsKeyboardEmit {
            app: Some("editor".to_string()),
        }]),
    );
    let receipt = harness.execute(&mut runtime, &graph);

    assert_eq!(receipt.state.token(), "failed");
    assert_eq!(emission_count(&fake), 0);
}

#[test]
fn window_scoped_request_denies_at_the_manifest_layer() {
    let harness = Harness::new();
    let fake = Arc::new(FakeKeyboardBackend::new());
    let registry = registry_with(Arc::clone(&fake) as Arc<dyn KeyboardSynthesizer>);

    // Scoped delivery does not ship this milestone, and the adapter cannot
    // honestly pre-declare arbitrary app identities: the registration
    // declares exactly the unqualified scope, so an app-qualified node
    // rejects at the engine's manifest intersection BEFORE any dispatch
    // (the port keeps a defensive typed refusal of its own).
    let graph = shortcut_graph(
        Capability::OsKeyboardEmit {
            app: Some("editor".to_string()),
        },
        json!({ "keys": "ctrl+s" }),
        &registry,
    )
    .expect("graph validates against the declared scope family");

    let mut runtime = harness.runtime(
        registry,
        ledger_with(&[Capability::OsKeyboardEmit { app: None }]),
    );
    let receipt = harness.execute(&mut runtime, &graph);

    match &receipt.state {
        TerminalState::Failed {
            reason: FailureReason::CapabilityDenied(DenialReason::NotRequestedByManifest),
        } => {}
        other => panic!("expected manifest-layer denial, got {other:?}"),
    }
    assert_eq!(emission_count(&fake), 0);
}

/// The shipped unsupported-platform stub, exercised through the full
/// runtime on every host so the honest explicit error is contract-tested
/// everywhere (CI included).
#[test]
fn unsupported_platform_reports_explicit_typed_error() {
    let harness = Harness::new();
    let registry = registry_with(
        Arc::new(UnsupportedKeyboardBackend::new("linux")) as Arc<dyn KeyboardSynthesizer>
    );
    let graph = shortcut_graph(
        Capability::OsKeyboardEmit { app: None },
        json!({ "keys": "ctrl+p" }),
        &registry,
    )
    .expect("graph validates");

    let mut runtime = harness.runtime(
        registry,
        ledger_with(&[Capability::OsKeyboardEmit { app: None }]),
    );
    let receipt = harness.execute(&mut runtime, &graph);

    assert_eq!(
        failed_code(&receipt),
        "unsupported_platform",
        "platform gaps surface explicitly, never as success"
    );
}

#[test]
fn foreign_requested_capability_rejects_at_graph_validation() {
    let registry = registry_with(Arc::new(FakeKeyboardBackend::new()));
    // The keyboard registration declares only os.keyboard.emit; binding a
    // process.execute capability to its action type must reject at S3.
    let error = shortcut_graph(
        Capability::ProcessExecute {
            identity: "obs64.exe".to_string(),
        },
        json!({ "keys": "ctrl+r" }),
        &registry,
    )
    .expect_err("undeclared scope must reject");
    assert!(
        error.contains("capability"),
        "expected scope rejection detail, got: {error}"
    );

    // Sanity: the same registry still accepts the honest binding.
    assert!(
        shortcut_graph(
            Capability::OsKeyboardEmit { app: None },
            json!({ "keys": "ctrl+r" }),
            &registry
        )
        .is_ok()
    );
}

/// Real-backend integration check. Doubly gated so nothing ever fires
/// unless a human asks for it on a Windows machine:
/// 1. `#[ignore]` keeps it out of default `cargo test` runs;
/// 2. the `OPENSTREAM_OS_KB_E2E=1` environment flag must be set even when
///    invoked with `--ignored`.
///
/// Run locally with:
/// `OPENSTREAM_OS_KB_E2E=1 cargo test -p openstream-os-automation --test engine_integration -- --ignored`
///
/// The synthesized chord uses F24, which carries no default binding on
/// common desktop configurations, minimizing side effects on the host.
#[cfg(target_os = "windows")]
#[test]
#[ignore = "real SendInput synthesis: run explicitly with OPENSTREAM_OS_KB_E2E=1"]
fn real_windows_backend_synthesizes_harmless_function_key() {
    if std::env::var("OPENSTREAM_OS_KB_E2E").ok().as_deref() != Some("1") {
        panic!("refusing to synthesize real input without OPENSTREAM_OS_KB_E2E=1");
    }
    let backend = openstream_os_automation::WindowsKeyboardBackend::new();
    let spec = parse_shortcut_params(&json!({ "keys": "f24" })).expect("valid spec");
    backend
        .emit(&spec)
        .expect("SendInput synthesis succeeds on Windows");
}
