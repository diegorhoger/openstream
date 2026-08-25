//! Event-driven live-state tracking for one OBS connection.
//!
//! [`LiveState`] is the typed, redaction-safe snapshot of the OBS session
//! surfaced to callers: current program scene name, streaming flag,
//! recording flag, replay-buffer active flag, and per-input mute state.
//! It is updated exclusively by consuming typed wire events
//! ([`apply_event`]); nothing here polls OBS or invents state. Unknown
//! event types are ignored (forward-compatible additive minors per
//! `PROTOCOL.md`).
//!
//! Evidence rules: names are held for functional state only and never
//! enter journal records or failure codes.

use crate::protocol::EventMessage;
use std::collections::BTreeMap;

/// Event type carried by obs-websocket when the program scene changes.
pub const EVENT_PROGRAM_SCENE_CHANGED: &str = "CurrentProgramSceneChanged";
/// Event type carried when the streaming output starts or stops.
pub const EVENT_STREAM_STATE_CHANGED: &str = "StreamStateChanged";
/// Event type carried when the recording output starts or stops.
pub const EVENT_RECORD_STATE_CHANGED: &str = "RecordStateChanged";
/// Event type carried when the replay buffer arms or disarms.
pub const EVENT_REPLAY_BUFFER_STATE_CHANGED: &str = "ReplayBufferStateChanged";
/// Event type carried when an input's mute state changes.
pub const EVENT_INPUT_MUTE_STATE_CHANGED: &str = "InputMuteStateChanged";

/// Typed live snapshot of one OBS session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LiveState {
    /// Current program scene name, when known.
    pub program_scene: Option<String>,
    /// Whether the streaming output is active.
    pub streaming: Option<bool>,
    /// Whether the recording output is active.
    pub recording: Option<bool>,
    /// Whether the replay buffer is armed/active.
    pub replay_buffer_active: Option<bool>,
    /// Per-input mute state for inputs whose events were observed.
    pub input_mutes: BTreeMap<String, bool>,
}

impl LiveState {
    /// Applies one parsed event, updating tracked fields. Returns true
    /// when the event changed some tracked state.
    #[must_use]
    pub fn apply_event(&mut self, event: &EventMessage) -> bool {
        match event.event_type.as_str() {
            EVENT_PROGRAM_SCENE_CHANGED => match string_field(&event.data, "sceneName") {
                Some(scene) if self.program_scene.as_deref() != Some(scene.as_str()) => {
                    self.program_scene = Some(scene);
                    true
                }
                _ => false,
            },
            EVENT_STREAM_STATE_CHANGED => {
                self.apply_flag(&event.data, "outputActive", |state| &mut state.streaming)
            }
            EVENT_RECORD_STATE_CHANGED => {
                self.apply_flag(&event.data, "outputActive", |state| &mut state.recording)
            }
            EVENT_REPLAY_BUFFER_STATE_CHANGED => {
                self.apply_flag(&event.data, "outputActive", |state| {
                    &mut state.replay_buffer_active
                })
            }
            EVENT_INPUT_MUTE_STATE_CHANGED => {
                let Some(input) = string_field(&event.data, "inputName") else {
                    return false;
                };
                let Some(muted) = data_bool(&event.data, "inputMuted") else {
                    return false;
                };
                if self.input_mutes.get(&input) == Some(&muted) {
                    return false;
                }
                self.input_mutes.insert(input, muted);
                true
            }
            _ => false,
        }
    }

    fn apply_flag(
        &mut self,
        data: &serde_json::Value,
        field: &str,
        select: impl Fn(&mut Self) -> &mut Option<bool>,
    ) -> bool {
        let Some(flag) = data.get(field).and_then(serde_json::Value::as_bool) else {
            return false;
        };
        let slot = select(self);
        if *slot == Some(flag) {
            return false;
        }
        *slot = Some(flag);
        true
    }
}

fn string_field(data: &serde_json::Value, field: &str) -> Option<String> {
    data.get(field)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

fn data_bool(data: &serde_json::Value, field: &str) -> Option<bool> {
    data.get(field).and_then(serde_json::Value::as_bool)
}
