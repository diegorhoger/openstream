//! Typed media transport and volume configuration, validated fail closed.
//!
//! Two bounded parameter schemas back the media/volume adapters:
//!
//! - Transport ([`parse_media_params`]): a JSON object carrying exactly one
//!   `action` field whose string value comes from the closed vocabulary
//!   `play_pause` | `next_track` | `previous_track` — the standard
//!   OS-level media-transport commands.
//! - Volume ([`parse_volume_params`]): a JSON object carrying exactly one
//!   required `operation` field (`up` | `down` | `toggle_mute`) plus an
//!   optional bounded integer `steps` field (1..=[`MAX_VOLUME_STEPS`],
//!   default 1) admitted only for the stepping operations.
//!
//! Validation rules (all rejections structural; rejected input never enters
//! an error value):
//!
//! - Tokens are ASCII lowercase `[a-z_]` only — no whitespace, control
//!   bytes, wildcards, punctuation, or mixed case — bounded by
//!   [`MAX_MEDIA_TOKEN_BYTES`].
//! - Unknown object fields reject; wrong JSON types reject; off-vocabulary
//!   tokens reject; `steps` outside 1..=[`MAX_VOLUME_STEPS`] rejects and
//!   `steps` together with `toggle_mute` rejects.
//!
//! The same validators run at authoring time ([`MediaCommand`] /
//! [`VolumeOperation`] typed constructors), at graph-registration
//! pre-validation, and again per dispatch inside the ports, so no untyped
//! path can reach a backend effect.

use core::fmt;
use serde_json::Value;

/// Maximum number of volume steps carried by one dispatch.
pub const MAX_VOLUME_STEPS: u8 = 10;

/// Maximum byte length of one action/operation token (`previous_track` is
/// the longest vocabulary entry).
pub const MAX_MEDIA_TOKEN_BYTES: usize = 16;

/// Media-transport command. Closed v1 vocabulary; additions are additive
/// minors of this schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaCommand {
    /// Toggle play/pause on the active media session.
    PlayPause,
    /// Skip to the next track.
    NextTrack,
    /// Return to the previous track.
    PreviousTrack,
}

impl MediaCommand {
    /// Canonical lowercase token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PlayPause => "play_pause",
            Self::NextTrack => "next_track",
            Self::PreviousTrack => "previous_track",
        }
    }

    fn parse(token: &str) -> Option<Self> {
        match token {
            "play_pause" => Some(Self::PlayPause),
            "next_track" => Some(Self::NextTrack),
            "previous_track" => Some(Self::PreviousTrack),
            _ => None,
        }
    }

    /// Parses one validated transport command from an already
    /// hygiened token (the exact check [`parse_media_params`] applies).
    ///
    /// # Errors
    /// [`MediaConfigError::ActionInvalidChar`] when the token carries bytes
    /// outside `[a-z_]` or exceeds [`MAX_MEDIA_TOKEN_BYTES`];
    /// [`MediaConfigError::ActionUnknownToken`] when the well-formed token
    /// is off vocabulary.
    pub fn try_parse(token: &str) -> Result<Self, MediaConfigError> {
        validate_token(token, MediaConfigError::ActionInvalidChar)?;
        Self::parse(token).ok_or(MediaConfigError::ActionUnknownToken)
    }
}

impl fmt::Display for MediaCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Direction of one relative volume change on the granted device scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StepDirection {
    /// Raise the volume.
    Up,
    /// Lower the volume.
    Down,
}

impl StepDirection {
    /// Canonical lowercase token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
        }
    }

    fn parse(token: &str) -> Option<Self> {
        match token {
            "up" => Some(Self::Up),
            "down" => Some(Self::Down),
            _ => None,
        }
    }
}

impl fmt::Display for StepDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One volume operation on the granted device scope: a bounded relative
/// step or a mute-state toggle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VolumeOperation {
    /// Raise the volume by `steps` discrete increments.
    Up {
        /// Number of increments; 1..=[`MAX_VOLUME_STEPS`].
        steps: u8,
    },
    /// Lower the volume by `steps` discrete increments.
    Down {
        /// Number of increments; 1..=[`MAX_VOLUME_STEPS`].
        steps: u8,
    },
    /// Toggle the mute state.
    ToggleMute,
}

impl VolumeOperation {
    /// Builds a validated relative step.
    ///
    /// # Errors
    /// [`MediaConfigError::StepsOutOfRange`] when `steps` is zero or above
    /// [`MAX_VOLUME_STEPS`].
    pub fn new_step(direction: StepDirection, steps: u8) -> Result<Self, MediaConfigError> {
        if steps == 0 || steps > MAX_VOLUME_STEPS {
            return Err(MediaConfigError::StepsOutOfRange);
        }
        Ok(match direction {
            StepDirection::Up => Self::Up { steps },
            StepDirection::Down => Self::Down { steps },
        })
    }

