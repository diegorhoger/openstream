//! OBS session lifecycle: connect, authenticated handshake, request
//! round-trips, and bounded reconnect.
//!
//! Secret flow (SECURITY.md hard rule): [`ConnectionConfig`] carries only
//! a host, port, and optional [`SecretRef`]. The password value is
//! resolved exclusively inside [`ObsSession::connect`] through the
//! injected [`CredentialVault`], held as a [`SecretValue`] guard for the
//! duration of the challenge-hash computation, and dropped immediately
//! after. No config type here could carry secret material at all.
//!
//! Reconnect discipline: [`backoff_delay_ms`] is a pure bounded schedule
//! (250 ms doubling, capped at 30 s) and [`MAX_RECONNECT_ATTEMPTS`] bounds
//! total attempts; [`reconnect_with_policy`] applies them honestly and
//! never loops forever.
//!
//! Failure honesty (`PROTOCOL.md`): a request whose dispatch began but
//! whose outcome could not be observed surfaces
//! [`SessionError::OutcomeLost`], which ports map to
//! `EffectOutcome::Unknown` so the engine journals `outcome_unknown`
//! instead of inventing success or failure.

use crate::auth;
use crate::protocol::{self, Challenge, Hello, RPC_VERSION_SUPPORTED, RequestResponse};
use crate::state::LiveState;
use crate::transport::{ObsTransport, TransportError};
use openstream_domain::secret::{SecretRef, SecretValue};
use openstream_persistence::vault::{CredentialVault, VaultError};

/// Connect timeout applied to TCP establishment plus WebSocket handshake.
pub const CONNECT_TIMEOUT_MS: u64 = 3_000;

/// Per-request response deadline.
pub const REQUEST_TIMEOUT_MS: u64 = 5_000;

/// Hard bound on reconnect attempts before giving up and surfacing the
/// failure honestly to the caller.
pub const MAX_RECONNECT_ATTEMPTS: u32 = 6;

/// Base delay of the bounded reconnect backoff schedule.
pub const RECONNECT_BASE_DELAY_MS: u64 = 250;

/// Ceiling of the bounded reconnect backoff schedule.
pub const RECONNECT_MAX_DELAY_MS: u64 = 30_000;

/// Pure bounded backoff: base doubling with saturating shift, capped at
/// [`RECONNECT_MAX_DELAY_MS`].
#[must_use]
pub fn backoff_delay_ms(attempt: u32) -> u64 {
    let shifted = RECONNECT_BASE_DELAY_MS.saturating_mul(1_u64 << attempt.min(31));
    shifted.min(RECONNECT_MAX_DELAY_MS)
}

/// Connection parameters for one OBS instance. Serialization carries no
/// password material by construction: the credential lives only behind
/// `secret_ref` in OS credential storage.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ConnectionConfig {
    /// OBS WebSocket host name or IP literal.
    pub host: String,
    /// OBS WebSocket TCP port (OBS default 4455).
    pub port: u16,
    /// Structural reference to the connection password in the OS vault;
    /// absent when the server requires no authentication.
    pub secret_ref: Option<SecretRef>,
}

/// Typed session failures. Structural classes only; no host or OS text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionError {
    /// No live connection exists; nothing was attempted.
    NotConnected,
    /// The server refused authentication (closed during the authenticated
    /// handshake). Nothing was dispatched.
    AuthRejected,
    /// The server speaks an unsupported protocol version. Nothing was
    /// dispatched.
    UnsupportedVersion,
    /// A wire-protocol violation occurred before anything was dispatched.
    ProtocolViolation,
    /// The transport failed before any bytes reached the peer.
    PreFlight,
    /// The vault had no entry for the configured reference.
    VaultNotFound,
    /// The stored vault value was unusable.
    VaultCorrupt,
    /// The platform credential store refused.
    VaultPlatformFailure,
    /// No vault backend exists on this platform.
    VaultUnsupported,
    /// The request was dispatched but its outcome cannot be observed.
    /// Maps onto the engine's honest `outcome_unknown` semantics.
    OutcomeLost,
}

