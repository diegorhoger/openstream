//! Engine port glue: the media transport and volume actions behind
//! `EffectPort`.
//!
//! Dispatch contract (`openstream-engine::port`): the runtime calls a
//! port here only after the grant intersection passed and a durable
//! preparation record exists. The ports add defense in depth:
//!
//! - [`MediaTransportPort`] serves `os.media.transport`. The requested
//!   capability must be exactly `os.media.emit`; anything else fails typed
//!   without touching a backend.
//! - [`AudioVolumePort`] serves `os.audio.volume`. The requested
//!   capability must be `audio.control` scoped to exactly the declared
//!   master device (`device=master`, the default render endpoint). Any
//!   other device scope fails typed with
//!   [`CODE_DEVICE_SCOPE_UNSUPPORTED`] — a scoped request never silently
//!   degrades into global/master control ("no silent global fallback").
//! - Parameters revalidate through the exact typed schemas
//!   ([`crate::media::spec`]) at every dispatch; invalid configs fail
//!   typed with [`CODE_INVALID_MEDIA_CONFIG`] /
//!   [`CODE_INVALID_VOLUME_CONFIG`] before any effect is attempted.
//! - Backend outcomes map onto bounded codes: success,
//!   [`CODE_UNSUPPORTED_PLATFORM`] (honest explicit error, never silent),
//!   [`CODE_PLATFORM_REFUSED`] (OS refused).
//!
//! Registration declares both adapters honestly: scopes are exactly the
//! unqualified `os.media.emit` and the single `audio.control:device=master`
//! scope; idempotency class non-idempotent for both (re-sending a transport
//! command repeats it and volume steps accumulate, so no automatic retry
//! and no replay after `outcome_unknown`), and no safe compensation (a
//! sent transport command or an applied volume step cannot be undone).

use crate::media::backend::{MediaDeviceController, MediaError};
use crate::media::spec::{MediaCommand, VolumeOperation, parse_media_params, parse_volume_params};
use openstream_domain::capability::Capability;
use openstream_engine::ConfigError;
use openstream_engine::port::{
    DispatchUnavailable, EffectOutcome, EffectPort, EffectRequest, EffectResponse,
};
use openstream_engine::registry::{ActionRegistration, ActionRegistry, IdempotencyClass};
use std::sync::Arc;

/// The one device scope this milestone declares for volume control: the OS
/// default render endpoint. Every other device scope refuses explicitly.
pub const MASTER_DEVICE_SCOPE: &str = "master";

/// Registered action type name for the media transport action.
pub const ACTION_TYPE_MEDIA_TRANSPORT: &str = "os.media.transport";

/// Registered action type name for the audio volume action.
pub const ACTION_TYPE_AUDIO_VOLUME: &str = "os.audio.volume";

/// Failure code: media-transport parameters failed the typed schema.
pub const CODE_INVALID_MEDIA_CONFIG: &str = "invalid_media_config";
/// Failure code: volume parameters failed the typed schema.
pub const CODE_INVALID_VOLUME_CONFIG: &str = "invalid_volume_config";
/// Failure code: no media/volume backend exists on this platform.
pub const CODE_UNSUPPORTED_PLATFORM: &str = "unsupported_platform";
/// Failure code: the OS refused the operation.
pub const CODE_PLATFORM_REFUSED: &str = "platform_refused";
/// Failure code: the effect carried a capability outside the adapter's
/// declared authority (defense in depth; the engine gates this already).
pub const CODE_CAPABILITY_MISMATCH: &str = "capability_mismatch";
/// Failure code: the requested device scope exists grammatically but is
/// not addressable by this milestone's backend; the request refuses
/// instead of silently controlling the master scope.
pub const CODE_DEVICE_SCOPE_UNSUPPORTED: &str = "device_scope_unsupported";

/// The `EffectPort` adapter binding one backend to the transport action.
#[derive(Debug)]
pub struct MediaTransportPort {
    backend: Arc<dyn MediaDeviceController>,
}

