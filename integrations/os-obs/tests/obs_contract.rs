//! Contract tests for the OBS WebSocket v5 integration (issue #13).
//!
//! Everything here runs against the deterministic in-process fake OBS
//! server on an ephemeral loopback port: full authenticated handshakes,
//! every registered action through the real engine runtime (grant
//! intersection, arming gate, typed failures, honest `outcome_unknown`),
//! event-driven live state, and bounded reconnect. No OBS installation is
//! required. The real-OBS check at the bottom is doubly gated.

use openstream_domain::capability::Capability;
use openstream_domain::grant::{
    ConsentEvidence, ConsentKind, DenialReason, GrantLedger, SubjectRef,
};
use openstream_engine::{
    ActionRegistry, Clock as _, ExecuteRequest, FailurePolicy, FailureReason, MessageId, NodeKind,
    RawGraph, RuntimeBuilder, SourceDeviceId, TerminalState, TimeControl, ValidatedGraph,
};
use openstream_os_obs::fake_server::{FakeObsConfig, FakeObsServer};
use openstream_os_obs::protocol::{self, PROTOCOL_MAJOR_SUPPORTED, RPC_VERSION_SUPPORTED};
use openstream_os_obs::session::{
    ConnectionConfig, MAX_RECONNECT_ATTEMPTS, ObsSession, RECONNECT_BASE_DELAY_MS,
    RECONNECT_MAX_DELAY_MS, SessionError,
};
use openstream_os_obs::transport::TungsteniteTransport;
use openstream_os_obs::{
    CODE_CONNECTION_UNAVAILABLE, CODE_INVALID_OBS_CONFIG, CODE_NOT_ARMED, CODE_OBS_REJECTED,
    EVENT_PROGRAM_SCENE_CHANGED, EVENT_RECORD_STATE_CHANGED, EVENT_STREAM_STATE_CHANGED,
    FakeObsController, ObsController, ProbeResult, SessionObsController, probe_endpoint,
    register_obs_actions, validate_name,
};
use openstream_persistence::vault::{CredentialVault, VaultError};
use proptest::prelude::*;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::str::FromStr as _;
use std::sync::{Arc, Mutex};

const WALL_START: i64 = 1_700_000_000_000;
const EXPIRY_MARGIN_MS: i64 = 60_000;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct TestVault {
    entries: Mutex<HashMap<String, String>>,
}

impl TestVault {
    fn with(secret_ref: &str, password: &str) -> Arc<Self> {
        let vault = Arc::new(Self::default());
        vault
            .entries
            .lock()
            .unwrap()
            .insert(secret_ref.to_string(), password.to_string());
        vault
    }
}

impl CredentialVault for TestVault {
    fn store(
        &self,
        secret_ref: &openstream_domain::secret::SecretRef,
        value: &openstream_domain::secret::SecretValue,
    ) -> Result<(), VaultError> {
        self.entries
            .lock()
            .unwrap()
            .insert(secret_ref.as_str().to_string(), value.expose().to_string());
        Ok(())
    }

    fn load(
        &self,
        secret_ref: &openstream_domain::secret::SecretRef,
    ) -> Result<openstream_domain::secret::SecretValue, VaultError> {
        match self.entries.lock().unwrap().get(secret_ref.as_str()) {
            Some(value) => {
                openstream_domain::secret::SecretValue::try_new(value.clone()).map_err(|_| {
                    VaultError::Corrupt {
                        operation: openstream_persistence::vault::VaultOperation::Load,
                    }
                })
            }
            None => Err(VaultError::NotFound {
                operation: openstream_persistence::vault::VaultOperation::Load,
            }),
        }
    }

    fn delete(&self, secret_ref: &openstream_domain::secret::SecretRef) -> Result<(), VaultError> {
        self.entries
            .lock()
            .unwrap()
            .remove(secret_ref.as_str())
            .map(|_| ())
            .ok_or(VaultError::NotFound {
                operation: openstream_persistence::vault::VaultOperation::Delete,
            })
    }
}

const SECRET_REF_NAME: &str = "obs.connection.studio";

fn connection_config(port: u16) -> ConnectionConfig {
    ConnectionConfig {
        host: "127.0.0.1".to_string(),
        port,
        secret_ref: Some(
            openstream_domain::secret::SecretRef::from_str(SECRET_REF_NAME).expect("fixture ref"),
        ),
    }
}

fn open_session(
    server: &FakeObsServer,
    vault: &Arc<TestVault>,
) -> Arc<Mutex<ObsSession<TungsteniteTransport>>> {
    let transport = TungsteniteTransport::connect("127.0.0.1", server.addr().port(), 3_000)
        .expect("loopback connect");
    let config = connection_config(server.addr().port());
    let (session, hello) =
        ObsSession::connect(transport, vault.as_ref(), &config).expect("handshake succeeds");
    assert_eq!(hello.rpc_version, RPC_VERSION_SUPPORTED);
    Arc::new(Mutex::new(session))
}

fn obs_registry(backend: Arc<dyn ObsController>) -> ActionRegistry {
    let mut registry = ActionRegistry::new();
    register_obs_actions(&mut registry, backend).expect("fixed declarations must register");
    registry
}

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