impl core::fmt::Display for SessionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::NotConnected => "no live obs connection",
            Self::AuthRejected => "obs authentication refused",
            Self::UnsupportedVersion => "obs websocket version unsupported",
            Self::ProtocolViolation => "obs protocol violation before dispatch",
            Self::PreFlight => "transport failed before dispatch",
            Self::VaultNotFound => "connection secret not found in vault",
            Self::VaultCorrupt => "stored connection secret is unusable",
            Self::VaultPlatformFailure => "platform credential store refused",
            Self::VaultUnsupported => "no credential vault backend for this platform",
            Self::OutcomeLost => "request dispatched but outcome unobservable",
        })
    }
}

impl std::error::Error for SessionError {}

fn map_vault(error: VaultError) -> SessionError {
    match error {
        VaultError::NotFound { .. } => SessionError::VaultNotFound,
        VaultError::Corrupt { .. } => SessionError::VaultCorrupt,
        VaultError::PlatformFailure { .. } => SessionError::VaultPlatformFailure,
        VaultError::Unsupported { .. } => SessionError::VaultUnsupported,
    }
}

fn map_handshake_transport(error: TransportError) -> SessionError {
    match error {
        TransportError::ConnectFailed | TransportError::IoFailure => SessionError::PreFlight,
        // A close or timeout during the pre-authentication read means the
        // peer vanished before we dispatched anything.
        TransportError::Closed | TransportError::Timeout => SessionError::PreFlight,
    }
}

fn map_protocol(error: protocol::ProtocolError) -> SessionError {
    match error {
        protocol::ProtocolError::UnsupportedRpcVersion { .. }
        | protocol::ProtocolError::UnsupportedProtocolVersion { .. } => {
            SessionError::UnsupportedVersion
        }
        _ => SessionError::ProtocolViolation,
    }
}

/// One authenticated OBS WebSocket v5 session over an owned transport.
///
/// Requests are correlated by monotonic session-unique ids. Events seen
/// while waiting for responses fold into the attached [`LiveState`]
/// tracker instead of being dropped.
#[derive(Debug)]
pub struct ObsSession<T: ObsTransport> {
    transport: T,
    state: LiveState,
    next_request_id: u64,
    connected: bool,
    pending_response: Option<RequestResponse>,
}

impl<T: ObsTransport> ObsSession<T> {
    /// Performs the full v5 handshake over `transport`: read Hello, gate
    /// compatibility fail-closed, resolve the password through `vault`
    /// when the server demands authentication, send Identify, and await
    /// Identified. On success the negotiated rpcVersion equals
    /// [`RPC_VERSION_SUPPORTED`].
    ///
    /// # Errors
    /// Typed [`SessionError`]; nothing partial survives (a failed
    /// handshake yields no usable session).
    pub fn connect(
        mut transport: T,
        vault: &dyn CredentialVault,
        config: &ConnectionConfig,
    ) -> Result<(Self, Hello), SessionError> {
        let hello_frame = transport
            .receive_text(CONNECT_TIMEOUT_MS)
            .map_err(map_handshake_transport)?;
        let hello = protocol::parse_hello(&hello_frame).map_err(map_protocol)?;
        protocol::ensure_supported_hello(&hello).map_err(map_protocol)?;

        let auth_hash = match &hello.auth {
            None => None,
            Some(Challenge { challenge, salt }) => {
                let Some(secret_ref) = &config.secret_ref else {
                    // The server demands auth and nothing is configured.
                    return Err(SessionError::AuthRejected);
                };
                let password: SecretValue = vault.load(secret_ref).map_err(map_vault)?;
                let hash = auth::challenge_response(password.expose(), salt, challenge);
                drop(password);
                Some(hash)
            }
        };

        transport
            .send_text(&protocol::identify_frame(
                RPC_VERSION_SUPPORTED,
                auth_hash.as_deref(),
            ))
            .map_err(|_| SessionError::PreFlight)?;

        let identified_frame = transport.receive_text(CONNECT_TIMEOUT_MS).map_err(|_| {
            // After credentials were sent, ANY failure to obtain the
            // ack classifies as refusal: fail closed on authentication.
            SessionError::AuthRejected
        })?;
        let (op, _payload) = protocol::split_frame(&identified_frame).map_err(map_protocol)?;
        if op != 2 {
            return Err(SessionError::ProtocolViolation);
        }

        Ok((
            Self {
                transport,
                state: LiveState::default(),
                next_request_id: 0,
                connected: true,
                pending_response: None,
            },
            hello,
        ))
    }

