//! OBS WebSocket v5 wire-protocol codec (JSON frames).
//!
//! Implements exactly the subset of the obs-websocket 5.x protocol this
//! integration uses: the Hello/Identify/Identified handshake with optional
//! challenge authentication, single request/response exchanges, and event
//! consumption. Parsing is hand-rolled over serde_json values and fails
//! closed: malformed frames, missing fields, wrong types, and unsupported
//! protocol versions all produce typed errors.
//!
//! Version compatibility (PROTOCOL.md honesty rule): only rpcVersion 1
//! with an obs-websocket major version of 5 is supported. Anything else,
//! including unparseable version strings and future majors, fails typed
//! instead of guessing.

/// The only RPC protocol version this integration speaks.
pub const RPC_VERSION_SUPPORTED: u32 = 1;

/// The only obs-websocket protocol major this integration supports.
pub const PROTOCOL_MAJOR_SUPPORTED: u32 = 5;

/// Typed wire-protocol failures. Structural data only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    /// The frame was not a JSON object.
    MalformedFrame,
    /// A required field was absent or had an unusable type.
    MalformedField(&'static str),
    /// The server announced an RPC version other than the supported one.
    UnsupportedRpcVersion {
        /// The observed rpcVersion value.
        observed: u32,
    },
    /// The server announced an obs-websocket major other than the
    /// supported one (or an unparseable version string).
    UnsupportedProtocolVersion {
        /// The observed obsWebSocketVersion string.
        observed: String,
    },
}

impl core::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::MalformedFrame => f.write_str("frame is not a JSON object"),
            Self::MalformedField(field) => write!(f, "malformed field {field:?}"),
            Self::UnsupportedRpcVersion { observed } => write!(
                f,
                "unsupported rpcVersion {observed}; supported: {RPC_VERSION_SUPPORTED}"
            ),
            Self::UnsupportedProtocolVersion { observed } => write!(
                f,
                "unsupported obs-websocket protocol version {observed:?}; \
                 supported major: {PROTOCOL_MAJOR_SUPPORTED}"
            ),
        }
    }
}

impl std::error::Error for ProtocolError {}

/// The authentication challenge block of a Hello message. Challenge and
/// salt are server-generated nonces, not secrets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Challenge {
    /// Server challenge nonce (base64).
    pub challenge: String,
    /// Server salt nonce (base64).
    pub salt: String,
}

/// Parsed Hello (opcode 0) message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hello {
    /// Server-reported obs-websocket version string (for example 5.4.2).
    pub obs_websocket_version: String,
    /// Server-supported RPC protocol version.
    pub rpc_version: u32,
    /// Present when the server requires password authentication.
    pub auth: Option<Challenge>,
}

/// One parsed request response (opcode 6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestResponse {
    /// Echoed request type.
    pub request_type: String,
    /// Echoed session-unique request id.
    pub request_id: String,
    /// OBS-side result flag (requestStatus.result).
    pub result: bool,
    /// OBS-side numeric status code (requestStatus.code).
    pub code: i64,
    /// Response payload when present.
    pub data: Option<serde_json::Value>,
}

/// One parsed event (opcode 4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventMessage {
    /// OBS event type name (for example CurrentProgramSceneChanged).
    pub event_type: String,
    /// Event payload when present.
    pub data: serde_json::Value,
}

fn field_str(
    object: &serde_json::Map<String, serde_json::Value>,
    name: &'static str,
) -> Result<String, ProtocolError> {
    match object.get(name) {
        Some(serde_json::Value::String(value)) => Ok(value.clone()),
        _ => Err(ProtocolError::MalformedField(name)),
    }
}

