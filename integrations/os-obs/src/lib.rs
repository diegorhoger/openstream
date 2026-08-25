//! `openstream-os-obs` — OBS WebSocket v5 integration adapters (issue
//! #13), the flagship M1 magic loop.
//!
//! Composes with the merged engine contracts (#9): every user-triggered
//! effect is a typed registered action whose capability scope is declared
//! at registration and revalidated by the runtime grant intersection
//! immediately before dispatch. Discovery and live state are reads, not
//! engine actions; they never touch capability authority.
//!
//! Capability mapping (existing taxonomy rows only; no new vocabulary):
//!
//! - `obs.read` — discovery probes and event-derived live state.
//! - `obs.control.scene` — [`port::ACTION_TYPE_OBS_SCENE_SWITCH`],
//!   [`port::ACTION_TYPE_OBS_SOURCE_VISIBILITY`], and
//!   [`port::ACTION_TYPE_OBS_INPUT_MUTE`]: the three non-destructive
//!   composition operations over what the program outputs (which scene is
//!   live, which sources are visible in it, which program audio channels
//!   are audible). Same consent class as the taxonomy row (install review
//!   plus first use), reversible effects, immediate revocability.
//! - `obs.control.stream` — stream start/stop, record start/stop,
//!   replay-buffer save: exactly the taxonomy row's destructive class,
//!   which additionally requires recorded arming consent at grant
//!   creation (`required_consent`).
//!
//! Destructive arming (SECURITY.md): `obs.stream.start`, `obs.stream.stop`,
//! and `obs.record.stop` refuse with [`port::CODE_NOT_ARMED`] unless their
//! parameters carry exactly `"armed": true`. Studio sets that flag only on
//! an explicit press-time confirmation; unarmed requests die before any
//! wire effect, so there are no accidental live-stream kills.
//!
//! Secret flow: connection passwords exist only behind the OS credential
//! vault ([`ConnectionConfig`] carries a structural [`SecretRef`]); the
//! value resolves inside the handshake for the duration of one challenge
//! hash and drops immediately after. Property tests prove no config or
//! protocol path can serialize secret material.
//!
//! Failure honesty: a disconnect or timeout mid-request maps to
//! `EffectOutcome::Unknown`, so the engine journals `outcome_unknown`
//! rather than inventing success or failure; known-dead connections fail
//! typed before dispatch; reconnect uses a pure bounded backoff schedule
//! ([`session::backoff_delay_ms`], capped at 30 s, bounded attempts).
//!
//! Contract testing: [`fake_server::FakeObsServer`] speaks the same wire
//! subset on an ephemeral loopback port for deterministic CI; the real-OBS
//! check is doubly gated (`#[ignore]` plus an environment flag) and never
//! runs without both.

pub mod auth;
pub mod backend;
pub mod discovery;
#[doc(hidden)]
pub mod fake_server;
pub mod port;
pub mod protocol;
pub mod session;
pub mod spec;
pub mod state;
pub mod transport;

#[doc(inline)]
pub use crate::{
    backend::{FakeObsController, ObsController, ObsFailure, ObsInvocation, SessionObsController},
    discovery::{
        DiscoveryCandidate, OBS_DEFAULT_PORT, ProbeResult, default_candidates, discover_endpoints,
        probe_endpoint,
    },
    port::{
        ACTION_TYPE_OBS_INPUT_MUTE, ACTION_TYPE_OBS_RECORD_START, ACTION_TYPE_OBS_RECORD_STOP,
        ACTION_TYPE_OBS_REPLAY_SAVE, ACTION_TYPE_OBS_SCENE_SWITCH,
        ACTION_TYPE_OBS_SOURCE_VISIBILITY, ACTION_TYPE_OBS_STREAM_START,
        ACTION_TYPE_OBS_STREAM_STOP, CODE_AUTH_REJECTED, CODE_CAPABILITY_MISMATCH,
        CODE_CONNECTION_UNAVAILABLE, CODE_INVALID_OBS_CONFIG, CODE_NOT_ARMED, CODE_OBS_REJECTED,
        CODE_PROTOCOL_VIOLATION, CODE_UNSUPPORTED_VERSION, CODE_VAULT_UNAVAILABLE,
        ObsInputMutePort, ObsRecordControlPort, ObsReplaySavePort, ObsSceneSwitchPort,
        ObsSourceVisibilityPort, ObsStreamControlPort, register_obs_actions,
    },
    protocol::{
        Challenge, EventMessage, Hello, PROTOCOL_MAJOR_SUPPORTED, ProtocolError,
        RPC_VERSION_SUPPORTED, RequestResponse, ensure_supported_hello, parse_event, parse_hello,
        parse_response, request_frame, split_frame,
    },
    session::{
        ConnectionConfig, MAX_RECONNECT_ATTEMPTS, ObsSession, RECONNECT_BASE_DELAY_MS,
        RECONNECT_MAX_DELAY_MS, REQUEST_TIMEOUT_MS, SessionError, backoff_delay_ms,
        reconnect_with_policy,
    },
    spec::{
        InputMute, MAX_OBS_NAME_BYTES, ObsConfigError, RecordControl, RecordOp, ReplaySave,
        SceneSwitch, SourceVisibility, StreamControl, StreamOp, validate_name,
    },
    state::{
        EVENT_INPUT_MUTE_STATE_CHANGED, EVENT_PROGRAM_SCENE_CHANGED, EVENT_RECORD_STATE_CHANGED,
        EVENT_REPLAY_BUFFER_STATE_CHANGED, EVENT_STREAM_STATE_CHANGED, LiveState,
    },
    transport::{FakeTransport, ObsTransport, TransportError, TungsteniteTransport},
};

/// Real-OBS opt-in environment flag for the doubly gated integration test.
pub const REAL_OBS_E2E_FLAG: &str = "OPENSTREAM_OBS_E2E";