    /// The operation kind token (`up` | `down` | `toggle_mute`).
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Up { .. } => "up",
            Self::Down { .. } => "down",
            Self::ToggleMute => "toggle_mute",
        }
    }
}

impl fmt::Display for VolumeOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Up { steps } => write!(f, "up:{steps}"),
            Self::Down { steps } => write!(f, "down:{steps}"),
            Self::ToggleMute => f.write_str("toggle_mute"),
        }
    }
}

fn validate_token(token: &str, invalid: MediaConfigError) -> Result<(), MediaConfigError> {
    if token.is_empty()
        || token.len() > MAX_MEDIA_TOKEN_BYTES
        || !token.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
    {
        return Err(invalid);
    }
    Ok(())
}

/// Validates media-transport action parameters against the declared config
/// schema.
///
/// Accepted shape:
///
/// ```json
/// {"action": "play_pause"}
/// ```
///
/// Anything else — wrong JSON type, extra fields, off-vocabulary tokens —
/// rejects fail closed with a structural reason.
///
/// # Errors
/// [`MediaConfigError`] variants; rejected input never enters errors.
pub fn parse_media_params(params: &Value) -> Result<MediaCommand, MediaConfigError> {
    let Value::Object(fields) = params else {
        return Err(MediaConfigError::NotAnObject);
    };
    if fields.len() != 1 || !fields.contains_key("action") {
        return Err(MediaConfigError::UnexpectedField);
    }
    let Value::String(raw) = fields
        .get("action")
        .unwrap_or_else(|| unreachable!("checked above"))
    else {
        return Err(MediaConfigError::ActionWrongType);
    };
    MediaCommand::try_parse(raw)
}

/// Validates volume-operation parameters against the declared config
/// schema.
///
/// Accepted shapes:
///
/// ```json
/// {"operation": "up", "steps": 3}
/// {"operation": "down"}
/// {"operation": "toggle_mute"}
/// ```
///
/// `steps` defaults to one increment, must sit inside
/// 1..=[`MAX_VOLUME_STEPS`], and rejects outright when paired with
/// `toggle_mute`.
///
/// # Errors
/// [`MediaConfigError`] variants; rejected input never enters errors.
pub fn parse_volume_params(params: &Value) -> Result<VolumeOperation, MediaConfigError> {
    let Value::Object(fields) = params else {
        return Err(MediaConfigError::NotAnObject);
    };
    let mut operation: Option<&str> = None;
    let mut steps: Option<u64> = None;
    for (key, value) in fields {
        match key.as_str() {
            "operation" => {
                let Value::String(raw) = value else {
                    return Err(MediaConfigError::OperationWrongType);
                };
                operation = Some(raw.as_str());
            }
            "steps" => {
                let Value::Number(number) = value else {
                    return Err(MediaConfigError::StepsWrongType);
                };
                steps = Some(number.as_u64().ok_or(MediaConfigError::StepsWrongType)?);
            }
            _ => return Err(MediaConfigError::UnexpectedField),
        }
    }
    let Some(raw) = operation else {
        return Err(MediaConfigError::UnexpectedField);
    };
    validate_token(raw, MediaConfigError::OperationInvalidChar)?;
    // The mute toggle takes no magnitude: a paired `steps` rejects instead
    // of being silently ignored.
    if raw == "toggle_mute" {
        if steps.is_some() {
            return Err(MediaConfigError::StepsNotAllowedForMute);
        }
        return Ok(VolumeOperation::ToggleMute);
    }
    let direction = StepDirection::parse(raw).ok_or(MediaConfigError::OperationUnknownToken)?;
    let requested_steps = steps.unwrap_or(1);
    let Ok(bounded) = u8::try_from(requested_steps) else {
        return Err(MediaConfigError::StepsOutOfRange);
    };
    VolumeOperation::new_step(direction, bounded)
}