/// Splits one wire frame into its opcode byte and payload object.
///
/// # Errors
/// [`ProtocolError`] when the frame is not a JSON object with numeric
/// opcode and an object payload.
pub fn split_frame(
    frame: &str,
) -> Result<(u8, serde_json::Map<String, serde_json::Value>), ProtocolError> {
    let value: serde_json::Value =
        serde_json::from_str(frame).map_err(|_| ProtocolError::MalformedFrame)?;
    let Some(object) = value.as_object() else {
        return Err(ProtocolError::MalformedFrame);
    };
    let op = match object.get("op") {
        Some(serde_json::Value::Number(number)) => number
            .as_u64()
            .and_then(|raw| u8::try_from(raw).ok())
            .ok_or(ProtocolError::MalformedField("op"))?,
        _ => return Err(ProtocolError::MalformedField("op")),
    };
    let payload = object
        .get("d")
        .and_then(|d| d.as_object())
        .cloned()
        .ok_or(ProtocolError::MalformedField("d"))?;
    Ok((op, payload))
}

/// Parses one Hello frame. Does NOT decide compatibility; see
/// [`ensure_supported_hello`].
///
/// # Errors
/// [`ProtocolError`] on any structural violation.
pub fn parse_hello(frame: &str) -> Result<Hello, ProtocolError> {
    let (0, data) = split_frame(frame)? else {
        return Err(ProtocolError::MalformedField("op"));
    };
    let obs_websocket_version = field_str(&data, "obsWebSocketVersion")?;
    let rpc_version = match data.get("rpcVersion") {
        Some(serde_json::Value::Number(number)) => number
            .as_u64()
            .and_then(|raw| u32::try_from(raw).ok())
            .ok_or(ProtocolError::MalformedField("rpcVersion"))?,
        _ => return Err(ProtocolError::MalformedField("rpcVersion")),
    };
    let auth = match data.get("authentication") {
        None | Some(serde_json::Value::Null) => None,
        Some(auth_value) => {
            let Some(auth) = auth_value.as_object() else {
                return Err(ProtocolError::MalformedField("authentication"));
            };
            Some(Challenge {
                challenge: field_str(auth, "challenge")?,
                salt: field_str(auth, "salt")?,
            })
        }
    };
    Ok(Hello {
        obs_websocket_version,
        rpc_version,
        auth,
    })
}

/// Fail-closed compatibility gate: rpcVersion must equal
/// [`RPC_VERSION_SUPPORTED`] and the obs-websocket major must equal
/// [`PROTOCOL_MAJOR_SUPPORTED`]. An unparseable version string is treated
/// as unsupported, never guessed.
///
/// # Errors
/// [`ProtocolError::UnsupportedRpcVersion`] or
/// [`ProtocolError::UnsupportedProtocolVersion`].
pub fn ensure_supported_hello(hello: &Hello) -> Result<(), ProtocolError> {
    if hello.rpc_version != RPC_VERSION_SUPPORTED {
        return Err(ProtocolError::UnsupportedRpcVersion {
            observed: hello.rpc_version,
        });
    }
    let major = hello
        .obs_websocket_version
        .split('.')
        .next()
        .unwrap_or_default();
    let Ok(major) = major.parse::<u32>() else {
        return Err(ProtocolError::UnsupportedProtocolVersion {
            observed: hello.obs_websocket_version.clone(),
        });
    };
    if major != PROTOCOL_MAJOR_SUPPORTED {
        return Err(ProtocolError::UnsupportedProtocolVersion {
            observed: hello.obs_websocket_version.clone(),
        });
    }
    Ok(())
}

/// Builds the Identify (opcode 1) frame. `auth_hash` carries the derived
/// challenge response only; raw password material never enters a frame.
#[must_use]
pub fn identify_frame(rpc_version: u32, auth_hash: Option<&str>) -> String {
    let mut data = serde_json::Map::new();
    data.insert(
        "rpcVersion".to_string(),
        serde_json::Value::from(rpc_version),
    );
    if let Some(hash) = auth_hash {
        data.insert(
            "authentication".to_string(),
            serde_json::Value::String(hash.to_string()),
        );
    }
    serde_json::json!({ "op": 1u8, "d": data }).to_string()
}

