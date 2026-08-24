//! Engine port glue: the keyboard shortcut action behind `EffectPort`.
//!
//! Dispatch contract (`openstream-engine::port`): the runtime calls
//! [`KeyboardShortcutPort`] only after the grant intersection passed and a
//! durable preparation record exists. The port adds defense-in-depth:
//!
//! - The requested capability must be `os.keyboard.emit` (any qualifier
//!   state); anything else fails typed without touching a backend.
//! - Window-scoped requests (`app=<identity>` qualifiers) fail typed with
//!   [`CODE_WINDOW_SCOPE_UNSUPPORTED`] this milestone: scoped delivery is
//!   not implemented, and silently delivering to the foreground window
//!   would mismatch the granted authority.
//! - Parameters revalidate through the exact typed schema
//!   ([`crate::spec`]) at every dispatch; invalid configs fail typed with
//!   [`CODE_INVALID_CONFIG`] before any synthesis is attempted.
//! - Backend outcomes map onto three bounded codes: success,
//!   [`CODE_UNSUPPORTED_PLATFORM`] (honest explicit error, never silent),
//!   [`CODE_PLATFORM_REFUSED`] (OS refused).
//!
//! Registration declares the adapter honestly: scope `os.keyboard.emit`
//! (unqualified — the manifest layer; per-request narrowing still happens
//! against user grants), idempotency class non-idempotent (re-sending a
//! shortcut is not safely repeatable, so no automatic retry and no replay
//! after `outcome_unknown`), and no safe compensation (sent keys cannot be
//! unsent).

use crate::backend::{KeyboardError, KeyboardSynthesizer};
use crate::spec::{ShortcutSpec, parse_shortcut_params};
use openstream_domain::capability::Capability;
use openstream_engine::ConfigError;
use openstream_engine::port::{
    DispatchUnavailable, EffectOutcome, EffectPort, EffectRequest, EffectResponse,
};
use openstream_engine::registry::{ActionRegistration, ActionRegistry, IdempotencyClass};
use std::sync::Arc;

/// Registered action type name for the keyboard shortcut action.
pub const ACTION_TYPE_KEYBOARD_SHORTCUT: &str = "os.keyboard.shortcut";

/// Failure code: parameters failed the typed config schema.
pub const CODE_INVALID_CONFIG: &str = "invalid_shortcut_config";
/// Failure code: no synthesis backend exists on this platform.
pub const CODE_UNSUPPORTED_PLATFORM: &str = "unsupported_platform";
/// Failure code: the OS refused the synthesis operation.
pub const CODE_PLATFORM_REFUSED: &str = "platform_refused";
/// Failure code: window-scoped delivery is not implemented this milestone;
/// app-qualified requests refuse instead of silently degrading.
pub const CODE_WINDOW_SCOPE_UNSUPPORTED: &str = "window_scope_unsupported";
/// Failure code: the effect carried a capability other than
/// `os.keyboard.emit` (defense in depth; the engine gates this already).
pub const CODE_CAPABILITY_MISMATCH: &str = "capability_mismatch";

/// The `EffectPort` adapter binding one synthesizer backend to the engine.
#[derive(Debug)]
pub struct KeyboardShortcutPort {
    backend: Arc<dyn KeyboardSynthesizer>,
}

impl KeyboardShortcutPort {
    /// Binds a backend. Use [`crate::backend::platform_keyboard_backend`]
    /// for real hosts and [`crate::backend::FakeKeyboardBackend`] in tests
    /// and CI.
    #[must_use]
    pub fn new(backend: Arc<dyn KeyboardSynthesizer>) -> Self {
        Self { backend }
    }

    fn classify(error: KeyboardError) -> EffectOutcome {
        match error {
            KeyboardError::Unsupported { .. } => failed(CODE_UNSUPPORTED_PLATFORM),
            KeyboardError::PlatformFailure => failed(CODE_PLATFORM_REFUSED),
        }
    }
}