/// Typed configuration failures. Structural reasons only: rejected input
/// values never appear in any variant (redaction rules, TM-LOG-01).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaConfigError {
    /// Parameters were not a JSON object.
    NotAnObject,
    /// The object carried unexpected fields, missed a required one, or
    /// repeated one.
    UnexpectedField,
    /// Transport `action` was not a string.
    ActionWrongType,
    /// The transport action token contained characters outside `[a-z_]` or
    /// exceeded [`MAX_MEDIA_TOKEN_BYTES`].
    ActionInvalidChar,
    /// A well-formed transport action token is outside the closed
    /// vocabulary.
    ActionUnknownToken,
    /// Volume `operation` was not a string.
    OperationWrongType,
    /// The volume operation token contained characters outside `[a-z_]` or
    /// exceeded [`MAX_MEDIA_TOKEN_BYTES`].
    OperationInvalidChar,
    /// A well-formed volume operation token is outside the closed
    /// vocabulary.
    OperationUnknownToken,
    /// `steps` was not a non-negative JSON integer.
    StepsWrongType,
    /// `steps` sat outside 1..=[`MAX_VOLUME_STEPS`].
    StepsOutOfRange,
    /// `steps` was paired with `toggle_mute`, which takes no magnitude.
    StepsNotAllowedForMute,
}

impl fmt::Display for MediaConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotAnObject => "params must be a JSON object",
            Self::UnexpectedField => "params carry an unexpected or missing field",
            Self::ActionWrongType => "'action' must be a string",
            Self::ActionInvalidChar => "action token contains characters outside [a-z_]",
            Self::ActionUnknownToken => "action token is outside the closed vocabulary",
            Self::OperationWrongType => "'operation' must be a string",
            Self::OperationInvalidChar => "operation token contains characters outside [a-z_]",
            Self::OperationUnknownToken => "operation token is outside the closed vocabulary",
            Self::StepsWrongType => "'steps' must be a non-negative integer",
            Self::StepsOutOfRange => "'steps' sits outside the allowed range",
            Self::StepsNotAllowedForMute => "'steps' cannot be combined with toggle_mute",
        })
    }
}

impl std::error::Error for MediaConfigError {}

#[cfg(test)]
mod tests {
    use super::{
        MAX_VOLUME_STEPS, MediaCommand, MediaConfigError, StepDirection, VolumeOperation,
        parse_media_params, parse_volume_params,
    };
    use serde_json::{Value, json};

    #[test]
    fn transport_actions_parse_across_the_closed_vocabulary() {
        assert_eq!(
            parse_media_params(&json!({ "action": "play_pause" })).unwrap(),
            MediaCommand::PlayPause
        );
        assert_eq!(
            parse_media_params(&json!({ "action": "next_track" })).unwrap(),
            MediaCommand::NextTrack
        );
        assert_eq!(
            parse_media_params(&json!({ "action": "previous_track" })).unwrap(),
            MediaCommand::PreviousTrack
        );
        assert_eq!(MediaCommand::PreviousTrack.to_string(), "previous_track");
        assert_eq!(
            MediaCommand::try_parse("next_track").unwrap(),
            MediaCommand::NextTrack
        );
    }

    #[test]
    fn volume_operations_parse_with_default_and_bounded_steps() {
        assert_eq!(
            parse_volume_params(&json!({ "operation": "up" })).unwrap(),
            VolumeOperation::Up { steps: 1 },
            "steps defaults to one increment"
        );
        assert_eq!(
            parse_volume_params(&json!({ "operation": "down", "steps": 3 })).unwrap(),
            VolumeOperation::Down { steps: 3 }
        );
        assert_eq!(
            parse_volume_params(&json!({ "operation": "toggle_mute" })).unwrap(),
            VolumeOperation::ToggleMute
        );
        // Boundary: exactly MAX_VOLUME_STEPS parses.
        assert_eq!(
            parse_volume_params(&json!({ "operation": "up", "steps": MAX_VOLUME_STEPS })).unwrap(),
            VolumeOperation::Up {
                steps: MAX_VOLUME_STEPS
            }
        );
        assert_eq!(VolumeOperation::ToggleMute.to_string(), "toggle_mute");
        assert_eq!(
            VolumeOperation::new_step(StepDirection::Up, 2).unwrap(),
            VolumeOperation::Up { steps: 2 }
        );
    }

    #[test]
    fn limits_reject_exactly_one_past_the_boundary() {
        assert_eq!(
            parse_volume_params(&json!({ "operation": "up", "steps": 0 })).unwrap_err(),
            MediaConfigError::StepsOutOfRange
        );
        let over: u64 = u64::from(MAX_VOLUME_STEPS) + 1;
        assert_eq!(
            parse_volume_params(&json!({ "operation": "up", "steps": over })).unwrap_err(),
            MediaConfigError::StepsOutOfRange
        );
        // Far overflow still rejects as out of range, not panic.
        assert_eq!(
            parse_volume_params(&json!({ "operation": "down", "steps": u64::MAX })).unwrap_err(),
            MediaConfigError::StepsOutOfRange
        );
        assert_eq!(
            VolumeOperation::new_step(StepDirection::Down, 0).unwrap_err(),
            MediaConfigError::StepsOutOfRange
        );
    }

