//! Engine port glue: the eight OBS actions behind `EffectPort`.
//!
//! Dispatch contract (`openstream-engine::port`): the runtime calls a
//! port here only after the grant intersection passed and a durable
//! preparation record exists. The ports add defense in depth:
//!
//! - The requested capability must be exactly the declared one for the
//!   action family (`obs.control.scene` for scene/source/mute authority,
//!   `obs.control.stream` for stream/record/replay authority); anything
//!   else fails typed without touching a backend.
//! - Parameters revalidate through the exact typed schemas
//!   ([`crate::spec`]) at every dispatch; invalid configs fail typed with
//!   [`CODE_INVALID_OBS_CONFIG`] before any effect is attempted.
//! - Destructive operations (stream start, stream stop, record stop)
//!   carry the arming gate: their params must contain exactly
//!   `"armed": true`, validated fail-closed by the schema. An unarmed
//!   destructive request refuses with [`CODE_NOT_ARMED`] before any wire
//!   effect — no accidental live-stream kills.
//! - Honest outcome mapping: backend outcomes map onto bounded codes;
//!   [`ObsFailure::OutcomeLost`] maps to `EffectOutcome::Unknown` so a
//!   disconnect mid-action journals `outcome_unknown` per engine
//!   semantics instead of inventing success or failure.
//!
//! Registration posture (see module docs of each schema): scopes are
//! exactly `obs.control.scene` (three non-destructive composition
//! actions) and `obs.control.stream` (five stream/recording/replay
//! actions, all non-idempotent with no safe compensation).

use crate::backend::{ObsController, ObsFailure};
use crate::spec::{
    InputMute, ObsConfigError, RecordControl, RecordOp, ReplaySave, SceneSwitch, SourceVisibility,
    StreamControl, StreamOp,
};
use openstream_domain::capability::Capability;
use openstream_engine::port::{
    DispatchUnavailable, EffectOutcome, EffectPort, EffectRequest, EffectResponse,
};
use openstream_engine::registry::{ActionRegistry, IdempotencyClass};
use std::sync::Arc;

/// Registered action type: switch the program scene.
pub const ACTION_TYPE_OBS_SCENE_SWITCH: &str = "obs.scene.switch";
/// Registered action type: show/hide one source in a named scene.
pub const ACTION_TYPE_OBS_SOURCE_VISIBILITY: &str = "obs.source.visibility";
/// Registered action type: mute/unmute one input.
pub const ACTION_TYPE_OBS_INPUT_MUTE: &str = "obs.input.mute";
/// Registered action type: start streaming (destructive).
pub const ACTION_TYPE_OBS_STREAM_START: &str = "obs.stream.start";
/// Registered action type: stop streaming (destructive).
pub const ACTION_TYPE_OBS_STREAM_STOP: &str = "obs.stream.stop";
/// Registered action type: start recording.
pub const ACTION_TYPE_OBS_RECORD_START: &str = "obs.record.start";
/// Registered action type: stop recording (destructive).
pub const ACTION_TYPE_OBS_RECORD_STOP: &str = "obs.record.stop";
/// Registered action type: save the replay buffer.
pub const ACTION_TYPE_OBS_REPLAY_SAVE: &str = "obs.replay.save";

/// Failure code: parameters failed the typed schema (including unarmed
/// destructive forms reaching the generic parse path).
pub const CODE_INVALID_OBS_CONFIG: &str = "invalid_obs_config";
/// Failure code: a destructive operation arrived without explicit arming.
pub const CODE_NOT_ARMED: &str = "not_armed";
/// Failure code: no live OBS connection existed; nothing was attempted.
pub const CODE_CONNECTION_UNAVAILABLE: &str = "connection_unavailable";
/// Failure code: OBS refused authentication. Nothing was attempted.
pub const CODE_AUTH_REJECTED: &str = "auth_rejected";
/// Failure code: the endpoint speaks an unsupported protocol version.
pub const CODE_UNSUPPORTED_VERSION: &str = "unsupported_version";
/// Failure code: a protocol violation occurred before dispatch.
pub const CODE_PROTOCOL_VIOLATION: &str = "protocol_violation";
/// Failure code: secret resolution through the vault failed on reconnect.
pub const CODE_VAULT_UNAVAILABLE: &str = "vault_unavailable";
/// Failure code: OBS received and rejected the request.
pub const CODE_OBS_REJECTED: &str = "obs_request_rejected";
/// Failure code: capability outside the adapter's declared authority
/// (defense in depth; the engine gates this already).
pub const CODE_CAPABILITY_MISMATCH: &str = "capability_mismatch";