fn failed(code: &'static str) -> EffectOutcome {
    EffectOutcome::Failed {
        code: code.to_string(),
    }
}

impl EffectPort for KeyboardShortcutPort {
    fn invoke(&self, request: EffectRequest) -> Result<EffectResponse, DispatchUnavailable> {
        // Defense in depth: the engine already intersected grants against
        // declared scopes; refuse any non-keyboard capability outright.
        if request.capability.kind_name() != "os.keyboard.emit" {
            return Ok(EffectResponse::Immediate(failed(CODE_CAPABILITY_MISMATCH)));
        }
        let Capability::OsKeyboardEmit { app } = &request.capability else {
            return Ok(EffectResponse::Immediate(failed(CODE_CAPABILITY_MISMATCH)));
        };
        // Scoped delivery does not ship this milestone: refuse explicitly
        // instead of silently emitting to the foreground window.
        if app.is_some() {
            return Ok(EffectResponse::Immediate(failed(
                CODE_WINDOW_SCOPE_UNSUPPORTED,
            )));
        }
        // Typed config validation on every dispatch; nothing untyped can
        // reach synthesis.
        let shortcut: ShortcutSpec = match parse_shortcut_params(&request.params) {
            Ok(shortcut) => shortcut,
            Err(_) => return Ok(EffectResponse::Immediate(failed(CODE_INVALID_CONFIG))),
        };
        let outcome = match self.backend.emit(&shortcut) {
            Ok(()) => EffectOutcome::Succeeded,
            Err(error) => Self::classify(error),
        };
        Ok(EffectResponse::Immediate(outcome))
    }
}

/// Registers the keyboard shortcut action type against an action registry.
///
/// Declaration posture (see module docs): unqualified `os.keyboard.emit`
/// scope, [`IdempotencyClass::NonIdempotent`], no safe compensation.
///
/// # Errors
/// [`ConfigError`] propagation from registration (name grammar, duplicate
/// names, internal scopes — none reachable with the fixed declaration, but
/// the typed boundary stays honest for callers).
pub fn register_keyboard_shortcut_action(
    registry: &mut ActionRegistry,
    backend: Arc<dyn KeyboardSynthesizer>,
) -> Result<(), ConfigError> {
    let scopes = vec![Capability::OsKeyboardEmit { app: None }];
    let registration = ActionRegistration::try_new(
        ACTION_TYPE_KEYBOARD_SHORTCUT,
        scopes,
        IdempotencyClass::NonIdempotent,
        false,
        Arc::new(KeyboardShortcutPort::new(backend)),
    )?;
    registry.register(registration)
}

#[cfg(test)]
mod tests {
    use super::{
        ACTION_TYPE_KEYBOARD_SHORTCUT, CODE_CAPABILITY_MISMATCH, CODE_WINDOW_SCOPE_UNSUPPORTED,
        register_keyboard_shortcut_action,
    };
    use crate::backend::FakeKeyboardBackend;
    use openstream_domain::capability::Capability;
    use openstream_engine::registry::{ActionRegistry, IdempotencyClass};
    use openstream_engine::{
        DispatchUnavailable, EffectOutcome, EffectPort, EffectRequest, EffectResponse,
    };
    use serde_json::json;
    use std::sync::Arc;

    fn fixture_request(capability: Capability, params: serde_json::Value) -> EffectRequest {
        EffectRequest {
            execution_id: openstream_engine::ExecutionId::generate(),
            node_key: openstream_engine::NodeKey::try_new("kb-node").expect("fixture key"),
            action_type: ACTION_TYPE_KEYBOARD_SHORTCUT.to_string(),
            capability,
            params,
            idempotency_key: "fixture:key".to_string(),
            attempt: 0,
            is_compensation: false,
        }
    }