fn action_graph(
    action_type: &str,
    capability: Capability,
    params: Value,
    registry: &ActionRegistry,
) -> Arc<ValidatedGraph> {
    let key = openstream_engine::NodeKey::try_new("obs-node").expect("fixture node key");
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
    Arc::new(ValidatedGraph::build(raw, registry).expect("graph validates"))
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
    ) -> openstream_engine::ExecutionReceipt {
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

fn failed_code(receipt: &openstream_engine::ExecutionReceipt) -> String {
    match &receipt.state {
        TerminalState::Failed {
            reason: FailureReason::AdapterFailed { code },
        } => code.clone(),
        other => panic!("expected typed adapter failure, got {other:?}"),
    }
}

fn denied_reason(receipt: &openstream_engine::ExecutionReceipt) -> String {
    match &receipt.state {
        TerminalState::Failed {
            reason: FailureReason::CapabilityDenied(reason),
        } => reason.to_string(),
        other => panic!("expected capability denial, got {other:?}"),
    }
}

fn authed_server() -> FakeObsServer {
    FakeObsServer::start(FakeObsConfig {
        password: Some("studio-pass-1".to_string()),
        ..FakeObsConfig::default()
    })
}

// ---------------------------------------------------------------------------
// Discovery contract
// ---------------------------------------------------------------------------

#[test]
fn probe_reports_compatible_for_supported_endpoint() {
    let server = authed_server();
    match probe_endpoint(&server.candidate()) {
        ProbeResult::Compatible {
            obs_websocket_version,
            rpc_version,
        } => {
            assert_eq!(rpc_version, RPC_VERSION_SUPPORTED);
            assert!(obs_websocket_version.starts_with('5'));
        }
        other => panic!("expected compatible probe, got {other:?}"),
    }
}

#[test]
fn probe_fails_typed_on_unsupported_rpc_version() {
    let server = FakeObsServer::start(FakeObsConfig {
        rpc_version: 2,
        ..FakeObsConfig::default()
    });
    match probe_endpoint(&server.candidate()) {
        ProbeResult::Incompatible(protocol::ProtocolError::UnsupportedRpcVersion { observed }) => {
            assert_eq!(observed, 2)
        }
        other => panic!("expected typed version failure, got {other:?}"),
    }
}

#[test]
fn probe_fails_typed_on_wrong_protocol_major() {
    let server = FakeObsServer::start(FakeObsConfig {
        version: format!("{}.9.9", PROTOCOL_MAJOR_SUPPORTED - 1),
        ..FakeObsConfig::default()
    });
    match probe_endpoint(&server.candidate()) {
        ProbeResult::Incompatible(protocol::ProtocolError::UnsupportedProtocolVersion {
            observed,
        }) => assert_eq!(observed, "4.9.9"),
        other => panic!("expected typed major failure, got {other:?}"),
    }
}

#[test]
fn probe_reports_unreachable_without_listener() {
    // Port 1 on loopback is never our fake server; expect honest
    // unreachability rather than a hang or a fabricated answer.
    let candidate = openstream_os_obs::DiscoveryCandidate::new("127.0.0.1", 1);
    assert_eq!(probe_endpoint(&candidate), ProbeResult::Unreachable);
}

#[test]
fn discovery_sweep_returns_only_answerable_endpoints() {
    let live = authed_server();
    let dead = openstream_os_obs::DiscoveryCandidate::new("127.0.0.1", 1);
    let results = openstream_os_obs::discover_endpoints(&[dead, live.candidate()]);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0, live.candidate());
    assert!(matches!(&results[0].1, ProbeResult::Compatible { .. }));
}

// ---------------------------------------------------------------------------
// Handshake + secret-vault contract
// ---------------------------------------------------------------------------

#[test]
fn authenticated_handshake_completes_with_vault_secret_and_server_validates_hash() {
    let server = authed_server();
    let vault = TestVault::with(SECRET_REF_NAME, "studio-pass-1");
    let transport =
        TungsteniteTransport::connect("127.0.0.1", server.addr().port(), 3_000).unwrap();
    let (_session, hello) = ObsSession::connect(
        transport,
        vault.as_ref(),
        &connection_config(server.addr().port()),
    )
    .expect("auth handshake succeeds");
    assert_eq!(hello.rpc_version, 1);
    assert!(hello.auth.is_some());
}

#[test]
fn wrong_password_is_refused_typed() {
    let server = authed_server();
    let vault = TestVault::with(SECRET_REF_NAME, "totally-wrong");
    let transport =
        TungsteniteTransport::connect("127.0.0.1", server.addr().port(), 3_000).unwrap();
    let error = ObsSession::connect(
        transport,
        vault.as_ref(),
        &connection_config(server.addr().port()),
    )
    .expect_err("wrong credential must refuse");
    assert_eq!(error, SessionError::AuthRejected);
}

#[test]
fn missing_vault_entry_fails_closed() {
    let server = authed_server();
    let vault = TestVault::with("some.other.entry", "unused");
    let transport =
        TungsteniteTransport::connect("127.0.0.1", server.addr().port(), 3_000).unwrap();
    let error = ObsSession::connect(
        transport,
        vault.as_ref(),
        &connection_config(server.addr().port()),
    )
    .expect_err("missing credential must fail closed");
    assert_eq!(error, SessionError::VaultNotFound);
}

#[test]
fn unsupported_version_connect_fails_typed_before_dispatch() {
    let server = FakeObsServer::start(FakeObsConfig {
        rpc_version: 7,
        ..FakeObsConfig::default()
    });
    let vault = TestVault::with(SECRET_REF_NAME, "x");
    let transport =
        TungsteniteTransport::connect("127.0.0.1", server.addr().port(), 3_000).unwrap();
    let error = ObsSession::connect(
        transport,
        vault.as_ref(),
        &connection_config(server.addr().port()),
    )
    .expect_err("unsupported version must fail closed");
    assert_eq!(error, SessionError::UnsupportedVersion);
}

#[test]
fn identify_frame_never_carries_raw_password() {
    let password = "hunter2-do-not-leak";
    let hash = openstream_os_obs::auth::challenge_response(password, "c2FsdA==", "Y2hhbGxlbmdl");
    assert!(!hash.contains(password));
    let frame = protocol::identify_frame(RPC_VERSION_SUPPORTED, Some(&hash));
    assert!(frame.contains(&hash), "frame carries the derived hash");
    assert!(
        !frame.contains(password),
        "raw password never enters a frame"
    );
    // And a no-auth identify omits the field entirely.
    let plain = protocol::identify_frame(RPC_VERSION_SUPPORTED, None);
    assert!(!plain.contains("authentication"));
}