impl MediaTransportPort {
    /// Binds a backend. Use [`crate::media::backend::platform_media_backend`]
    /// for real hosts and [`crate::media::backend::FakeMediaBackend`] in
    /// tests and CI.
    #[must_use]
    pub fn new(backend: Arc<dyn MediaDeviceController>) -> Self {
        Self { backend }
    }

    fn classify(error: MediaError) -> EffectOutcome {
        match error {
            MediaError::Unsupported { .. } => failed(CODE_UNSUPPORTED_PLATFORM),
            MediaError::PlatformFailure => failed(CODE_PLATFORM_REFUSED),
        }
    }
}

impl EffectPort for MediaTransportPort {
    fn invoke(&self, request: EffectRequest) -> Result<EffectResponse, DispatchUnavailable> {
        // Defense in depth: the engine already intersected grants against
        // declared scopes; refuse any non-media capability outright.
        if request.capability.kind_name() != "os.media.emit" {
            return Ok(EffectResponse::Immediate(failed(CODE_CAPABILITY_MISMATCH)));
        }
        let Capability::OsMediaEmit = &request.capability else {
            return Ok(EffectResponse::Immediate(failed(CODE_CAPABILITY_MISMATCH)));
        };
        // Typed config validation on every dispatch; nothing untyped can
        // reach synthesis.
        let command: MediaCommand = match parse_media_params(&request.params) {
            Ok(command) => command,
            Err(_) => {
                return Ok(EffectResponse::Immediate(failed(CODE_INVALID_MEDIA_CONFIG)));
            }
        };
        let outcome = match self.backend.send_transport(&command) {
            Ok(()) => EffectOutcome::Succeeded,
            Err(error) => Self::classify(error),
        };
        Ok(EffectResponse::Immediate(outcome))
    }
}

/// The `EffectPort` adapter binding one backend to the volume action.
#[derive(Debug)]
pub struct AudioVolumePort {
    backend: Arc<dyn MediaDeviceController>,
}

impl AudioVolumePort {
    /// Binds a backend. Use [`register_media_actions`] for the canonical
    /// composition.
    #[must_use]
    pub fn new(backend: Arc<dyn MediaDeviceController>) -> Self {
        Self { backend }
    }

    fn classify(error: MediaError) -> EffectOutcome {
        match error {
            MediaError::Unsupported { .. } => failed(CODE_UNSUPPORTED_PLATFORM),
            MediaError::PlatformFailure => failed(CODE_PLATFORM_REFUSED),
        }
    }
}

impl EffectPort for AudioVolumePort {
    fn invoke(&self, request: EffectRequest) -> Result<EffectResponse, DispatchUnavailable> {
        // Defense in depth: refuse any non-audio-control capability
        // outright.
        if request.capability.kind_name() != "audio.control" {
            return Ok(EffectResponse::Immediate(failed(CODE_CAPABILITY_MISMATCH)));
        }
        let Capability::AudioControl { device } = &request.capability else {
            return Ok(EffectResponse::Immediate(failed(CODE_CAPABILITY_MISMATCH)));
        };
        // Named-device match enforced fail closed: only the declared
        // master scope is addressable this milestone. A differently named
        // device never silently routes to master.
        if device.as_str() != MASTER_DEVICE_SCOPE {
            return Ok(EffectResponse::Immediate(failed(
                CODE_DEVICE_SCOPE_UNSUPPORTED,
            )));
        }
        let operation: VolumeOperation = match parse_volume_params(&request.params) {
            Ok(operation) => operation,
            Err(_) => {
                return Ok(EffectResponse::Immediate(failed(
                    CODE_INVALID_VOLUME_CONFIG,
                )));
            }
        };
        let outcome = match self.backend.adjust_volume(&operation) {
            Ok(()) => EffectOutcome::Succeeded,
            Err(error) => Self::classify(error),
        };
        Ok(EffectResponse::Immediate(outcome))
    }
}

fn failed(code: &'static str) -> EffectOutcome {
    EffectOutcome::Failed {
        code: code.to_string(),
    }
}