fn classify(failure: ObsFailure) -> EffectOutcome {
    match failure {
        ObsFailure::NotConnected => failed(CODE_CONNECTION_UNAVAILABLE),
        ObsFailure::AuthRejected => failed(CODE_AUTH_REJECTED),
        ObsFailure::UnsupportedVersion => failed(CODE_UNSUPPORTED_VERSION),
        ObsFailure::ProtocolViolation => failed(CODE_PROTOCOL_VIOLATION),
        ObsFailure::VaultFailure => failed(CODE_VAULT_UNAVAILABLE),
        // Dispatch began but the result is unobservable: honest unknown,
        // never invented success or failure.
        ObsFailure::OutcomeLost => EffectOutcome::Unknown,
        ObsFailure::ObsRejected => failed(CODE_OBS_REJECTED),
    }
}

fn failed(code: &'static str) -> EffectOutcome {
    EffectOutcome::Failed {
        code: code.to_string(),
    }
}

fn config_error_code(error: ObsConfigError) -> &'static str {
    match error {
        ObsConfigError::NotArmed => CODE_NOT_ARMED,
        _ => CODE_INVALID_OBS_CONFIG,
    }
}

fn immediate<T>(
    parsed: Result<T, ObsConfigError>,
    run: impl FnOnce(&T) -> Result<(), ObsFailure>,
) -> EffectResponse {
    match parsed {
        Ok(action) => EffectResponse::Immediate(classify_to_outcome(run(&action))),
        Err(error) => EffectResponse::Immediate(failed(config_error_code(error))),
    }
}

fn classify_to_outcome(result: Result<(), ObsFailure>) -> EffectOutcome {
    match result {
        Ok(()) => EffectOutcome::Succeeded,
        Err(failure) => classify(failure),
    }
}

/// Shared defense-in-depth capability check for the composition actions.
fn require_scene_capability(request: &EffectRequest) -> Option<EffectResponse> {
    if request.capability != Capability::ObsControlScene {
        return Some(EffectResponse::Immediate(failed(CODE_CAPABILITY_MISMATCH)));
    }
    None
}

/// Shared defense-in-depth capability check for the stream-class actions.
fn require_stream_capability(request: &EffectRequest) -> Option<EffectResponse> {
    if request.capability != Capability::ObsControlStream {
        return Some(EffectResponse::Immediate(failed(CODE_CAPABILITY_MISMATCH)));
    }
    None
}

/// Defense-in-depth: a port serves exactly its registered action type.
fn require_action_type(request: &EffectRequest, expected: &str) -> Option<EffectResponse> {
    if request.action_type != expected {
        return Some(EffectResponse::Immediate(failed(CODE_CAPABILITY_MISMATCH)));
    }
    None
}

/// Port for `obs.scene.switch`.
#[derive(Debug)]
pub struct ObsSceneSwitchPort {
    backend: Arc<dyn ObsController>,
}

impl ObsSceneSwitchPort {
    /// Binds a backend.
    #[must_use]
    pub fn new(backend: Arc<dyn ObsController>) -> Self {
        Self { backend }
    }
}

impl EffectPort for ObsSceneSwitchPort {
    fn invoke(&self, request: EffectRequest) -> Result<EffectResponse, DispatchUnavailable> {
        if let Some(refusal) = require_scene_capability(&request) {
            return Ok(refusal);
        }
        if let Some(refusal) = require_action_type(&request, ACTION_TYPE_OBS_SCENE_SWITCH) {
            return Ok(refusal);
        }
        Ok(immediate(SceneSwitch::parse(&request.params), |action| {
            self.backend.switch_scene(action)
        }))
    }
}