    fn outcome_code(response: Result<EffectResponse, DispatchUnavailable>) -> String {
        match response.expect("port must always accept work") {
            EffectResponse::Immediate(outcome) => outcome
                .failure_code()
                .expect("fixture expects failure")
                .to_string(),
            EffectResponse::Delayed { .. } => panic!("keyboard effects settle immediately"),
        }
    }

    #[test]
    fn success_emits_through_backend_and_reports_succeeded() {
        let fake = Arc::new(FakeKeyboardBackend::new());
        let port = super::KeyboardShortcutPort::new(
            Arc::clone(&fake) as Arc<dyn crate::backend::KeyboardSynthesizer>
        );
        let response = port
            .invoke(fixture_request(
                Capability::OsKeyboardEmit { app: None },
                json!({ "keys": "ctrl+shift+t" }),
            ))
            .unwrap();
        match response {
            EffectResponse::Immediate(EffectOutcome::Succeeded) => {}
            other => panic!("expected success, got {other:?}"),
        }
        assert_eq!(fake.count(), 1);
    }

    #[test]
    fn window_scoped_request_refuses_without_emission() {
        let fake = Arc::new(FakeKeyboardBackend::new());
        let port = super::KeyboardShortcutPort::new(
            Arc::clone(&fake) as Arc<dyn crate::backend::KeyboardSynthesizer>
        );
        let code = outcome_code(port.invoke(fixture_request(
            Capability::OsKeyboardEmit {
                app: Some("editor".to_string()),
            },
            json!({ "keys": "ctrl+s" }),
        )));
        assert_eq!(code, CODE_WINDOW_SCOPE_UNSUPPORTED);
        assert_eq!(fake.count(), 0);
    }

    #[test]
    fn foreign_capability_fails_typed_defense_in_depth() {
        let fake = Arc::new(FakeKeyboardBackend::new());
        let port = super::KeyboardShortcutPort::new(
            Arc::clone(&fake) as Arc<dyn crate::backend::KeyboardSynthesizer>
        );
        let code = outcome_code(port.invoke(fixture_request(
            Capability::NotificationShow,
            json!({ "keys": "ctrl+s" }),
        )));
        assert_eq!(code, CODE_CAPABILITY_MISMATCH);
        assert_eq!(fake.count(), 0);
    }

    #[test]
    fn invalid_config_never_reaches_backend() {
        let fake = Arc::new(FakeKeyboardBackend::new());
        let port = super::KeyboardShortcutPort::new(
            Arc::clone(&fake) as Arc<dyn crate::backend::KeyboardSynthesizer>
        );
        for params in [
            json!({ "keys": "ctrl+drop+table" }),
            json!("nope"),
            json!({}),
        ] {
            let code = outcome_code(port.invoke(fixture_request(
                Capability::OsKeyboardEmit { app: None },
                params.clone(),
            )));
            assert_eq!(code, "invalid_shortcut_config", "case {params}");
        }
        assert_eq!(fake.count(), 0);
    }

    #[test]
    fn registration_declares_honest_posture() {
        let mut registry = ActionRegistry::new();
        register_keyboard_shortcut_action(&mut registry, Arc::new(FakeKeyboardBackend::new()))
            .expect("fixed declaration must register");
        let registration = registry
            .lookup(ACTION_TYPE_KEYBOARD_SHORTCUT)
            .expect("registered");
        assert_eq!(registration.name(), ACTION_TYPE_KEYBOARD_SHORTCUT);
        assert_eq!(
            registration.scopes(),
            [Capability::OsKeyboardEmit { app: None }]
        );
        assert_eq!(registration.idempotency(), IdempotencyClass::NonIdempotent);
        assert!(!registration.safe_compensation());

        let duplicate =
            register_keyboard_shortcut_action(&mut registry, Arc::new(FakeKeyboardBackend::new()))
                .unwrap_err();
        assert_eq!(
            duplicate,
            openstream_engine::ConfigError::DuplicateActionName
        );
    }
}
