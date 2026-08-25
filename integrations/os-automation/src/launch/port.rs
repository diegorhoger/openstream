//! Engine port glue: the three launch actions behind `EffectPort`.
//!
//! Dispatch contract (`openstream-engine::port`): the runtime calls a
//! [`LaunchPort`] only after the grant intersection passed and a durable
//! preparation record exists. The port adds defense in depth:
//!
//! - The requested capability kind must be `os.application.launch`;
//!   anything else fails typed without touching a backend.
//! - Parameters revalidate through the exact typed schema
//!   ([`crate::launch::spec`]) at every dispatch; invalid configs fail
//!   typed with [`CODE_INVALID_CONFIG`] before any launch is attempted.
//! - Targets revalidate against the registration policy on every dispatch:
//!   URL schemes must sit inside the allowlist ([`CODE_POLICY_REFUSED`]),
//!   executable-like file targets refuse ([`CODE_POLICY_REFUSED`]) so the
//!   default-handler path can never collapse into direct process
//!   execution.
//! - The target token recomputed from parameters must equal the request's
//!   capability identity exactly ([`CODE_CAPABILITY_MISMATCH`]) — the
//!   granted authority is per exact approved target, and drift fails
//!   closed before spawn (taxonomy §6).
//! - Backend outcomes map onto bounded codes: success,
//!   [`CODE_MISSING_TARGET`] (explicit missing-target error),
//!   [`CODE_UNSUPPORTED_PLATFORM`] (honest explicit error, never silent),
//!   [`CODE_PLATFORM_REFUSED`] (OS refused).
//!
//! Registration posture: each kind registers once with scopes equal to
//! exactly the bindings approved for that kind (typed target policies are
//! validated at registration; an identity outside them rejects at graph
//! validation), idempotency class non-idempotent (re-launching is not
//! safely repeatable, so no automatic retry and no replay after
//! `outcome_unknown`), and no safe compensation (a launched process or
//! opened window cannot be un-launched).

use crate::launch::backend::{LaunchBackend, LaunchError};
use crate::launch::spec::{
    LaunchBinding, LaunchConfigError, LaunchPolicy, parse_application_params, parse_file_params,
    parse_url_params,
};
use openstream_domain::capability::Capability;
use openstream_engine::ConfigError;
use openstream_engine::port::{
    DispatchUnavailable, EffectOutcome, EffectPort, EffectRequest, EffectResponse,
};
use openstream_engine::registry::{ActionRegistration, ActionRegistry, IdempotencyClass};
use std::sync::Arc;

/// Failure code: parameters failed the typed config schema.
pub const CODE_INVALID_CONFIG: &str = "invalid_launch_config";
/// Failure code: no launch backend exists on this platform.
pub const CODE_UNSUPPORTED_PLATFORM: &str = "unsupported_platform";
/// Failure code: the approved target is missing or unresolvable.
pub const CODE_MISSING_TARGET: &str = "missing_target";
/// Failure code: the OS refused the launch operation.
pub const CODE_PLATFORM_REFUSED: &str = "platform_refused";
/// Failure code: the structurally valid target violates the active launch
/// policy (scheme not allowlisted; executable-like file target).
pub const CODE_POLICY_REFUSED: &str = "policy_refused";
/// Failure code: capability kind drift or target-token drift between the
/// request's capability and its parameters (defense in depth; the engine
/// gates the kind already).
pub const CODE_CAPABILITY_MISMATCH: &str = "capability_mismatch";

/// Registration failure for the launch actions: either the registry-level
/// configuration rejected, or one binding failed its typed target policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchRegistrationError {
    /// Registry-level rejection (name grammar, duplicate action names).
    Config(ConfigError),
    /// A binding violated its typed target policy; see the carried reason.
    InvalidBinding(LaunchConfigError),
}

impl core::fmt::Display for LaunchRegistrationError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Config(error) => write!(f, "launch registration rejected: {error}"),
            Self::InvalidBinding(reason) => {
                write!(f, "launch binding rejected: {reason}")
            }
        }
    }
}