/// Port for `obs.source.visibility`.
#[derive(Debug)]
pub struct ObsSourceVisibilityPort {
    backend: Arc<dyn ObsController>,
}

impl ObsSourceVisibilityPort {
    /// Binds a backend.
    #[must_use]
    pub fn new(backend: Arc<dyn ObsController>) -> Self {
        Self { backend }
    }
}

impl EffectPort for ObsSourceVisibilityPort {
    fn invoke(&self, request: EffectRequest) -> Result<EffectResponse, DispatchUnavailable> {
        if let Some(refusal) = require_scene_capability(&request) {
            return Ok(refusal);
        }
        if let Some(refusal) = require_action_type(&request, ACTION_TYPE_OBS_SOURCE_VISIBILITY) {
            return Ok(refusal);
        }
        Ok(immediate(
            SourceVisibility::parse(&request.params),
            |action| self.backend.set_source_visibility(action),
        ))
    }
}

/// Port for `obs.input.mute`.
#[derive(Debug)]
pub struct ObsInputMutePort {
    backend: Arc<dyn ObsController>,
}

impl ObsInputMutePort {
    /// Binds a backend.
    #[must_use]
    pub fn new(backend: Arc<dyn ObsController>) -> Self {
        Self { backend }
    }
}

impl EffectPort for ObsInputMutePort {
    fn invoke(&self, request: EffectRequest) -> Result<EffectResponse, DispatchUnavailable> {
        if let Some(refusal) = require_scene_capability(&request) {
            return Ok(refusal);
        }
        if let Some(refusal) = require_action_type(&request, ACTION_TYPE_OBS_INPUT_MUTE) {
            return Ok(refusal);
        }
        Ok(immediate(InputMute::parse(&request.params), |action| {
            self.backend.set_input_mute(action)
        }))
    }
}

/// Port for the destructive stream start/stop pair. The arming gate runs
/// inside [`StreamControl::parse`] and refuses before any backend call.
#[derive(Debug)]
pub struct ObsStreamControlPort {
    backend: Arc<dyn ObsController>,
}

impl ObsStreamControlPort {
    /// Binds a backend.
    #[must_use]
    pub fn new(backend: Arc<dyn ObsController>) -> Self {
        Self { backend }
    }

    fn serve(&self, op: StreamOp, params: &serde_json::Value) -> EffectResponse {
        immediate(StreamControl::parse(op, params), |action| {
            self.backend.stream_control(action)
        })
    }
}

impl EffectPort for ObsStreamControlPort {
    fn invoke(&self, request: EffectRequest) -> Result<EffectResponse, DispatchUnavailable> {
        if let Some(refusal) = require_stream_capability(&request) {
            return Ok(refusal);
        }
        let op = match request.action_type.as_str() {
            ACTION_TYPE_OBS_STREAM_START => StreamOp::Start,
            ACTION_TYPE_OBS_STREAM_STOP => StreamOp::Stop,
            _ => return Ok(EffectResponse::Immediate(failed(CODE_CAPABILITY_MISMATCH))),
        };
        Ok(self.serve(op, &request.params))
    }
}

/// Port for the record start/stop pair; stop carries the arming gate.
#[derive(Debug)]
pub struct ObsRecordControlPort {
    backend: Arc<dyn ObsController>,
}

impl ObsRecordControlPort {
    /// Binds a backend.
    #[must_use]
    pub fn new(backend: Arc<dyn ObsController>) -> Self {
        Self { backend }
    }

    fn serve(&self, op: RecordOp, params: &serde_json::Value) -> EffectResponse {
        immediate(RecordControl::parse(op, params), |action| {
            self.backend.record_control(action)
        })
    }
}

