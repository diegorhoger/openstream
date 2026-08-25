//! Typed, bounded action-parameter schemas for the OBS action family.
//!
//! Every registered OBS action admits exactly one closed JSON schema;
//! unknown fields, wrong JSON types, off-vocabulary tokens, empty/oversized
//! names, and unarmed destructive operations all reject with distinct
//! structural errors before any effect can be attempted. The same typed
//! constructors validate authoring-time configuration and the `parse`
//! functions revalidate every dispatch (`invalid_obs_config` at the port).
//!
//! Redaction rules: error values carry structural reasons only — scene,
//! source, and input names never enter an error or a failure code.

/// Maximum byte length of one OBS scene/source/input name.
pub const MAX_OBS_NAME_BYTES: usize = 128;

/// Structural schema failures. No rejected input ever enters the value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObsConfigError {
    /// A required field is absent.
    MissingField(&'static str),
    /// A present field has the wrong JSON type.
    WrongType(&'static str),
    /// A field outside the closed schema was supplied.
    UnknownField(&'static str),
    /// A name field is empty.
    EmptyName,
    /// A name exceeds [`MAX_OBS_NAME_BYTES`].
    NameTooLong,
    /// A name carries control characters, wildcards, or surrounding
    /// whitespace.
    ForbiddenCharacter,
    /// A destructive operation arrived without `"armed": true`.
    NotArmed,
}

impl core::fmt::Display for ObsConfigError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::MissingField(field) => write!(f, "missing required field {field:?}"),
            Self::WrongType(field) => write!(f, "field {field:?} has the wrong JSON type"),
            Self::UnknownField(field) => write!(f, "unknown field {field:?}"),
            Self::EmptyName => f.write_str("name must not be empty"),
            Self::NameTooLong => f.write_str("name exceeds the bounded length"),
            Self::ForbiddenCharacter => f.write_str("name carries forbidden characters"),
            Self::NotArmed => f.write_str("destructive operation requires armed=true confirmation"),
        }
    }
}

impl std::error::Error for ObsConfigError {}

/// Validates one bounded OBS name (scene, source, or input). Fail closed:
/// non-empty, length-bounded, no control characters, no wildcards, no
/// surrounding whitespace.
pub fn validate_name(raw: &str) -> Result<(), ObsConfigError> {
    if raw.is_empty() {
        return Err(ObsConfigError::EmptyName);
    }
    if raw.len() > MAX_OBS_NAME_BYTES {
        return Err(ObsConfigError::NameTooLong);
    }
    if raw.trim() != raw
        || raw
            .chars()
            .any(|c| c.is_control() || matches!(c, '*' | '?'))
    {
        return Err(ObsConfigError::ForbiddenCharacter);
    }
    Ok(())
}

fn require_name(params: &serde_json::Value, field: &'static str) -> Result<String, ObsConfigError> {
    let Some(value) = params.get(field) else {
        return Err(ObsConfigError::MissingField(field));
    };
    let Some(name) = value.as_str() else {
        return Err(ObsConfigError::WrongType(field));
    };
    validate_name(name)?;
    Ok(name.to_string())
}

fn require_bool(params: &serde_json::Value, field: &'static str) -> Result<bool, ObsConfigError> {
    let Some(value) = params.get(field) else {
        return Err(ObsConfigError::MissingField(field));
    };
    let Some(flag) = value.as_bool() else {
        return Err(ObsConfigError::WrongType(field));
    };
    Ok(flag)
}

fn reject_unknown_fields(
    params: &serde_json::Value,
    allowed: &[&str],
) -> Result<(), ObsConfigError> {
    let Some(map) = params.as_object() else {
        return Err(ObsConfigError::WrongType("(root)"));
    };
    for key in map.keys() {
        if !allowed.contains(&key.as_str()) {
            // Copy the offending key into a bounded static-shaped error
            // without leaking arbitrary input text: the field name class
            // is what evidence needs, not the value.
            return Err(ObsConfigError::UnknownField("unrecognized"));
        }
    }
    Ok(())
}