#[test]
fn unauthenticated_server_handshake_needs_no_secret() {
    let server = FakeObsServer::start(FakeObsConfig::default());
    // An EMPTY vault proves the no-auth path never consults it: any load
    // would fail with VaultNotFound and fail the handshake.
    let vault = Arc::new(TestVault::default());
    let transport =
        TungsteniteTransport::connect("127.0.0.1", server.addr().port(), 3_000).unwrap();
    let config = ConnectionConfig {
        host: "127.0.0.1".to_string(),
        port: server.addr().port(),
        secret_ref: None,
    };
    let (mut session, _) = ObsSession::connect(transport, vault.as_ref(), &config)
        .expect("no-auth handshake succeeds");
    assert!(session.is_connected());
    let response = session.request("GetVersion", None).expect("request works");
    assert!(response.result);
}

// ---------------------------------------------------------------------------
// Actions through the engine runtime (fake backend)
// ---------------------------------------------------------------------------

#[test]
fn all_eight_actions_register_with_honest_posture() {
    let registry = obs_registry(FakeObsController::new());
    let names: Vec<&str> = registry.names().collect();
    assert_eq!(names.len(), 8);
    for name in [
        openstream_os_obs::ACTION_TYPE_OBS_SCENE_SWITCH,
        openstream_os_obs::ACTION_TYPE_OBS_SOURCE_VISIBILITY,
        openstream_os_obs::ACTION_TYPE_OBS_INPUT_MUTE,
        openstream_os_obs::ACTION_TYPE_OBS_STREAM_START,
        openstream_os_obs::ACTION_TYPE_OBS_STREAM_STOP,
        openstream_os_obs::ACTION_TYPE_OBS_RECORD_START,
        openstream_os_obs::ACTION_TYPE_OBS_RECORD_STOP,
        openstream_os_obs::ACTION_TYPE_OBS_REPLAY_SAVE,
    ] {
        let registration = registry.lookup(name).unwrap_or_else(|| panic!("{name}"));
        let expected_scope = if name.starts_with("obs.scene")
            || name.starts_with("obs.source")
            || name.starts_with("obs.input")
        {
            Capability::ObsControlScene
        } else {
            Capability::ObsControlStream
        };
        assert_eq!(registration.scopes(), [expected_scope], "{name}");
        assert!(!registration.idempotency().is_declared(), "{name}");
        assert!(!registration.safe_compensation(), "{name}");
    }
}

#[test]
fn composition_actions_route_through_backend_when_granted() {
    let harness = Harness::new();
    let fake = FakeObsController::new();
    let registry = obs_registry(Arc::clone(&fake) as Arc<dyn ObsController>);
    let mut runtime = harness.runtime(registry, ledger_with(&[Capability::ObsControlScene]));

    let cases: Vec<(&str, Value)> = vec![
        (
            openstream_os_obs::ACTION_TYPE_OBS_SCENE_SWITCH,
            json!({ "scene": "Starting Soon" }),
        ),
        (
            openstream_os_obs::ACTION_TYPE_OBS_SOURCE_VISIBILITY,
            json!({ "scene": "Main", "source": "Webcam", "visible": false }),
        ),
        (
            openstream_os_obs::ACTION_TYPE_OBS_INPUT_MUTE,
            json!({ "input": "Mic", "mute": true }),
        ),
    ];
    for (action_type, params) in cases {
        let graph = action_graph(action_type, Capability::ObsControlScene, params, &{
            let mut r = ActionRegistry::new();
            register_obs_actions(&mut r, Arc::clone(&fake) as Arc<dyn ObsController>).unwrap();
            r
        });
        let receipt = harness.execute(&mut runtime, &graph);
        assert_eq!(receipt.state.token(), "succeeded", "{action_type}");
    }
    assert_eq!(
        fake.invocations(),
        vec![
            openstream_os_obs::ObsInvocation::SceneSwitch("Starting Soon".to_string()),
            openstream_os_obs::ObsInvocation::SourceVisibility {
                scene: "Main".to_string(),
                source: "Webcam".to_string(),
                visible: false,
            },
            openstream_os_obs::ObsInvocation::InputMute {
                input: "Mic".to_string(),
                muted: true,
            },
        ]
    );
}

#[test]
fn stream_class_actions_route_through_backend_when_granted_and_armed() {
    let harness = Harness::new();
    let fake = FakeObsController::new();
    let ledger = ledger_with(&[Capability::ObsControlStream]);
    let registry_fn = || {
        let mut r = ActionRegistry::new();
        register_obs_actions(&mut r, Arc::clone(&fake) as Arc<dyn ObsController>).unwrap();
        r
    };
    let mut runtime = harness.runtime(registry_fn(), Arc::clone(&ledger));

    let cases: Vec<(&str, Value, openstream_os_obs::ObsInvocation)> = vec![
        (
            openstream_os_obs::ACTION_TYPE_OBS_STREAM_START,
            json!({ "armed": true }),
            openstream_os_obs::ObsInvocation::Stream(openstream_os_obs::StreamOp::Start),
        ),
        (
            openstream_os_obs::ACTION_TYPE_OBS_STREAM_STOP,
            json!({ "armed": true }),
            openstream_os_obs::ObsInvocation::Stream(openstream_os_obs::StreamOp::Stop),
        ),
        (
            openstream_os_obs::ACTION_TYPE_OBS_RECORD_START,
            json!({}),
            openstream_os_obs::ObsInvocation::Record(openstream_os_obs::RecordOp::Start),
        ),
        (
            openstream_os_obs::ACTION_TYPE_OBS_RECORD_STOP,
            json!({ "armed": true }),
            openstream_os_obs::ObsInvocation::Record(openstream_os_obs::RecordOp::Stop),
        ),
        (
            openstream_os_obs::ACTION_TYPE_OBS_REPLAY_SAVE,
            json!({}),
            openstream_os_obs::ObsInvocation::ReplaySave,
        ),
    ];
    for (action_type, params, expected_invocation) in cases {
        let graph = action_graph(
            action_type,
            Capability::ObsControlStream,
            params,
            &registry_fn(),
        );
        let receipt = harness.execute(&mut runtime, &graph);
        assert_eq!(receipt.state.token(), "succeeded", "{action_type}");
        let recorded = fake.invocations();
        assert_eq!(
            recorded.last(),
            Some(&expected_invocation),
            "{action_type} routed the exact operation"
        );
    }
    let recorded = fake.invocations();
    assert_eq!(recorded.len(), 5);
    assert_eq!(recorded[4], openstream_os_obs::ObsInvocation::ReplaySave);
}