/// Builds the Identified acknowledgment (opcode 2) frame (server side).
#[must_use]
pub fn identified_frame(negotiated_rpc_version: u32) -> String {
    serde_json::json!({
        "op": 2u8,
        "d": { "negotiatedRpcVersion": negotiated_rpc_version }
    })
    .to_string()
}

/// Builds one request (opcode 5) frame with a session-unique request id.
#[must_use]
pub fn request_frame(
    request_type: &str,
    request_id: &str,
    data: Option<&serde_json::Value>,
) -> String {
    let mut d = serde_json::Map::new();
    d.insert(
        "requestType".to_string(),
        serde_json::Value::String(request_type.to_string()),
    );
    d.insert(
        "requestId".to_string(),
        serde_json::Value::String(request_id.to_string()),
    );
    if let Some(payload) = data {
        d.insert("requestData".to_string(), payload.clone());
    }
    serde_json::json!({ "op": 5u8, "d": d }).to_string()
}

/// Builds one request response (opcode 6) frame (server side).
#[must_use]
pub fn response_frame(
    request_type: &str,
    request_id: &str,
    result: bool,
    code: i64,
    data: Option<&serde_json::Value>,
) -> String {
    let mut d = serde_json::Map::new();
    d.insert(
        "requestType".to_string(),
        serde_json::Value::String(request_type.to_string()),
    );
    d.insert(
        "requestId".to_string(),
        serde_json::Value::String(request_id.to_string()),
    );
    d.insert(
        "requestStatus".to_string(),
        serde_json::json!({ "result": result, "code": code }),
    );
    if let Some(payload) = data {
        d.insert("responseData".to_string(), payload.clone());
    }
    serde_json::json!({ "op": 6u8, "d": d }).to_string()
}

/// Builds one event (opcode 4) frame.
#[must_use]
pub fn event_frame(event_type: &str, data: Option<&serde_json::Value>) -> String {
    let mut d = serde_json::Map::new();
    d.insert(
        "eventType".to_string(),
        serde_json::Value::String(event_type.to_string()),
    );
    if let Some(payload) = data {
        d.insert("eventData".to_string(), payload.clone());
    }
    serde_json::json!({ "op": 4u8, "d": d }).to_string()
}

/// Parses one request-response (opcode 6) frame.
///
/// # Errors
/// [`ProtocolError`] on any structural violation.
pub fn parse_response(frame: &str) -> Result<RequestResponse, ProtocolError> {
    let (6, data) = split_frame(frame)? else {
        return Err(ProtocolError::MalformedField("op"));
    };
    let request_type = field_str(&data, "requestType")?;
    let request_id = field_str(&data, "requestId")?;
    let status = data
        .get("requestStatus")
        .and_then(|s| s.as_object())
        .ok_or(ProtocolError::MalformedField("requestStatus"))?;
    let result = match status.get("result") {
        Some(serde_json::Value::Bool(flag)) => *flag,
        _ => return Err(ProtocolError::MalformedField("requestStatus.result")),
    };
    let code = match status.get("code") {
        Some(serde_json::Value::Number(number)) => number
            .as_i64()
            .ok_or(ProtocolError::MalformedField("requestStatus.code"))?,
        _ => return Err(ProtocolError::MalformedField("requestStatus.code")),
    };
    Ok(RequestResponse {
        request_type,
        request_id,
        result,
        code,
        data: data.get("responseData").cloned().filter(|v| !v.is_null()),
    })
}

/// Parses one event (opcode 4) frame.
///
/// # Errors
/// [`ProtocolError`] on any structural violation.
pub fn parse_event(frame: &str) -> Result<EventMessage, ProtocolError> {
    let (4, data) = split_frame(frame)? else {
        return Err(ProtocolError::MalformedField("op"));
    };
    Ok(EventMessage {
        event_type: field_str(&data, "eventType")?,
        data: data
            .get("eventData")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
    })
}