/// Scene-switch parameters (`obs.scene.switch`): exactly one `scene` field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneSwitch {
    scene: String,
}

impl SceneSwitch {
    /// Typed constructor.
    pub fn try_new(scene: &str) -> Result<Self, ObsConfigError> {
        validate_name(scene)?;
        Ok(Self {
            scene: scene.to_string(),
        })
    }

    /// The target scene name.
    #[must_use]
    pub fn scene(&self) -> &str {
        &self.scene
    }

    /// Revalidates dispatch params against the closed schema.
    ///
    /// # Errors
    /// [`ObsConfigError`] on any schema violation.
    pub fn parse(params: &serde_json::Value) -> Result<Self, ObsConfigError> {
        reject_unknown_fields(params, &["scene"])?;
        Ok(Self {
            scene: require_name(params, "scene")?,
        })
    }
}

/// Source-visibility parameters (`obs.source.visibility`): show/hide one
/// source item inside exactly one named parent scene.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceVisibility {
    scene: String,
    source: String,
    visible: bool,
}

impl SourceVisibility {
    /// Typed constructor.
    pub fn try_new(scene: &str, source: &str, visible: bool) -> Result<Self, ObsConfigError> {
        validate_name(scene)?;
        validate_name(source)?;
        Ok(Self {
            scene: scene.to_string(),
            source: source.to_string(),
            visible,
        })
    }

    /// The parent scene whose item list is addressed.
    #[must_use]
    pub fn scene(&self) -> &str {
        &self.scene
    }

    /// The target source name.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Whether the source becomes visible (`true`) or hidden (`false`).
    #[must_use]
    pub const fn visible(&self) -> bool {
        self.visible
    }

    /// Revalidates dispatch params against the closed schema.
    ///
    /// # Errors
    /// [`ObsConfigError`] on any schema violation.
    pub fn parse(params: &serde_json::Value) -> Result<Self, ObsConfigError> {
        reject_unknown_fields(params, &["scene", "source", "visible"])?;
        Ok(Self {
            scene: require_name(params, "scene")?,
            source: require_name(params, "source")?,
            visible: require_bool(params, "visible")?,
        })
    }
}

/// Input-mute parameters (`obs.input.mute`): mute/unmute one OBS input's
/// audio channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputMute {
    input: String,
    muted: bool,
}

impl InputMute {
    /// Typed constructor.
    pub fn try_new(input: &str, muted: bool) -> Result<Self, ObsConfigError> {
        validate_name(input)?;
        Ok(Self {
            input: input.to_string(),
            muted,
        })
    }

    /// The target input name.
    #[must_use]
    pub fn input(&self) -> &str {
        &self.input
    }

    /// Whether the input becomes muted (`true`) or unmuted (`false`).
    #[must_use]
    pub const fn muted(&self) -> bool {
        self.muted
    }

    /// Revalidates dispatch params against the closed schema.
    ///
    /// # Errors
    /// [`ObsConfigError`] on any schema violation.
    pub fn parse(params: &serde_json::Value) -> Result<Self, ObsConfigError> {
        reject_unknown_fields(params, &["input", "mute"])?;
        Ok(Self {
            input: require_name(params, "input")?,
            muted: require_bool(params, "mute")?,
        })
    }
}

/// Closed stream-operation vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamOp {
    /// Start streaming.
    Start,
    /// Stop streaming.
    Stop,
}

impl StreamOp {
    /// Canonical lowercase token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
        }
    }
}

fn require_armed_flag(
    params: &serde_json::Value,
    allowed: &[&str],
) -> Result<bool, ObsConfigError> {
    reject_unknown_fields(params, allowed)?;
    match params.get("armed") {
        // Absent or false both mean "not armed": the destructive class
        // refuses before anything else is interpreted.
        None | Some(serde_json::Value::Bool(false)) => Err(ObsConfigError::NotArmed),
        Some(serde_json::Value::Bool(true)) => Ok(true),
        Some(_) => Err(ObsConfigError::WrongType("armed")),
    }
}