#[test]
fn unarmed_destructive_requests_refuse_before_any_effect() {
    let harness = Harness::new();
    let fake = FakeObsController::new();
    let registry_fn = || {
        let mut r = ActionRegistry::new();
        register_obs_actions(&mut r, Arc::clone(&fake) as Arc<dyn ObsController>).unwrap();
        r
    };
    let mut runtime = harness.runtime(registry_fn(), ledger_with(&[Capability::ObsControlStream]));

    let unarmed_cases: Vec<(&str, Value, &'static str)> = vec![
        (
            openstream_os_obs::ACTION_TYPE_OBS_STREAM_STOP,
            json!({}),
            CODE_NOT_ARMED,
        ),
        (
            openstream_os_obs::ACTION_TYPE_OBS_STREAM_STOP,
            json!({ "armed": false }),
            CODE_NOT_ARMED,
        ),
        // A wrong-typed arming field is a config error, not an arming
        // refusal: it never counts as confirmation either way.
        (
            openstream_os_obs::ACTION_TYPE_OBS_STREAM_STOP,
            json!({ "armed": "yes" }),
            CODE_INVALID_OBS_CONFIG,
        ),
        (
            openstream_os_obs::ACTION_TYPE_OBS_STREAM_START,
            json!({}),
            CODE_NOT_ARMED,
        ),
        (
            openstream_os_obs::ACTION_TYPE_OBS_STREAM_START,
            json!({ "armed": false }),
            CODE_NOT_ARMED,
        ),
        (
            openstream_os_obs::ACTION_TYPE_OBS_RECORD_STOP,
            json!({}),
            CODE_NOT_ARMED,
        ),
        (
            openstream_os_obs::ACTION_TYPE_OBS_RECORD_STOP,
            json!({ "armed": 1 }),
            CODE_INVALID_OBS_CONFIG,
        ),
    ];
    for (action_type, params, expected_code) in unarmed_cases {
        let graph = action_graph(
            action_type,
            Capability::ObsControlStream,
            params,
            &registry_fn(),
        );
        let receipt = harness.execute(&mut runtime, &graph);
        assert_eq!(failed_code(&receipt), expected_code, "{action_type}");
    }
    assert_eq!(
        fake.count(),
        0,
        "unarmed destructive requests must never reach the backend"
    );

    // Armed start also refuses an unexpected extra field (fail closed).
    let graph = action_graph(
        openstream_os_obs::ACTION_TYPE_OBS_STREAM_START,
        Capability::ObsControlStream,
        json!({ "armed": true, "force": true }),
        &registry_fn(),
    );
    let receipt = harness.execute(&mut runtime, &graph);
    assert_eq!(failed_code(&receipt), CODE_INVALID_OBS_CONFIG);
}

#[test]
fn empty_ledger_denies_before_any_backend_call() {
    let harness = Harness::new();
    let fake = FakeObsController::new();
    let registry_fn = || {
        let mut r = ActionRegistry::new();
        register_obs_actions(&mut r, Arc::clone(&fake) as Arc<dyn ObsController>).unwrap();
        r
    };
    let mut runtime = harness.runtime(registry_fn(), ledger_with(&[]));
    let graph = action_graph(
        openstream_os_obs::ACTION_TYPE_OBS_SCENE_SWITCH,
        Capability::ObsControlScene,
        json!({ "scene": "Main" }),
        &registry_fn(),
    );
    let receipt = harness.execute(&mut runtime, &graph);
    assert_eq!(
        denied_reason(&receipt),
        DenialReason::NoActiveGrant.to_string()
    );
    assert_eq!(fake.count(), 0, "denial fires before any dispatch");
}