impl std::error::Error for LaunchRegistrationError {}

impl From<ConfigError> for LaunchRegistrationError {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

/// The launch action kind a [`LaunchPort`] serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LaunchKind {
    /// Application launch (`os.launch.application`).
    Application,
    /// File open (`os.launch.file`).
    File,
    /// URL open (`os.launch.url`).
    Url,
}

impl LaunchKind {
    /// The registered action type name for this kind.
    #[must_use]
    pub const fn action_type(self) -> &'static str {
        match self {
            Self::Application => crate::launch::spec::ACTION_TYPE_LAUNCH_APPLICATION,
            Self::File => crate::launch::spec::ACTION_TYPE_LAUNCH_FILE,
            Self::Url => crate::launch::spec::ACTION_TYPE_LAUNCH_URL,
        }
    }
}

/// One `EffectPort` adapter binding one backend and policy to one launch
/// action kind.
#[derive(Debug)]
pub struct LaunchPort {
    kind: LaunchKind,
    backend: Arc<dyn LaunchBackend>,
    policy: Arc<LaunchPolicy>,
}

impl LaunchPort {
    /// Binds a kind to a backend and policy. Use
    /// [`register_launch_actions`] for the canonical composition.
    #[must_use]
    pub fn new(
        kind: LaunchKind,
        backend: Arc<dyn LaunchBackend>,
        policy: Arc<LaunchPolicy>,
    ) -> Self {
        Self {
            kind,
            backend,
            policy,
        }
    }

    fn classify(error: LaunchError) -> EffectOutcome {
        match error {
            LaunchError::Unsupported { .. } => failed(CODE_UNSUPPORTED_PLATFORM),
            LaunchError::MissingTarget => failed(CODE_MISSING_TARGET),
            LaunchError::PlatformFailure => failed(CODE_PLATFORM_REFUSED),
        }
    }

    fn refused(reason: LaunchConfigError) -> EffectOutcome {
        match reason {
            LaunchConfigError::PolicySchemeNotAllowed
            | LaunchConfigError::PolicyExecutableTarget => failed(CODE_POLICY_REFUSED),
            _ => failed(CODE_INVALID_CONFIG),
        }
    }
}

fn failed(code: &'static str) -> EffectOutcome {
    EffectOutcome::Failed {
        code: code.to_string(),
    }
}

impl EffectPort for LaunchPort {
    fn invoke(&self, request: EffectRequest) -> Result<EffectResponse, DispatchUnavailable> {
        // Defense in depth: the engine already intersected grants against
        // declared scopes; refuse any non-launch capability outright.
        if request.capability.kind_name() != "os.application.launch" {
            return Ok(EffectResponse::Immediate(failed(CODE_CAPABILITY_MISMATCH)));
        }
        let Capability::OsApplicationLaunch { identity } = &request.capability else {
            return Ok(EffectResponse::Immediate(failed(CODE_CAPABILITY_MISMATCH)));
        };
        let outcome = match self.kind {
            LaunchKind::Application => {
                let target = match parse_application_params(&request.params) {
                    Ok(target) => target,
                    Err(reason) => return Ok(EffectResponse::Immediate(Self::refused(reason))),
                };
                if target.capability_identity() != identity.as_str() {
                    return Ok(EffectResponse::Immediate(failed(CODE_CAPABILITY_MISMATCH)));
                }
                self.backend.launch_application(&target)
            }
            LaunchKind::File => {
                let target = match parse_file_params(&request.params) {
                    Ok(target) => target,
                    Err(reason) => return Ok(EffectResponse::Immediate(Self::refused(reason))),
                };
                if let Err(reason) = self.policy.check_file(&target) {
                    return Ok(EffectResponse::Immediate(Self::refused(reason)));
                }
                if target.capability_identity() != identity.as_str() {
                    return Ok(EffectResponse::Immediate(failed(CODE_CAPABILITY_MISMATCH)));
                }
                self.backend.open_file(&target)
            }
            LaunchKind::Url => {
                let target = match parse_url_params(&request.params) {
                    Ok(target) => target,
                    Err(reason) => return Ok(EffectResponse::Immediate(Self::refused(reason))),
                };
                if let Err(reason) = self.policy.check_url(&target) {
                    return Ok(EffectResponse::Immediate(Self::refused(reason)));
                }
                if target.capability_identity() != identity.as_str() {
                    return Ok(EffectResponse::Immediate(failed(CODE_CAPABILITY_MISMATCH)));
                }
                self.backend.open_url(&target)
            }
        };
        let outcome = match outcome {
            Ok(()) => EffectOutcome::Succeeded,
            Err(error) => Self::classify(error),
        };
        Ok(EffectResponse::Immediate(outcome))
    }
}