/// Registers the media transport and volume action types against an action
/// registry.
///
/// Declaration posture (see module docs): scope `os.media.emit` for the
/// transport action, exactly one scope `audio.control:device=master` for
/// the volume action, [`IdempotencyClass::NonIdempotent`], no safe
/// compensation.
///
/// # Errors
/// [`ConfigError`] propagation from registration (name grammar, duplicate
/// names — none reachable with the fixed declarations, but the typed
/// boundary stays honest for callers).
pub fn register_media_actions(
    registry: &mut ActionRegistry,
    backend: Arc<dyn MediaDeviceController>,
) -> Result<(), ConfigError> {
    let registrations = [
        (
            ACTION_TYPE_MEDIA_TRANSPORT,
            vec![Capability::OsMediaEmit],
            Arc::new(MediaTransportPort::new(Arc::clone(&backend))) as Arc<dyn EffectPort>,
        ),
        (
            ACTION_TYPE_AUDIO_VOLUME,
            vec![Capability::AudioControl {
                device: MASTER_DEVICE_SCOPE.to_string(),
            }],
            Arc::new(AudioVolumePort::new(Arc::clone(&backend))) as Arc<dyn EffectPort>,
        ),
    ];
    for (name, scopes, port) in registrations {
        let registration = ActionRegistration::try_new(
            name,
            scopes,
            IdempotencyClass::NonIdempotent,
            false,
            port,
        )?;
        registry.register(registration)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        ACTION_TYPE_AUDIO_VOLUME, ACTION_TYPE_MEDIA_TRANSPORT, AudioVolumePort,
        CODE_CAPABILITY_MISMATCH, CODE_DEVICE_SCOPE_UNSUPPORTED, CODE_INVALID_MEDIA_CONFIG,
        CODE_INVALID_VOLUME_CONFIG, MASTER_DEVICE_SCOPE, MediaTransportPort,
        register_media_actions,
    };
    use crate::media::backend::{
        FakeMediaBackend, MediaDeviceController, MediaError, MediaInvocation,
        UnsupportedMediaBackend,
    };
    use crate::media::spec::{MediaCommand, StepDirection, VolumeOperation};
    use openstream_domain::capability::Capability;
    use openstream_engine::registry::{ActionRegistry, IdempotencyClass};
    use openstream_engine::{
        DispatchUnavailable, EffectOutcome, EffectPort, EffectRequest, EffectResponse,
    };
    use serde_json::json;
    use std::sync::Arc;

    const MASTER: &str = MASTER_DEVICE_SCOPE;

    fn fixture_request(capability: Capability, params: serde_json::Value) -> EffectRequest {
        EffectRequest {
            execution_id: openstream_engine::ExecutionId::generate(),
            node_key: openstream_engine::NodeKey::try_new("media-node").expect("fixture key"),
            action_type: ACTION_TYPE_MEDIA_TRANSPORT.to_string(),
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
            EffectResponse::Delayed { .. } => panic!("media effects settle immediately"),
        }
    }

    fn transport_port(fake: &Arc<FakeMediaBackend>) -> MediaTransportPort {
        MediaTransportPort::new(Arc::clone(fake) as Arc<dyn MediaDeviceController>)
    }

    fn volume_port(fake: &Arc<FakeMediaBackend>) -> AudioVolumePort {
        AudioVolumePort::new(Arc::clone(fake) as Arc<dyn MediaDeviceController>)
    }

    #[test]
    fn transport_commands_route_through_the_backend_and_report_succeeded() {
        let fake = Arc::new(FakeMediaBackend::new());
        let port = transport_port(&fake);
        for command in [
            MediaCommand::PlayPause,
            MediaCommand::NextTrack,
            MediaCommand::PreviousTrack,
        ] {
            fake.clear();
            let response = port
                .invoke(fixture_request(
                    Capability::OsMediaEmit,
                    json!({ "action": command.as_str() }),
                ))
                .unwrap();
            assert_eq!(
                response,
                EffectResponse::Immediate(EffectOutcome::Succeeded),
                "command {command}"
            );
            assert_eq!(
                fake.invocations(),
                vec![MediaInvocation::Transport(command)]
            );
        }
    }

    #[test]
    fn volume_operations_route_through_the_backend_and_report_succeeded() {
        let fake = Arc::new(FakeMediaBackend::new());
        let port = volume_port(&fake);
        for (params, expected) in [
            (
                json!({ "operation": "up", "steps": 3 }),
                VolumeOperation::Up { steps: 3 },
            ),
            (
                json!({ "operation": "down" }),
                VolumeOperation::Down { steps: 1 },
            ),
            (
                json!({ "operation": "toggle_mute" }),
                VolumeOperation::ToggleMute,
            ),
        ] {
            fake.clear();
            let response = port
                .invoke(fixture_request(
                    Capability::AudioControl {
                        device: MASTER.to_string(),
                    },
                    params,
                ))
                .unwrap();
            assert_eq!(
                response,
                EffectResponse::Immediate(EffectOutcome::Succeeded)
            );
            assert_eq!(fake.invocations(), vec![MediaInvocation::Volume(expected)]);
        }
    }

    #[test]
    fn foreign_capabilities_fail_typed_defense_in_depth_on_both_ports() {
        let fake = Arc::new(FakeMediaBackend::new());
        let transport = transport_port(&fake);
        let volume = volume_port(&fake);

        let code = outcome_code(transport.invoke(fixture_request(
            Capability::NotificationShow,
            json!({ "action": "play_pause" }),
        )));
        assert_eq!(code, CODE_CAPABILITY_MISMATCH);

        let code = outcome_code(volume.invoke(fixture_request(
            Capability::NotificationShow,
            json!({ "operation": "up" }),
        )));
        assert_eq!(code, CODE_CAPABILITY_MISMATCH);

        // Cross-family drift: a media capability bound to the volume port
        // (and vice versa) refuses too.
        let code = outcome_code(volume.invoke(fixture_request(
            Capability::OsMediaEmit,
            json!({ "operation": "up" }),
        )));
        assert_eq!(code, CODE_CAPABILITY_MISMATCH);
        let code = outcome_code(transport.invoke(fixture_request(
            Capability::AudioControl {
                device: MASTER.to_string(),
            },
            json!({ "action": "play_pause" }),
        )));
        assert_eq!(code, CODE_CAPABILITY_MISMATCH);

        assert_eq!(fake.count(), 0);
    }

    #[test]
    fn non_master_device_scope_refuses_without_global_fallback() {
        let fake = Arc::new(FakeMediaBackend::new());
        let port = volume_port(&fake);
        for device in ["headphones", "speakers", "Master", "default"] {
            let code = outcome_code(port.invoke(fixture_request(
                Capability::AudioControl {
                    device: device.to_string(),
                },
                json!({ "operation": "up" }),
            )));
            assert_eq!(code, CODE_DEVICE_SCOPE_UNSUPPORTED, "device {device:?}");
        }
        assert_eq!(
            fake.count(),
            0,
            "scoped requests must never degrade into master control"
        );
    }

    #[test]
    fn invalid_configs_never_reach_the_backend() {
        let fake = Arc::new(FakeMediaBackend::new());
        let transport = transport_port(&fake);
        let volume = volume_port(&fake);

        let media_cases = [
            json!({ "action": "skip_ahead" }),
            json!({ "action": 5 }),
            json!({}),
            json!("nope"),
            json!({ "action": "next_track", "extra": 1 }),
        ];
        for params in media_cases {
            let code = outcome_code(
                transport.invoke(fixture_request(Capability::OsMediaEmit, params.clone())),
            );
            assert_eq!(code, CODE_INVALID_MEDIA_CONFIG, "case {params}");
        }

        let volume_cases = [
            json!({ "operation": "louder" }),
            json!({ "operation": "up", "steps": 0 }),
            json!({ "operation": "up", "steps": 99 }),
            json!({ "operation": "up", "steps": -1 }),
            json!({ "operation": "toggle_mute", "steps": 2 }),
            json!({ "operation": "up", "extra": true }),
            json!({ "step": 1 }),
        ];
        for params in volume_cases {
            let code = outcome_code(volume.invoke(fixture_request(
                Capability::AudioControl {
                    device: MASTER.to_string(),
                },
                params.clone(),
            )));
            assert_eq!(code, CODE_INVALID_VOLUME_CONFIG, "case {params}");
        }
        assert_eq!(fake.count(), 0);
    }

    #[test]
    fn backend_errors_map_onto_bounded_codes() {
        let fake = Arc::new(FakeMediaBackend::new());
        let transport = transport_port(&fake);
        let volume = volume_port(&fake);
        let transport_request =
            || fixture_request(Capability::OsMediaEmit, json!({ "action": "play_pause" }));
        let volume_request = || {
            fixture_request(
                Capability::AudioControl {
                    device: MASTER.to_string(),
                },
                json!({ "operation": "toggle_mute" }),
            )
        };
        for (injected, expected) in [
            (
                MediaError::Unsupported { platform: "linux" },
                super::CODE_UNSUPPORTED_PLATFORM,
            ),
            (MediaError::PlatformFailure, super::CODE_PLATFORM_REFUSED),
        ] {
            fake.set_failure(Some(injected));
            let code = outcome_code(transport.invoke(transport_request()));
            assert_eq!(code, expected, "transport case {injected:?}");
            let code = outcome_code(volume.invoke(volume_request()));
            assert_eq!(code, expected, "volume case {injected:?}");
            assert_eq!(fake.count(), 0, "failed calls record nothing");
        }
    }

    #[test]
    fn unsupported_stub_reports_explicit_typed_error_through_ports() {
        let backend = Arc::new(UnsupportedMediaBackend::new("macos"));
        let transport =
            MediaTransportPort::new(Arc::clone(&backend) as Arc<dyn MediaDeviceController>);
        let volume = AudioVolumePort::new(Arc::clone(&backend) as Arc<dyn MediaDeviceController>);
        let code = outcome_code(transport.invoke(fixture_request(
            Capability::OsMediaEmit,
            json!({ "action": "play_pause" }),
        )));
        assert_eq!(code, super::CODE_UNSUPPORTED_PLATFORM);
        let code = outcome_code(volume.invoke(fixture_request(
            Capability::AudioControl {
                device: MASTER.to_string(),
            },
            json!({ "operation": "toggle_mute" }),
        )));
        assert_eq!(code, super::CODE_UNSUPPORTED_PLATFORM);
    }

    #[test]
    fn registration_declares_honest_posture() {
        let mut registry = ActionRegistry::new();
        register_media_actions(&mut registry, Arc::new(FakeMediaBackend::new()))
            .expect("fixed declarations must register");

        let transport = registry
            .lookup(ACTION_TYPE_MEDIA_TRANSPORT)
            .expect("registered");
        assert_eq!(transport.name(), ACTION_TYPE_MEDIA_TRANSPORT);
        assert_eq!(transport.scopes(), [Capability::OsMediaEmit]);
        assert_eq!(transport.idempotency(), IdempotencyClass::NonIdempotent);
        assert!(!transport.safe_compensation());

        let volume = registry
            .lookup(ACTION_TYPE_AUDIO_VOLUME)
            .expect("registered");
        assert_eq!(volume.name(), ACTION_TYPE_AUDIO_VOLUME);
        assert_eq!(
            volume.scopes(),
            [Capability::AudioControl {
                device: MASTER.to_string()
            }]
        );
        assert_eq!(volume.idempotency(), IdempotencyClass::NonIdempotent);
        assert!(!volume.safe_compensation());

        let duplicate =
            register_media_actions(&mut registry, Arc::new(FakeMediaBackend::new())).unwrap_err();
        assert_eq!(
            duplicate,
            openstream_engine::ConfigError::DuplicateActionName
        );
    }

    #[test]
    fn step_direction_tokens_round_trip_through_the_typed_constructor() {
        use crate::media::spec::MAX_VOLUME_STEPS;

        assert_eq!(
            VolumeOperation::new_step(StepDirection::Up, 5).unwrap(),
            parse_volume_fixture(json!({ "operation": "up", "steps": 5 }))
        );
        assert_eq!(
            VolumeOperation::new_step(StepDirection::Down, MAX_VOLUME_STEPS + 1).unwrap_err(),
            crate::media::spec::MediaConfigError::StepsOutOfRange
        );
    }

    fn parse_volume_fixture(params: serde_json::Value) -> VolumeOperation {
        crate::media::spec::parse_volume_params(&params).expect("fixture parses")
    }
}