#[test]
fn revocation_blocks_the_next_execution() {
    let harness = Harness::new();
    let fake = FakeObsController::new();
    let registry_fn = || {
        let mut r = ActionRegistry::new();
        register_obs_actions(&mut r, Arc::clone(&fake) as Arc<dyn ObsController>).unwrap();
        r
    };
    let ledger = ledger_with(&[Capability::ObsControlScene]);
    let mut runtime = harness.runtime(registry_fn(), Arc::clone(&ledger));
    let graph = action_graph(
        openstream_os_obs::ACTION_TYPE_OBS_SCENE_SWITCH,
        Capability::ObsControlScene,
        json!({ "scene": "Main" }),
        &registry_fn(),
    );
    let receipt = harness.execute(&mut runtime, &graph);
    assert_eq!(receipt.state.token(), "succeeded");
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
fn invalid_configs_fail_typed_without_reaching_the_backend() {
    let harness = Harness::new();
    let fake = FakeObsController::new();
    let registry_fn = || {
        let mut r = ActionRegistry::new();
        register_obs_actions(&mut r, Arc::clone(&fake) as Arc<dyn ObsController>).unwrap();
        r
    };
    let mut runtime = harness.runtime(registry_fn(), ledger_with(&[Capability::ObsControlScene]));
    let invalid_cases: Vec<(&str, Value)> = vec![
        (openstream_os_obs::ACTION_TYPE_OBS_SCENE_SWITCH, json!({})),
        (
            openstream_os_obs::ACTION_TYPE_OBS_SCENE_SWITCH,
            json!({ "scene": "" }),
        ),
        (
            openstream_os_obs::ACTION_TYPE_OBS_SCENE_SWITCH,
            json!({ "scene": " x" }),
        ),
        (
            openstream_os_obs::ACTION_TYPE_OBS_SCENE_SWITCH,
            json!({ "scene": "a*b" }),
        ),
        (
            openstream_os_obs::ACTION_TYPE_OBS_SCENE_SWITCH,
            json!({ "scene": 5 }),
        ),
        (
            openstream_os_obs::ACTION_TYPE_OBS_SCENE_SWITCH,
            json!({ "scene": "a", "extra": 1 }),
        ),
        (
            openstream_os_obs::ACTION_TYPE_OBS_SOURCE_VISIBILITY,
            json!({ "scene": "Main" }),
        ),
        (
            openstream_os_obs::ACTION_TYPE_OBS_INPUT_MUTE,
            json!({ "input": "Mic", "mute": "yes" }),
        ),
    ];
    for (action_type, params) in invalid_cases {
        let graph = action_graph(
            action_type,
            Capability::ObsControlScene,
            params,
            &registry_fn(),
        );
        let receipt = harness.execute(&mut runtime, &graph);
        assert_eq!(
            failed_code(&receipt),
            CODE_INVALID_OBS_CONFIG,
            "{action_type}"
        );
    }
    assert_eq!(
        fake.count(),
        0,
        "invalid configs must never reach the backend"
    );
}

#[test]
fn unregistered_action_types_reject_at_graph_validation() {
    let fake = FakeObsController::new();
    let mut registry = ActionRegistry::new();
    register_obs_actions(&mut registry, Arc::clone(&fake) as Arc<dyn ObsController>)
        .expect("registers");
    let key = openstream_engine::NodeKey::try_new("obs-node").unwrap();
    let mut raw = RawGraph::new(FailurePolicy::Stop);
    raw.add_node(
        key.clone(),
        NodeKind::Action {
            action_type: "obs.scene.explode".to_string(),
            capability: Capability::ObsControlScene,
            params: json!({ "scene": "Main" }),
            deadline_override_ms: None,
        },
    )
    .expect("node grammar ok");
    let raw = raw.entry(key);
    assert!(
        ValidatedGraph::build(raw, &registry).is_err(),
        "unknown action types must fail closed before any dispatch"
    );
    assert_eq!(fake.count(), 0);
}

#[test]
fn foreign_capabilities_fail_defense_in_depth_at_the_port() {
    use openstream_engine::{EffectPort, EffectRequest};
    let fake = FakeObsController::new();
    let scene_port =
        openstream_os_obs::ObsSceneSwitchPort::new(Arc::clone(&fake) as Arc<dyn ObsController>);
    let stream_port =
        openstream_os_obs::ObsReplaySavePort::new(Arc::clone(&fake) as Arc<dyn ObsController>);

    fn outcome_code(port: &impl EffectPort, capability: Capability, action_type: &str) -> String {
        let request = EffectRequest {
            execution_id: openstream_engine::ExecutionId::generate(),
            node_key: openstream_engine::NodeKey::try_new("n").unwrap(),
            action_type: action_type.to_string(),
            capability,
            params: json!({}),
            idempotency_key: "fixture:key".to_string(),
            attempt: 0,
            is_compensation: false,
        };
        match port
            .invoke(request)
            .expect("ports always accept work shape")
        {
            openstream_engine::EffectResponse::Immediate(outcome) => outcome
                .failure_code()
                .expect("fixture expects failure code")
                .to_string(),
            other => panic!("immediate expected, got {other:?}"),
        }
    }

    // Foreign family on the composition port.
    assert_eq!(
        outcome_code(
            &scene_port,
            Capability::ObsControlStream,
            openstream_os_obs::ACTION_TYPE_OBS_REPLAY_SAVE
        ),
        openstream_os_obs::CODE_CAPABILITY_MISMATCH
    );
    // Cross-family drift on the stream-class port.
    assert_eq!(
        outcome_code(
            &stream_port,
            Capability::ObsControlScene,
            openstream_os_obs::ACTION_TYPE_OBS_SCENE_SWITCH
        ),
        openstream_os_obs::CODE_CAPABILITY_MISMATCH
    );
    // Wrong action type inside the stream port refuses too.
    assert_eq!(
        outcome_code(
            &stream_port,
            Capability::ObsControlStream,
            openstream_os_obs::ACTION_TYPE_OBS_STREAM_START
        ),
        openstream_os_obs::CODE_CAPABILITY_MISMATCH
    );
    assert_eq!(fake.count(), 0);
}

// ---------------------------------------------------------------------------
// Actions against the live fake server (real wire, real handshake)
// ---------------------------------------------------------------------------

#[test]
fn granted_scene_switch_round_trips_the_exact_wire_request() {
    let server = authed_server();
    let vault = TestVault::with(SECRET_REF_NAME, "studio-pass-1");
    let session = open_session(&server, &vault);
    let backend = SessionObsController::new(Arc::clone(&session));
    let mut registry = ActionRegistry::new();
    register_obs_actions(&mut registry, Arc::new(backend) as Arc<dyn ObsController>)
        .expect("registers");

    let harness = Harness::new();
    let graph = action_graph(
        openstream_os_obs::ACTION_TYPE_OBS_SCENE_SWITCH,
        Capability::ObsControlScene,
        json!({ "scene": "Starting Soon" }),
        &registry,
    );
    let mut runtime = harness.runtime(registry, ledger_with(&[Capability::ObsControlScene]));
    let receipt = harness.execute(&mut runtime, &graph);
    assert_eq!(receipt.state.token(), "succeeded");

    let requests = server.recorded_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].0, "SetCurrentProgramScene");
    assert_eq!(requests[0].1, json!({ "sceneName": "Starting Soon" }));
}