/// Registers the three launch action types against an action registry.
///
/// Each binding must carry an already-validated target
/// ([`ApplicationTarget`] / [`FileTarget`] / [`UrlTarget`] constructors and
/// parsers enforce the typed policies); every binding additionally passes
/// the policy checks here so a scheme-outside-allowlist or executable-like
/// file target refuses at authoring time. Scopes declare exactly the
/// approved identities per kind, so nodes requesting any other identity
/// reject at graph validation.
///
/// # Errors
/// [`LaunchRegistrationError::InvalidBinding`] when any binding violates
/// its typed target policy; [`LaunchRegistrationError::Config`]
/// propagation from registration (duplicate action names on repeated
/// calls).
pub fn register_launch_actions(
    registry: &mut ActionRegistry,
    backend: Arc<dyn LaunchBackend>,
    policy: LaunchPolicy,
    bindings: &[LaunchBinding],
) -> Result<(), LaunchRegistrationError> {
    let policy = Arc::new(policy);
    let mut application_scopes = Vec::new();
    let mut file_scopes = Vec::new();
    let mut url_scopes = Vec::new();
    for binding in bindings {
        let scope = binding.capability();
        match binding {
            LaunchBinding::Application(_) => {
                application_scopes.push(scope);
            }
            LaunchBinding::File(target) => {
                policy
                    .check_file(target)
                    .map_err(LaunchRegistrationError::InvalidBinding)?;
                file_scopes.push(scope);
            }
            LaunchBinding::Url(target) => {
                policy
                    .check_url(target)
                    .map_err(LaunchRegistrationError::InvalidBinding)?;
                url_scopes.push(scope);
            }
        }
    }
    for (kind, scopes) in [
        (LaunchKind::Application, application_scopes),
        (LaunchKind::File, file_scopes),
        (LaunchKind::Url, url_scopes),
    ] {
        let registration = ActionRegistration::try_new(
            kind.action_type(),
            scopes,
            IdempotencyClass::NonIdempotent,
            false,
            Arc::new(LaunchPort::new(
                kind,
                Arc::clone(&backend),
                Arc::clone(&policy),
            )),
        )?;
        registry.register(registration)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        CODE_CAPABILITY_MISMATCH, CODE_INVALID_CONFIG, CODE_MISSING_TARGET, CODE_PLATFORM_REFUSED,
        CODE_POLICY_REFUSED, CODE_UNSUPPORTED_PLATFORM, LaunchRegistrationError,
        register_launch_actions,
    };
    use crate::launch::backend::{FakeLaunchBackend, LaunchError, LaunchInvocation};
    use crate::launch::spec::{
        ACTION_TYPE_LAUNCH_APPLICATION, ACTION_TYPE_LAUNCH_FILE, ACTION_TYPE_LAUNCH_URL,
        ApplicationTarget, FileTarget, LaunchBinding, LaunchConfigError, LaunchPolicy, UrlScheme,
        UrlTarget,
    };
    use openstream_domain::capability::Capability;
    use openstream_engine::registry::{ActionRegistry, IdempotencyClass};
    use openstream_engine::{DispatchUnavailable, EffectOutcome, EffectRequest, EffectResponse};
    use serde_json::json;
    use std::sync::Arc;

    fn fixture_request(capability: Capability, params: serde_json::Value) -> EffectRequest {
        EffectRequest {
            execution_id: openstream_engine::ExecutionId::generate(),
            node_key: openstream_engine::NodeKey::try_new("launch-node").expect("fixture key"),
            action_type: ACTION_TYPE_LAUNCH_APPLICATION.to_string(),
            capability,
            params,
            idempotency_key: "fixture:key".to_string(),
            attempt: 0,
            is_compensation: false,
        }
    }

    fn fixture_bindings() -> Vec<LaunchBinding> {
        vec![
            LaunchBinding::Application(ApplicationTarget::try_new("obs-studio").unwrap()),
            LaunchBinding::File(FileTarget::try_new("/stage/cue.txt").unwrap()),
            LaunchBinding::Url(UrlTarget::try_new("https://example.com/live").unwrap()),
        ]
    }

    fn registry_with(backend: Arc<FakeLaunchBackend>) -> ActionRegistry {
        let mut registry = ActionRegistry::new();
        register_launch_actions(
            &mut registry,
            backend,
            LaunchPolicy::standard(),
            &fixture_bindings(),
        )
        .expect("fixture bindings satisfy their policies");
        registry
    }

    fn outcome_code(response: Result<EffectResponse, DispatchUnavailable>) -> String {
        match response.expect("port must always accept work") {
            EffectResponse::Immediate(outcome) => outcome
                .failure_code()
                .expect("fixture expects failure")
                .to_string(),
            EffectResponse::Delayed { .. } => panic!("launch effects settle immediately"),
        }
    }

    #[test]
    fn success_routes_through_backend_per_kind_and_reports_succeeded() {
        let fake = Arc::new(FakeLaunchBackend::new());
        let registry = registry_with(Arc::clone(&fake));
        for (action_type, capability, params, expected_invocation) in [
            (
                ACTION_TYPE_LAUNCH_APPLICATION,
                Capability::OsApplicationLaunch {
                    identity: "obs-studio".to_string(),
                },
                json!({ "identity": "obs-studio" }),
                LaunchInvocation::Application(ApplicationTarget::try_new("obs-studio").unwrap()),
            ),
            (
                ACTION_TYPE_LAUNCH_FILE,
                Capability::OsApplicationLaunch {
                    identity: "/stage/cue.txt".to_string(),
                },
                json!({ "path": "/stage/cue.txt" }),
                LaunchInvocation::File(FileTarget::try_new("/stage/cue.txt").unwrap()),
            ),
            (
                ACTION_TYPE_LAUNCH_URL,
                Capability::OsApplicationLaunch {
                    identity: "https://example.com/live".to_string(),
                },
                json!({ "url": "https://example.com/live" }),
                LaunchInvocation::Url(UrlTarget::try_new("https://example.com/live").unwrap()),
            ),
        ] {
            fake.clear();
            let registration = registry.lookup(action_type).expect("registered");
            let response = registration
                .port()
                .invoke(fixture_request(capability, params))
                .unwrap();
            assert_eq!(
                response,
                EffectResponse::Immediate(EffectOutcome::Succeeded)
            );
            assert_eq!(
                fake.invocations(),
                vec![expected_invocation],
                "kind {action_type}"
            );
        }
    }

    #[test]
    fn foreign_capability_fails_typed_defense_in_depth() {
        let fake = Arc::new(FakeLaunchBackend::new());
        let registry = registry_with(Arc::clone(&fake));
        let port = registry
            .lookup(ACTION_TYPE_LAUNCH_APPLICATION)
            .expect("registered")
            .port();
        let code = outcome_code(port.invoke(fixture_request(
            Capability::NotificationShow,
            json!({ "identity": "obs-studio" }),
        )));
        assert_eq!(code, CODE_CAPABILITY_MISMATCH);
        assert_eq!(fake.count(), 0);
    }

    #[test]
    fn invalid_configs_never_reach_backend() {
        let fake = Arc::new(FakeLaunchBackend::new());
        let registry = registry_with(Arc::clone(&fake));
        let cases = [
            (
                ACTION_TYPE_LAUNCH_APPLICATION,
                vec![
                    json!({ "identity": "" }),
                    json!({ "identity": "has space" }),
                    json!({ "identity": 5 }),
                    json!({}),
                    json!("nope"),
                ],
            ),
            (
                ACTION_TYPE_LAUNCH_FILE,
                vec![
                    json!({ "path": "relative.txt" }),
                    json!({ "path": "C:\\..\\x" }),
                    json!({ "path": "\\\\.\\PhysicalDrive0" }),
                ],
            ),
            (
                ACTION_TYPE_LAUNCH_URL,
                vec![
                    json!({ "url": "not-a-url" }),
                    json!({ "url": "https://user@example.com" }),
                    json!({ "url": "https://ex ample.com" }),
                ],
            ),
        ];
        for (action_type, params_cases) in cases {
            let port = registry.lookup(action_type).expect("registered").port();
            for params in params_cases {
                let code = outcome_code(port.invoke(fixture_request(
                    Capability::OsApplicationLaunch {
                        identity: "obs-studio".to_string(),
                    },
                    params.clone(),
                )));
                assert_eq!(code, CODE_INVALID_CONFIG, "case {action_type} {params}");
            }
        }
        assert_eq!(fake.count(), 0);
    }

    #[test]
    fn policy_violations_fail_typed_before_backend_call() {
        let fake = Arc::new(FakeLaunchBackend::new());
        let registry = registry_with(Arc::clone(&fake));

        // URL under the HTTPS-only standard policy whose node capability
        // was declared by a wider policy: direct-port invocation models the
        // drifted request the engine would never admit through graphs but
        // the port must still refuse.
        let url_port = registry.lookup(ACTION_TYPE_LAUNCH_URL).expect("r").port();
        let code = outcome_code(url_port.invoke(fixture_request(
            Capability::OsApplicationLaunch {
                identity: "http://example.com/live".to_string(),
            },
            json!({ "url": "http://example.com/live" }),
        )));
        assert_eq!(code, CODE_POLICY_REFUSED);

        // Executable-like file target refuses as policy violation.
        let file_port = registry.lookup(ACTION_TYPE_LAUNCH_FILE).expect("r").port();
        let code = outcome_code(file_port.invoke(fixture_request(
            Capability::OsApplicationLaunch {
                identity: "C:\\tools\\run.exe".to_string(),
            },
            json!({ "path": "C:\\tools\\run.exe" }),
        )));
        assert_eq!(code, CODE_POLICY_REFUSED);
        assert_eq!(fake.count(), 0);
    }

    #[test]
    fn target_token_drift_fails_closed() {
        let fake = Arc::new(FakeLaunchBackend::new());
        let registry = registry_with(Arc::clone(&fake));
        let port = registry
            .lookup(ACTION_TYPE_LAUNCH_APPLICATION)
            .expect("registered")
            .port();
        let code = outcome_code(port.invoke(fixture_request(
            Capability::OsApplicationLaunch {
                identity: "other-app".to_string(),
            },
            json!({ "identity": "obs-studio" }),
        )));
        assert_eq!(code, CODE_CAPABILITY_MISMATCH);
        assert_eq!(fake.count(), 0);
    }

    #[test]
    fn backend_errors_map_onto_bounded_codes() {
        let fake = Arc::new(FakeLaunchBackend::new());
        let registry = registry_with(Arc::clone(&fake));
        let port = registry
            .lookup(ACTION_TYPE_LAUNCH_APPLICATION)
            .expect("registered")
            .port();
        let request = || {
            fixture_request(
                Capability::OsApplicationLaunch {
                    identity: "obs-studio".to_string(),
                },
                json!({ "identity": "obs-studio" }),
            )
        };
        for (injected, expected) in [
            (
                LaunchError::Unsupported { platform: "linux" },
                CODE_UNSUPPORTED_PLATFORM,
            ),
            (LaunchError::MissingTarget, CODE_MISSING_TARGET),
            (LaunchError::PlatformFailure, CODE_PLATFORM_REFUSED),
        ] {
            fake.set_failure(Some(injected));
            let code = outcome_code(port.invoke(request()));
            assert_eq!(code, expected, "case {injected:?}");
            assert_eq!(fake.count(), 0, "failed calls record nothing");
        }
    }

    #[test]
    fn registration_declares_honest_posture() {
        let fake = Arc::new(FakeLaunchBackend::new());
        let mut registry = registry_with(Arc::clone(&fake));
        let expected_scopes = [
            (
                ACTION_TYPE_LAUNCH_APPLICATION,
                vec![Capability::OsApplicationLaunch {
                    identity: "obs-studio".to_string(),
                }],
            ),
            (
                ACTION_TYPE_LAUNCH_FILE,
                vec![Capability::OsApplicationLaunch {
                    identity: "/stage/cue.txt".to_string(),
                }],
            ),
            (
                ACTION_TYPE_LAUNCH_URL,
                vec![Capability::OsApplicationLaunch {
                    identity: "https://example.com/live".to_string(),
                }],
            ),
        ];
        for (name, scopes) in expected_scopes {
            let registration = registry.lookup(name).expect("registered");
            assert_eq!(registration.name(), name);
            assert_eq!(registration.scopes(), scopes.as_slice());
            assert_eq!(registration.idempotency(), IdempotencyClass::NonIdempotent);
            assert!(!registration.safe_compensation());
        }

        let duplicate = register_launch_actions(
            &mut registry,
            Arc::new(FakeLaunchBackend::new()),
            LaunchPolicy::standard(),
            &[],
        )
        .unwrap_err();
        assert_eq!(
            duplicate,
            LaunchRegistrationError::Config(openstream_engine::ConfigError::DuplicateActionName)
        );

        // An empty-binding registration into a fresh registry declares all
        // three kinds with zero scopes: every node of these types then
        // rejects at the manifest layer (fail closed, honestly empty).
        let mut fresh = ActionRegistry::new();
        register_launch_actions(
            &mut fresh,
            Arc::new(FakeLaunchBackend::new()),
            LaunchPolicy::standard(),
            &[],
        )
        .expect("empty bindings register");
        for name in [
            ACTION_TYPE_LAUNCH_APPLICATION,
            ACTION_TYPE_LAUNCH_FILE,
            ACTION_TYPE_LAUNCH_URL,
        ] {
            assert!(
                fresh.lookup(name).expect("registered").scopes().is_empty(),
                "{name}"
            );
        }
    }

    #[test]
    fn registration_rejects_policy_violating_bindings_typed() {
        let mut registry = ActionRegistry::new();

        // HTTP binding under the HTTPS-only standard policy refuses.
        let error = register_launch_actions(
            &mut registry,
            Arc::new(FakeLaunchBackend::new()),
            LaunchPolicy::standard(),
            &[LaunchBinding::Url(
                UrlTarget::try_new("http://example.com/live").unwrap(),
            )],
        )
        .unwrap_err();
        assert_eq!(
            error,
            LaunchRegistrationError::InvalidBinding(LaunchConfigError::PolicySchemeNotAllowed)
        );
        assert!(registry.is_empty(), "refusal must leave nothing registered");

        // Executable-like file binding refuses outright.
        let error = register_launch_actions(
            &mut registry,
            Arc::new(FakeLaunchBackend::new()),
            LaunchPolicy::standard(),
            &[LaunchBinding::File(
                FileTarget::try_new("C:\\tools\\run.exe").unwrap(),
            )],
        )
        .unwrap_err();
        assert_eq!(
            error,
            LaunchRegistrationError::InvalidBinding(LaunchConfigError::PolicyExecutableTarget)
        );

        // Widening the policy admits both shapes.
        let mut registry = ActionRegistry::new();
        register_launch_actions(
            &mut registry,
            Arc::new(FakeLaunchBackend::new()),
            LaunchPolicy::new(&[UrlScheme::Http, UrlScheme::Https]),
            &[
                LaunchBinding::Url(UrlTarget::try_new("http://example.com/live").unwrap()),
                LaunchBinding::File(FileTarget::try_new("/opt/app/launcher").unwrap()),
            ],
        )
        .expect("widened policy admits the bindings");
        assert_eq!(registry.len(), 3);
    }
}
