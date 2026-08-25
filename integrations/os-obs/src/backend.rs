//! OBS effect backends behind one object-safe boundary.
//!
//! [`ObsController`] is the port-facing surface every OBS action routes
//! through. Implementations:
//!
//! - [`SessionObsController`] drives one live [`crate::session::ObsSession`]
//!   shared behind a mutex; every method performs a typed wire exchange
//!   and maps honest session failures onto [`ObsFailure`] classes.
//! - [`FakeObsController`] records invocations in memory with scripted
//!   failure injection for deterministic CI. A documented test double,
//!   never a production fallback.
//!
//! Failure honesty: [`ObsFailure::OutcomeLost`] marks effects whose
//! dispatch began but whose result cannot be observed; ports translate it
//! to `EffectOutcome::Unknown`, never to success or failure. Error values
//! carry structural classes only — no scene, source, or input names.

use crate::session::{ObsSession, SessionError};
use crate::spec::{
    InputMute, RecordControl, ReplaySave, SceneSwitch, SourceVisibility, StreamControl,
};
use crate::transport::ObsTransport;
use core::fmt;
use std::sync::{Arc, Mutex};

/// Typed backend failures mapped onto bounded engine codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObsFailure {
    /// No live OBS connection exists. Nothing was attempted.
    NotConnected,
    /// The server refused authentication. Nothing was attempted.
    AuthRejected,
    /// The endpoint speaks an unsupported protocol version.
    UnsupportedVersion,
    /// A protocol violation occurred before dispatch.
    ProtocolViolation,
    /// The vault refused secret resolution during reconnect.
    VaultFailure,
    /// The request was dispatched but its outcome cannot be observed
    /// (disconnect or timeout mid-flight).
    OutcomeLost,
    /// OBS received the request and answered `result: false`.
    ObsRejected,
}

impl fmt::Display for ObsFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotConnected => "no live obs connection",
            Self::AuthRejected => "obs authentication refused",
            Self::UnsupportedVersion => "obs websocket version unsupported",
            Self::ProtocolViolation => "obs protocol violation",
            Self::VaultFailure => "connection secret unavailable",
            Self::OutcomeLost => "effect outcome unobservable",
            Self::ObsRejected => "obs rejected the request",
        })
    }
}

impl std::error::Error for ObsFailure {}

fn map_session(error: SessionError) -> ObsFailure {
    match error {
        SessionError::NotConnected => ObsFailure::NotConnected,
        SessionError::AuthRejected => ObsFailure::AuthRejected,
        SessionError::UnsupportedVersion => ObsFailure::UnsupportedVersion,
        SessionError::ProtocolViolation => ObsFailure::ProtocolViolation,
        SessionError::PreFlight => ObsFailure::NotConnected,
        SessionError::VaultNotFound
        | SessionError::VaultCorrupt
        | SessionError::VaultPlatformFailure
        | SessionError::VaultUnsupported => ObsFailure::VaultFailure,
        SessionError::OutcomeLost => ObsFailure::OutcomeLost,
    }
}

/// Object-safe boundary over OBS control effects. Implementations must be
/// usable from the engine runtime (`Send + Sync`) and must never log
/// payloads, scene names, or secrets.
pub trait ObsController: fmt::Debug + Send + Sync {
    /// Switches the program scene.
    ///
    /// # Errors
    /// [`ObsFailure`] per its variant documentation.
    fn switch_scene(&self, action: &SceneSwitch) -> Result<(), ObsFailure>;

    /// Shows or hides one scene source.
    ///
    /// # Errors
    /// [`ObsFailure`] per its variant documentation.
    fn set_source_visibility(&self, action: &SourceVisibility) -> Result<(), ObsFailure>;

    /// Mutes or unmutes one input.
    ///
    /// # Errors
    /// [`ObsFailure`] per its variant documentation.
    fn set_input_mute(&self, action: &InputMute) -> Result<(), ObsFailure>;

    /// Starts or stops streaming (destructive class; arming is enforced
    /// at the port before this is reached).
    ///
    /// # Errors
    /// [`ObsFailure`] per its variant documentation.
    fn stream_control(&self, action: &StreamControl) -> Result<(), ObsFailure>;

    /// Starts or stops recording (stop is destructive class).
    ///
    /// # Errors
    /// [`ObsFailure`] per its variant documentation.
    fn record_control(&self, action: &RecordControl) -> Result<(), ObsFailure>;

    /// Saves the replay buffer.
    ///
    /// # Errors
    /// [`ObsFailure`] per its variant documentation.
    fn save_replay(&self, _action: &ReplaySave) -> Result<(), ObsFailure> {
        Ok(())
    }
}

/// Real controller bound to one shared live session.
#[derive(Debug)]
pub struct SessionObsController<T: ObsTransport> {
    session: Arc<Mutex<ObsSession<T>>>,
}

impl<T: ObsTransport> SessionObsController<T> {
    /// Binds one live session.
    #[must_use]
    pub fn new(session: Arc<Mutex<ObsSession<T>>>) -> Self {
        Self { session }
    }

    fn run(
        &self,
        request_type: &str,
        data: Option<&serde_json::Value>,
    ) -> Result<serde_json::Value, ObsFailure> {
        let mut session = self.session.lock().map_err(|_| ObsFailure::NotConnected)?;
        let response = session.request(request_type, data).map_err(map_session)?;
        if response.result {
            Ok(response.data.unwrap_or(serde_json::Value::Null))
        } else {
            Err(ObsFailure::ObsRejected)
        }
    }
}