#[test]
fn source_visibility_resolves_the_scene_item_id_on_the_wire() {
    let server = authed_server();
    server.respond_with(
        "GetSceneItemList",
        json!({ "sceneItems": [
            { "sceneItemId": 7, "sourceName": "Webcam" },
            { "sceneItemId": 3, "sourceName": "Slides" }
        ]}),
    );
    let vault = TestVault::with(SECRET_REF_NAME, "studio-pass-1");
    let session = open_session(&server, &vault);
    let backend = SessionObsController::new(Arc::clone(&session));
    let mut registry = ActionRegistry::new();
    register_obs_actions(&mut registry, Arc::new(backend) as Arc<dyn ObsController>)
        .expect("registers");

    let harness = Harness::new();
    let graph = action_graph(
        openstream_os_obs::ACTION_TYPE_OBS_SOURCE_VISIBILITY,
        Capability::ObsControlScene,
        json!({ "scene": "Main", "source": "Webcam", "visible": false }),
        &registry,
    );
    let mut runtime = harness.runtime(registry, ledger_with(&[Capability::ObsControlScene]));
    let receipt = harness.execute(&mut runtime, &graph);
    assert_eq!(receipt.state.token(), "succeeded");

    let requests = server.recorded_requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].0, "GetSceneItemList");
    assert_eq!(requests[1].0, "SetSceneItemEnabled");
    assert_eq!(
        requests[1].1,
        json!({ "sceneName": "Main", "sceneItemId": 7, "sceneItemEnabled": false })
    );
}

#[test]
fn obs_side_rejection_maps_to_typed_failure() {
    let server = authed_server();
    server.fail_requests_of("StartStream", 500);
    let vault = TestVault::with(SECRET_REF_NAME, "studio-pass-1");
    let session = open_session(&server, &vault);
    let backend = SessionObsController::new(Arc::clone(&session));
    let mut registry = ActionRegistry::new();
    register_obs_actions(&mut registry, Arc::new(backend) as Arc<dyn ObsController>)
        .expect("registers");
    let harness = Harness::new();
    let graph = action_graph(
        openstream_os_obs::ACTION_TYPE_OBS_STREAM_START,
        Capability::ObsControlStream,
        json!({ "armed": true }),
        &registry,
    );
    let mut runtime = harness.runtime(registry, ledger_with(&[Capability::ObsControlStream]));
    let receipt = harness.execute(&mut runtime, &graph);
    assert_eq!(failed_code(&receipt), CODE_OBS_REJECTED);
}

#[test]
fn disconnect_mid_action_journals_outcome_unknown() {
    // The server vanishes WITHOUT answering StopStream: dispatch began,
    // so the only honest terminal is outcome_unknown.
    let server = FakeObsServer::start(FakeObsConfig {
        password: Some("studio-pass-1".to_string()),
        drop_on_request_type: Some("StopStream".to_string()),
        ..FakeObsConfig::default()
    });
    let vault = TestVault::with(SECRET_REF_NAME, "studio-pass-1");
    let session = open_session(&server, &vault);
    let backend = SessionObsController::new(Arc::clone(&session));
    let mut registry = ActionRegistry::new();
    register_obs_actions(&mut registry, Arc::new(backend) as Arc<dyn ObsController>)
        .expect("registers");
    let harness = Harness::new();
    let graph = action_graph(
        openstream_os_obs::ACTION_TYPE_OBS_STREAM_STOP,
        Capability::ObsControlStream,
        json!({ "armed": true }),
        &registry,
    );
    let mut runtime = harness.runtime(registry, ledger_with(&[Capability::ObsControlStream]));
    let receipt = harness.execute(&mut runtime, &graph);
    assert_eq!(
        receipt.state,
        TerminalState::OutcomeUnknown,
        "a lost mid-flight outcome must never be invented as success or failure"
    );
    assert_eq!(receipt.effects.len(), 1);
    assert_eq!(receipt.effects[0].outcome, "unknown");
}