    #[test]
    fn malformed_inputs_reject_with_typed_reasons() {
        let transport_cases: Vec<(Value, MediaConfigError)> = vec![
            (json!("play"), MediaConfigError::NotAnObject),
            (json!([]), MediaConfigError::NotAnObject),
            (json!(null), MediaConfigError::NotAnObject),
            (json!({}), MediaConfigError::UnexpectedField),
            (
                json!({ "action": "next_track", "extra": 1 }),
                MediaConfigError::UnexpectedField,
            ),
            (
                json!({ "verb": "next_track" }),
                MediaConfigError::UnexpectedField,
            ),
            (json!({ "action": 5 }), MediaConfigError::ActionWrongType),
            (
                json!({ "action": ["up"] }),
                MediaConfigError::ActionWrongType,
            ),
            (
                json!({ "action": "PLAY_PAUSE" }),
                MediaConfigError::ActionInvalidChar,
            ),
            (
                json!({ "action": "next track" }),
                MediaConfigError::ActionInvalidChar,
            ),
            (
                json!({ "action": "next\u{0}track" }),
                MediaConfigError::ActionInvalidChar,
            ),
            (
                json!({ "action": format!("a{}", "x".repeat(20)) }),
                MediaConfigError::ActionInvalidChar,
            ),
            (
                json!({ "action": "skip_ahead" }),
                MediaConfigError::ActionUnknownToken,
            ),
            (
                json!({ "action": "stop_playback" }),
                MediaConfigError::ActionUnknownToken,
            ),
        ];
        for (params, expected) in transport_cases {
            assert_eq!(
                parse_media_params(&params).unwrap_err(),
                expected,
                "case {params}"
            );
        }

        let volume_cases: Vec<(Value, MediaConfigError)> = vec![
            (json!([]), MediaConfigError::NotAnObject),
            (json!(null), MediaConfigError::NotAnObject),
            (json!({}), MediaConfigError::UnexpectedField),
            (
                json!({ "operation": "up", "extra": true }),
                MediaConfigError::UnexpectedField,
            ),
            (json!({ "verb": "up" }), MediaConfigError::UnexpectedField),
            (
                json!({ "operation": 5 }),
                MediaConfigError::OperationWrongType,
            ),
            (
                json!({ "operation": "louder" }),
                MediaConfigError::OperationUnknownToken,
            ),
            (
                json!({ "operation": "UP" }),
                MediaConfigError::OperationInvalidChar,
            ),
            (
                json!({ "operation": "up", "steps": "3" }),
                MediaConfigError::StepsWrongType,
            ),
            (
                json!({ "operation": "up", "steps": 1.5 }),
                MediaConfigError::StepsWrongType,
            ),
            (
                json!({ "operation": "up", "steps": -2 }),
                MediaConfigError::StepsWrongType,
            ),
            (
                json!({ "operation": "up", "steps": [1] }),
                MediaConfigError::StepsWrongType,
            ),
            (
                json!({ "operation": "toggle_mute", "steps": 1 }),
                MediaConfigError::StepsNotAllowedForMute,
            ),
        ];
        for (params, expected) in volume_cases {
            assert_eq!(
                parse_volume_params(&params).unwrap_err(),
                expected,
                "case {params}"
            );
        }
    }

    #[test]
    fn garbage_regression_sweep_never_panics_and_always_rejects() {
        let alphabet = [
            "",
            " ",
            "UP",
            "ü",
            "🔥",
            "next-track",
            "next.track",
            "play_pause;",
            "\u{7f}",
            "a_b_c_d_e_f_g_h_i_j_k_l_m_n_o_p",
        ];
        for raw in alphabet {
            let as_action = json!({ "action": raw });
            let as_operation = json!({ "operation": raw });
            assert!(
                parse_media_params(&as_action).is_err(),
                "transport garbage must reject: {raw:?}"
            );
            assert!(
                parse_volume_params(&as_operation).is_err(),
                "volume garbage must reject: {raw:?}"
            );
        }
        // Non-object containers reject for both families.
        for junk in ["5", "true", "[\"up\"]"] {
            let value: Value = serde_json::from_str(junk).expect("fixture json");
            assert!(parse_media_params(&value).is_err());
            assert!(parse_volume_params(&value).is_err());
        }
    }

    #[test]
    fn display_strings_stay_structural() {
        assert_eq!(VolumeOperation::Down { steps: 4 }.to_string(), "down:4");
        assert_eq!(StepDirection::Up.to_string(), "up");
        assert!(matches!(MediaCommand::PlayPause, MediaCommand::PlayPause));
    }
}