    /// Performs one request/response exchange. Events observed while
    /// waiting update the live-state tracker.
    ///
    /// # Errors
    /// [`SessionError`] per its variant documentation; notably
    /// [`SessionError::OutcomeLost`] when the outcome cannot be observed
    /// after dispatch began.
    pub fn request(
        &mut self,
        request_type: &str,
        data: Option<&serde_json::Value>,
    ) -> Result<RequestResponse, SessionError> {
        if !self.connected {
            return Err(SessionError::NotConnected);
        }
        self.pending_response = None;
        let request_id = format!("os-{}", self.next_request_id);
        self.next_request_id += 1;
        let frame = protocol::request_frame(request_type, &request_id, data);
        if self.transport.send_text(&frame).is_err() {
            // Bytes may have partially reached the peer: honest unknown,
            // never invented failure.
            self.connected = false;
            return Err(SessionError::OutcomeLost);
        }
        loop {
            match self.transport.receive_text(REQUEST_TIMEOUT_MS) {
                Ok(incoming) => self.absorb_frame(&incoming)?,
                Err(_) => {
                    self.connected = false;
                    return Err(SessionError::OutcomeLost);
                }
            }
            if let Some(response) = self.pending_response.take_if(|response| {
                response.request_id == request_id && response.request_type == request_type
            }) {
                return Ok(response);
            }
        }
    }

    fn absorb_frame(&mut self, frame: &str) -> Result<(), SessionError> {
        let (op, _payload) =
            protocol::split_frame(frame).map_err(|_| SessionError::ProtocolViolation)?;
        match op {
            4 => {
                if let Ok(event) = protocol::parse_event(frame) {
                    let _changed = self.state.apply_event(&event);
                }
                Ok(())
            }
            6 => {
                let response = protocol::parse_response(frame).map_err(map_protocol)?;
                self.pending_response = Some(response);
                Ok(())
            }
            _ => Err(SessionError::ProtocolViolation),
        }
    }

    /// Read-only snapshot of the event-derived live state.
    #[must_use]
    pub const fn live_state(&self) -> &LiveState {
        &self.state
    }

    /// Whether this session believes the transport is still usable.
    #[must_use]
    pub const fn is_connected(&self) -> bool {
        self.connected
    }

    /// Marks the session dead after an externally observed disconnect.
    pub fn mark_disconnected(&mut self) {
        self.connected = false;
    }
}

/// Attempts one reconnect against a fresh transport factory, applying the
/// bounded schedule: attempt `n` sleeps [`backoff_delay_ms`](n) first, up
/// to `max_attempts` total tries. The `sleep_ms` hook lets tests verify
/// the schedule deterministically without real time.
///
/// # Errors
/// The final connect error after the bounded attempts.
pub fn reconnect_with_policy<T, F>(
    max_attempts: u32,
    mut connect_attempt: F,
    sleep_ms: &mut dyn FnMut(u64),
) -> Result<T, SessionError>
where
    F: FnMut() -> Result<T, SessionError>,
{
    let mut last = SessionError::NotConnected;
    for attempt in 0..max_attempts.max(1) {
        sleep_ms(backoff_delay_ms(attempt));
        match connect_attempt() {
            Ok(session) => return Ok(session),
            Err(error) => last = error,
        }
    }
    Err(last)
}