#[test]
fn dead_connection_fails_typed_before_dispatch_and_reconnect_restores_service() {
    let server = authed_server();
    let vault = TestVault::with(SECRET_REF_NAME, "studio-pass-1");
    let session = open_session(&server, &vault);
    let backend = SessionObsController::new(Arc::clone(&session));
    let mut registry = ActionRegistry::new();
    register_obs_actions(&mut registry, Arc::new(backend) as Arc<dyn ObsController>)
        .expect("registers");
    let harness = Harness::new();
    let graph = action_graph(
        openstream_os_obs::ACTION_TYPE_OBS_REPLAY_SAVE,
        Capability::ObsControlStream,
        json!({}),
        &registry,
    );
    let mut runtime = harness.runtime(registry, ledger_with(&[Capability::ObsControlStream]));

    // Kill every live connection behind the session's back. The first
    // dispatch may still hit the wire before the reset is observed, so
    // the honest terminal for it is outcome_unknown; the session then
    // marks itself dead and refuses further work typed.
    server.kill_connections();
    let receipt = harness.execute(&mut runtime, &graph);
    assert!(
        matches!(receipt.state, TerminalState::OutcomeUnknown)
            || failed_code(&receipt) == CODE_CONNECTION_UNAVAILABLE,
        "a killed connection yields unknown or a pre-dispatch refusal, never success"
    );
    let receipt = harness.execute(&mut runtime, &graph);
    assert_eq!(failed_code(&receipt), CODE_CONNECTION_UNAVAILABLE);

    // Bounded reconnect restores a usable session; the same action then
    // succeeds through the NEW connection.
    let config = connection_config(server.addr().port());
    let vault_for_reconnect = Arc::clone(&vault);
    let mut observed_delays: Vec<u64> = Vec::new();
    let reconnected = openstream_os_obs::reconnect_with_policy(
        MAX_RECONNECT_ATTEMPTS,
        || {
            let transport = TungsteniteTransport::connect("127.0.0.1", server.addr().port(), 3_000)
                .map_err(|_| SessionError::PreFlight)?;
            let (session, _) =
                ObsSession::connect(transport, vault_for_reconnect.as_ref(), &config)?;
            Ok(session)
        },
        &mut |delay| observed_delays.push(delay),
    )
    .expect("bounded reconnect succeeds against the accepting server");
    assert!(!observed_delays.is_empty());
    for delay in &observed_delays {
        assert!(
            *delay >= RECONNECT_BASE_DELAY_MS && *delay <= RECONNECT_MAX_DELAY_MS,
            "backoff stays inside its declared bounds"
        );
    }

    let backend = SessionObsController::new(Arc::new(Mutex::new(reconnected)));
    let mut registry2 = ActionRegistry::new();
    register_obs_actions(&mut registry2, Arc::new(backend) as Arc<dyn ObsController>)
        .expect("registers");
    let graph2 = action_graph(
        openstream_os_obs::ACTION_TYPE_OBS_REPLAY_SAVE,
        Capability::ObsControlStream,
        json!({}),
        &registry2,
    );
    let mut runtime2 = harness.runtime(registry2, ledger_with(&[Capability::ObsControlStream]));
    let receipt = harness.execute(&mut runtime2, &graph2);
    assert_eq!(receipt.state.token(), "succeeded");
}

#[test]
fn bounded_backoff_schedule_never_exceeds_its_cap() {
    let delays: Vec<u64> = (0..12).map(openstream_os_obs::backoff_delay_ms).collect();
    assert_eq!(delays[0], RECONNECT_BASE_DELAY_MS);
    assert_eq!(delays[1], RECONNECT_BASE_DELAY_MS * 2);
    for delay in delays {
        assert!(delay <= RECONNECT_MAX_DELAY_MS);
    }
    // The schedule saturates at the cap instead of overflowing.
    assert_eq!(
        openstream_os_obs::backoff_delay_ms(u32::MAX),
        RECONNECT_MAX_DELAY_MS
    );
}

#[test]
fn reconnect_gives_up_after_bounded_attempts_with_honest_error() {
    let mut sleeps: Vec<u64> = Vec::new();
    let attempts = std::cell::Cell::new(0u32);
    let error = openstream_os_obs::reconnect_with_policy(
        MAX_RECONNECT_ATTEMPTS,
        || {
            attempts.set(attempts.get() + 1);
            Err::<(), SessionError>(SessionError::PreFlight)
        },
        &mut |delay| sleeps.push(delay),
    )
    .expect_err("exhausted reconnect surfaces its final error");
    assert_eq!(error, SessionError::PreFlight);
    assert_eq!(attempts.get(), MAX_RECONNECT_ATTEMPTS);
    assert_eq!(
        sleeps.len(),
        usize::try_from(MAX_RECONNECT_ATTEMPTS).unwrap()
    );
}

// ---------------------------------------------------------------------------
// Event-driven live state
// ---------------------------------------------------------------------------

#[test]
fn events_drive_the_live_state_snapshot() {
    let server = FakeObsServer::start(FakeObsConfig::default());
    server.queue_event(
        EVENT_PROGRAM_SCENE_CHANGED,
        Some(&json!({ "sceneName": "Main" })),
    );
    server.queue_event(
        EVENT_STREAM_STATE_CHANGED,
        Some(&json!({ "outputActive": true })),
    );
    server.queue_event(
        EVENT_RECORD_STATE_CHANGED,
        Some(&json!({ "outputActive": false })),
    );
    server.queue_event(
        openstream_os_obs::EVENT_REPLAY_BUFFER_STATE_CHANGED,
        Some(&json!({ "outputActive": true })),
    );
    server.queue_event(
        openstream_os_obs::EVENT_INPUT_MUTE_STATE_CHANGED,
        Some(&json!({ "inputName": "Mic", "inputMuted": true })),
    );

    let vault = TestVault::with(SECRET_REF_NAME, "unused-no-auth");
    let transport =
        TungsteniteTransport::connect("127.0.0.1", server.addr().port(), 3_000).unwrap();
    let config = ConnectionConfig {
        host: "127.0.0.1".to_string(),
        port: server.addr().port(),
        secret_ref: None,
    };
    let (mut session, _) = ObsSession::connect(transport, vault.as_ref(), &config).unwrap();

    // Queued events flush right after Identified; one request proves the
    // flush completed without any sleeps.
    let _ = session.request("GetVersion", None).expect("request works");
    let state = session.live_state().clone();
    assert_eq!(state.program_scene.as_deref(), Some("Main"));
    assert_eq!(state.streaming, Some(true));
    assert_eq!(state.recording, Some(false));
    assert_eq!(state.replay_buffer_active, Some(true));
    assert_eq!(state.input_mutes.get("Mic"), Some(&true));

    // A later scene event updates in place.
    server.queue_event(
        EVENT_PROGRAM_SCENE_CHANGED,
        Some(&json!({ "sceneName": "Interview" })),
    );
    let _ = session.request("GetVersion", None).expect("request works");
    assert_eq!(
        session.live_state().program_scene.as_deref(),
        Some("Interview")
    );
}