/// Stream start/stop parameters (`obs.stream.start` / `obs.stream.stop`):
/// exactly one `armed` field that must be literal `true`. Both directions
/// are destructive-class per SECURITY.md — an unarmed request rejects
/// before any effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamControl {
    op: StreamOp,
    armed: bool,
}

impl StreamControl {
    /// Typed constructor. Fails closed unless armed.
    pub fn try_new(op: StreamOp, armed: bool) -> Result<Self, ObsConfigError> {
        Self::require_armed(armed)?;
        Ok(Self { op, armed: true })
    }

    const fn require_armed(armed: bool) -> Result<(), ObsConfigError> {
        if armed {
            Ok(())
        } else {
            Err(ObsConfigError::NotArmed)
        }
    }

    /// The requested operation.
    #[must_use]
    pub const fn op(&self) -> StreamOp {
        self.op
    }

    /// Always `true`; construction fails otherwise.
    #[must_use]
    pub const fn armed(&self) -> bool {
        self.armed
    }

    /// Revalidates dispatch params: exactly one `armed` field, literal
    /// `true`.
    ///
    /// # Errors
    /// [`ObsConfigError::NotArmed`] on any unarmed form; schema errors
    /// otherwise.
    pub fn parse(op: StreamOp, params: &serde_json::Value) -> Result<Self, ObsConfigError> {
        require_armed_flag(params, &["armed"])?;
        Ok(Self { op, armed: true })
    }
}

/// Closed record-operation vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordOp {
    /// Start recording.
    Start,
    /// Stop recording.
    Stop,
}

impl RecordOp {
    /// Canonical lowercase token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
        }
    }
}

/// Record start/stop parameters (`obs.record.start` / `obs.record.stop`):
/// record start takes exactly zero fields; record stop requires exactly
/// one `armed` field with literal `true` (stopping a recording mid-take is
/// destructive-class; starting one is not).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordControl {
    op: RecordOp,
    armed: bool,
}

impl RecordControl {
    /// Typed constructor. Stop requires arming; start must be unarmed.
    pub fn try_new(op: RecordOp, armed: bool) -> Result<Self, ObsConfigError> {
        match op {
            RecordOp::Stop => Self::require_armed(armed)?,
            RecordOp::Start => {
                if armed {
                    return Err(ObsConfigError::NotArmed);
                }
            }
        }
        Ok(Self { op, armed })
    }

    const fn require_armed(armed: bool) -> Result<(), ObsConfigError> {
        if armed {
            Ok(())
        } else {
            Err(ObsConfigError::NotArmed)
        }
    }

    /// The requested operation.
    #[must_use]
    pub const fn op(&self) -> RecordOp {
        self.op
    }

    /// Whether the request carried explicit arming.
    #[must_use]
    pub const fn armed(&self) -> bool {
        self.armed
    }

    /// Revalidates dispatch params against the per-operation closed schema.
    ///
    /// # Errors
    /// [`ObsConfigError::NotArmed`] on an unarmed stop or an armed start;
    /// schema errors otherwise.
    pub fn parse(op: RecordOp, params: &serde_json::Value) -> Result<Self, ObsConfigError> {
        match op {
            RecordOp::Start => {
                reject_unknown_fields(params, &[])?;
                Ok(Self { op, armed: false })
            }
            RecordOp::Stop => {
                require_armed_flag(params, &["armed"])?;
                Ok(Self { op, armed: true })
            }
        }
    }
}

/// Replay-buffer save parameters (`obs.replay.save`): exactly zero fields.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReplaySave;

impl ReplaySave {
    /// Typed constructor.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Revalidates dispatch params against the closed schema.
    ///
    /// # Errors
    /// [`ObsConfigError`] on any supplied field.
    pub fn parse(params: &serde_json::Value) -> Result<Self, ObsConfigError> {
        reject_unknown_fields(params, &[])?;
        Ok(Self)
    }
}