impl<T: ObsTransport> ObsController for SessionObsController<T> {
    fn switch_scene(&self, action: &SceneSwitch) -> Result<(), ObsFailure> {
        let data = serde_json::json!({ "sceneName": action.scene() });
        self.run("SetCurrentProgramScene", Some(&data)).map(|_| ())
    }

    fn set_source_visibility(&self, action: &SourceVisibility) -> Result<(), ObsFailure> {
        // v5 addresses scene items by numeric id inside their named parent
        // scene; resolve the id for the configured source first.
        let list = self.run(
            "GetSceneItemList",
            Some(&serde_json::json!({ "sceneName": action.scene() })),
        )?;
        let item_id = list
            .get("sceneItems")
            .and_then(serde_json::Value::as_array)
            .and_then(|items| {
                items.iter().find_map(|item| {
                    if item.get("sourceName").and_then(serde_json::Value::as_str)
                        == Some(action.source())
                    {
                        item.get("sceneItemId").and_then(serde_json::Value::as_i64)
                    } else {
                        None
                    }
                })
            })
            .ok_or(ObsFailure::ObsRejected)?;
        let data = serde_json::json!({
            "sceneName": action.scene(),
            "sceneItemId": item_id,
            "sceneItemEnabled": action.visible(),
        });
        self.run("SetSceneItemEnabled", Some(&data)).map(|_| ())
    }

    fn set_input_mute(&self, action: &InputMute) -> Result<(), ObsFailure> {
        let data = serde_json::json!({ "inputName": action.input(), "inputMuted": action.muted() });
        self.run("SetInputMute", Some(&data)).map(|_| ())
    }

    fn stream_control(&self, action: &StreamControl) -> Result<(), ObsFailure> {
        let request_type = match action.op() {
            crate::spec::StreamOp::Start => "StartStream",
            crate::spec::StreamOp::Stop => "StopStream",
        };
        self.run(request_type, None).map(|_| ())
    }

    fn record_control(&self, action: &RecordControl) -> Result<(), ObsFailure> {
        let request_type = match action.op() {
            crate::spec::RecordOp::Start => "StartRecord",
            crate::spec::RecordOp::Stop => "StopRecord",
        };
        self.run(request_type, None).map(|_| ())
    }

    fn save_replay(&self, _action: &ReplaySave) -> Result<(), ObsFailure> {
        self.run("SaveReplayBuffer", None).map(|_| ())
    }
}

/// One recorded fake invocation, in call order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObsInvocation {
    /// A scene switch was requested.
    SceneSwitch(String),
    /// A source visibility change was requested.
    SourceVisibility {
        /// Parent scene.
        scene: String,
        /// Target source.
        source: String,
        /// Requested visibility.
        visible: bool,
    },
    /// An input mute change was requested.
    InputMute {
        /// Target input.
        input: String,
        /// Requested mute state.
        muted: bool,
    },
    /// A stream control was requested.
    Stream(crate::spec::StreamOp),
    /// A record control was requested.
    Record(crate::spec::RecordOp),
    /// A replay save was requested.
    ReplaySave,
}

/// Recorded in-memory fake with sticky failure injection for
/// deterministic tests and CI.
#[derive(Debug)]
pub struct FakeObsController {
    invocations: Mutex<Vec<ObsInvocation>>,
    failure: Mutex<Option<ObsFailure>>,
}

impl Default for FakeObsController {
    fn default() -> Self {
        Self {
            invocations: Mutex::new(Vec::new()),
            failure: Mutex::new(None),
        }
    }
}

impl FakeObsController {
    /// Creates an empty fake.
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Every recorded invocation, in order.
    #[must_use]
    pub fn invocations(&self) -> Vec<ObsInvocation> {
        self.invocations.lock().expect("invocation log").clone()
    }

    /// Number of recorded invocations.
    #[must_use]
    pub fn count(&self) -> usize {
        self.invocations.lock().expect("invocation log").len()
    }

    /// Clears the recording.
    pub fn clear(&self) {
        self.invocations.lock().expect("invocation log").clear();
    }

    /// Injects a sticky failure returned by every subsequent call until
    /// cleared with `None`.
    pub fn set_failure(&self, failure: Option<ObsFailure>) {
        *self.failure.lock().expect("failure slot") = failure;
    }

    fn record(&self, invocation: ObsInvocation) -> Result<(), ObsFailure> {
        let injected = *self.failure.lock().expect("failure slot");
        if let Some(failure) = injected {
            return Err(failure);
        }
        self.invocations
            .lock()
            .expect("invocation log")
            .push(invocation);
        Ok(())
    }
}

impl ObsController for FakeObsController {
    fn switch_scene(&self, action: &SceneSwitch) -> Result<(), ObsFailure> {
        self.record(ObsInvocation::SceneSwitch(action.scene().to_string()))
    }

    fn set_source_visibility(&self, action: &SourceVisibility) -> Result<(), ObsFailure> {
        self.record(ObsInvocation::SourceVisibility {
            scene: action.scene().to_string(),
            source: action.source().to_string(),
            visible: action.visible(),
        })
    }

    fn set_input_mute(&self, action: &InputMute) -> Result<(), ObsFailure> {
        self.record(ObsInvocation::InputMute {
            input: action.input().to_string(),
            muted: action.muted(),
        })
    }

    fn stream_control(&self, action: &StreamControl) -> Result<(), ObsFailure> {
        self.record(ObsInvocation::Stream(action.op()))
    }

    fn record_control(&self, action: &RecordControl) -> Result<(), ObsFailure> {
        self.record(ObsInvocation::Record(action.op()))
    }

    fn save_replay(&self, _action: &ReplaySave) -> Result<(), ObsFailure> {
        self.record(ObsInvocation::ReplaySave)
    }
}