impl EffectPort for ObsRecordControlPort {
    fn invoke(&self, request: EffectRequest) -> Result<EffectResponse, DispatchUnavailable> {
        if let Some(refusal) = require_stream_capability(&request) {
            return Ok(refusal);
        }
        let op = match request.action_type.as_str() {
            ACTION_TYPE_OBS_RECORD_START => RecordOp::Start,
            ACTION_TYPE_OBS_RECORD_STOP => RecordOp::Stop,
            _ => return Ok(EffectResponse::Immediate(failed(CODE_CAPABILITY_MISMATCH))),
        };
        Ok(self.serve(op, &request.params))
    }
}

/// Port for `obs.replay.save`.
#[derive(Debug)]
pub struct ObsReplaySavePort {
    backend: Arc<dyn ObsController>,
}

impl ObsReplaySavePort {
    /// Binds a backend.
    #[must_use]
    pub fn new(backend: Arc<dyn ObsController>) -> Self {
        Self { backend }
    }
}

impl EffectPort for ObsReplaySavePort {
    fn invoke(&self, request: EffectRequest) -> Result<EffectResponse, DispatchUnavailable> {
        if let Some(refusal) = require_stream_capability(&request) {
            return Ok(refusal);
        }
        if let Some(refusal) = require_action_type(&request, ACTION_TYPE_OBS_REPLAY_SAVE) {
            return Ok(refusal);
        }
        Ok(immediate(ReplaySave::parse(&request.params), |action| {
            self.backend.save_replay(action)
        }))
    }
}

/// Registers all eight OBS action types against an action registry.
///
/// Declaration posture: scene/source/mute actions declare exactly
/// `obs.control.scene`; stream/record/replay actions declare exactly
/// `obs.control.stream`. All are [`IdempotencyClass::NonIdempotent`] with
/// no safe compensation.
///
/// # Errors
/// [`openstream_engine::ConfigError`] propagation from registration (name
/// grammar, duplicate names).
pub fn register_obs_actions(
    registry: &mut ActionRegistry,
    backend: Arc<dyn ObsController>,
) -> Result<(), openstream_engine::ConfigError> {
    use openstream_engine::registry::ActionRegistration as Reg;
    let registrations: Vec<(&'static str, Vec<Capability>, Arc<dyn EffectPort>)> = vec![
        (
            ACTION_TYPE_OBS_SCENE_SWITCH,
            vec![Capability::ObsControlScene],
            Arc::new(ObsSceneSwitchPort::new(Arc::clone(&backend))),
        ),
        (
            ACTION_TYPE_OBS_SOURCE_VISIBILITY,
            vec![Capability::ObsControlScene],
            Arc::new(ObsSourceVisibilityPort::new(Arc::clone(&backend))),
        ),
        (
            ACTION_TYPE_OBS_INPUT_MUTE,
            vec![Capability::ObsControlScene],
            Arc::new(ObsInputMutePort::new(Arc::clone(&backend))),
        ),
        (
            ACTION_TYPE_OBS_STREAM_START,
            vec![Capability::ObsControlStream],
            Arc::new(ObsStreamControlPort::new(Arc::clone(&backend))),
        ),
        (
            ACTION_TYPE_OBS_STREAM_STOP,
            vec![Capability::ObsControlStream],
            Arc::new(ObsStreamControlPort::new(Arc::clone(&backend))),
        ),
        (
            ACTION_TYPE_OBS_RECORD_START,
            vec![Capability::ObsControlStream],
            Arc::new(ObsRecordControlPort::new(Arc::clone(&backend))),
        ),
        (
            ACTION_TYPE_OBS_RECORD_STOP,
            vec![Capability::ObsControlStream],
            Arc::new(ObsRecordControlPort::new(Arc::clone(&backend))),
        ),
        (
            ACTION_TYPE_OBS_REPLAY_SAVE,
            vec![Capability::ObsControlStream],
            Arc::new(ObsReplaySavePort::new(Arc::clone(&backend))),
        ),
    ];
    for (name, scopes, port) in registrations {
        let registration =
            Reg::try_new(name, scopes, IdempotencyClass::NonIdempotent, false, port)?;
        registry.register(registration)?;
    }
    Ok(())
}