// ---------------------------------------------------------------------------
// Property tests
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn no_serialization_path_carries_secret_material(
        password in "[!-~]{8,64}",
    ) {
        // Secret values are never serializable, full stop.
        let secret = openstream_domain::secret::SecretValue::try_new(password.clone())
            .expect("fixture value");
        prop_assert!(serde_json::to_string(&secret).is_err());

        // The derived challenge hash never embeds the password.
        let hash = openstream_os_obs::auth::challenge_response(
            password.as_str(),
            "c2FsdA==",
            "Y2hhbGxlbmdl",
        );
        prop_assert!(!hash.contains(password.as_str()));
        prop_assert!(!openstream_os_obs::auth::challenge_response(
            password.as_str(),
            "",
            ""
        )
        .contains(password.as_str()));

        // Identify frames carry only the hash.
        let frame = protocol::identify_frame(RPC_VERSION_SUPPORTED, Some(hash.as_str()));
        prop_assert!(!frame.contains(password.as_str()));

        // Connection configs serialize without any secret field.
        let config = ConnectionConfig {
            host: "obs.local".to_string(),
            port: 4455,
            secret_ref: Some(
                openstream_domain::secret::SecretRef::from_str(SECRET_REF_NAME).unwrap(),
            ),
        };
        let serialized = serde_json::to_string(&config).expect("config is serializable");
        prop_assert!(!serialized.contains(password.as_str()));

        // Session errors are structural; nothing echoes material.
        let rendered = format!("{:?} {}", SessionError::OutcomeLost, SessionError::AuthRejected);
        prop_assert!(!rendered.contains(password.as_str()));
    }

    #[test]
    fn validate_name_matches_the_declared_predicate(
        name in "[ -~]{0,160}",
    ) {
        let acceptable = !name.is_empty()
            && name.len() <= openstream_os_obs::MAX_OBS_NAME_BYTES
            && name.trim() == name
            && !name.chars().any(|c| c.is_control() || c == '*' || c == '?');
        match validate_name(&name) {
            Ok(()) => prop_assert!(acceptable, "accepted {name:?} but predicate says reject"),
            Err(_) => prop_assert!(!acceptable, "rejected {name:?} but predicate says accept"),
        }
    }
}

// ---------------------------------------------------------------------------
// Real-OBS integration check — doubly gated
// ---------------------------------------------------------------------------

/// Real-OBS contract check. Doubly gated so CI and default test runs never
/// require OBS:
/// 1. `#[ignore]` keeps it out of default `cargo test` runs;
/// 2. the OPENSTREAM_OBS_E2E=1 environment flag must be set even when
///    invoked with `--ignored`.
///
/// Run locally with OBS running (WebSocket server enabled):
/// `OPENSTREAM_OBS_E2E=1 cargo test -p openstream-os-obs --test obs_contract -- --ignored`
///
/// Optional overrides: OPENSTREAM_OBS_HOST / OPENSTREAM_OBS_PORT.
/// Authentication is NOT attempted: run OBS with auth disabled or extend
/// this check with a locally stored secret ref.
#[test]
#[ignore = "requires a real OBS WebSocket endpoint: run explicitly with OPENSTREAM_OBS_E2E=1"]
fn real_obs_endpoint_contract() {
    if std::env::var("OPENSTREAM_OBS_E2E").ok().as_deref() != Some("1") {
        panic!("refusing to contact a real OBS endpoint without OPENSTREAM_OBS_E2E=1");
    }
    let host = std::env::var("OPENSTREAM_OBS_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port: u16 = std::env::var("OPENSTREAM_OBS_PORT")
        .ok()
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(openstream_os_obs::OBS_DEFAULT_PORT);

    let candidate = openstream_os_obs::DiscoveryCandidate::new(&host, port);
    let probe = probe_endpoint(&candidate);
    let (version,) = match &probe {
        ProbeResult::Compatible {
            obs_websocket_version,
            ..
        } => (obs_websocket_version.clone(),),
        other => panic!("real OBS probe failed typed: {other:?}"),
    };
    assert!(version.starts_with('5'), "unexpected version {version}");

    let transport = TungsteniteTransport::connect(&host, port, 3_000)
        .expect("second connect succeeds after probe");
    let vault = NoVaultForRealCheck;
    let config = ConnectionConfig {
        host: host.clone(),
        port,
        secret_ref: None,
    };
    let (mut session, hello) = ObsSession::connect(transport, &vault, &config)
        .expect("handshake with local OBS succeeds (auth disabled)");
    assert_eq!(hello.rpc_version, RPC_VERSION_SUPPORTED);
    let response = session
        .request("GetVersion", None)
        .expect("GetVersion works");
    assert!(response.result);
    assert!(session.is_connected());
}

/// Explicit no-vault stub for the gated real-OBS check; documents that no
/// credential path runs unless a reviewed extension adds one.
#[derive(Debug)]
struct NoVaultForRealCheck;

impl CredentialVault for NoVaultForRealCheck {
    fn store(
        &self,
        _secret_ref: &openstream_domain::secret::SecretRef,
        _value: &openstream_domain::secret::SecretValue,
    ) -> Result<(), VaultError> {
        Err(VaultError::Unsupported { platform: "test" })
    }

    fn load(
        &self,
        _secret_ref: &openstream_domain::secret::SecretRef,
    ) -> Result<openstream_domain::secret::SecretValue, VaultError> {
        Err(VaultError::Unsupported { platform: "test" })
    }

    fn delete(&self, _secret_ref: &openstream_domain::secret::SecretRef) -> Result<(), VaultError> {
        Err(VaultError::Unsupported { platform: "test" })
    }
}
