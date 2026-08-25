//! OBS discovery with explicit, typed version compatibility.
//!
//! [`probe_endpoint`] connects to one candidate host/port, reads the
//! server Hello, and classifies it fail-closed:
//!
//! - [`ProbeResult::Compatible`] when the announced rpcVersion equals
//!   [`protocol::RPC_VERSION_SUPPORTED`] and the obs-websocket major
//!   equals [`protocol::PROTOCOL_MAJOR_SUPPORTED`];
//! - [`ProbeResult::Incompatible`] carrying the typed reason for anything
//!   else (including unparseable version strings — never guessed);
//! - [`ProbeResult::Unreachable`] when no OBS endpoint answers.
//!
//! [`discover_endpoints`] sweeps a caller-supplied candidate list. The
//! default candidate set is the localhost OBS default port. mDNS-sourced
//! candidates plug into the same list type; LAN browsing itself stays
//! owned by `openstream-discovery` (its own reviewed milestone), so this
//! crate never starts a network listener or an mDNS daemon of its own.
//!
//! Probes never authenticate: only the unauthenticated Hello is read, so
//! discovery exercises zero credential material.

use crate::protocol::{self, ProtocolError};
use crate::session::CONNECT_TIMEOUT_MS;
use crate::transport::TungsteniteTransport;
use std::time::Duration;

/// Default TCP port OBS serves its WebSocket on.
pub const OBS_DEFAULT_PORT: u16 = 4455;

/// One discovery candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryCandidate {
    /// Host name or IP literal.
    pub host: String,
    /// TCP port.
    pub port: u16,
}

impl DiscoveryCandidate {
    /// Builds one candidate.
    #[must_use]
    pub fn new(host: &str, port: u16) -> Self {
        Self {
            host: host.to_string(),
            port,
        }
    }
}

/// The default candidate sweep: the local machine's OBS default port.
#[must_use]
pub fn default_candidates() -> Vec<DiscoveryCandidate> {
    vec![DiscoveryCandidate::new("127.0.0.1", OBS_DEFAULT_PORT)]
}

/// Typed probe outcome for one candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeResult {
    /// A supported OBS WebSocket v5 endpoint answered.
    Compatible {
        /// Server-reported obs-websocket version string.
        obs_websocket_version: String,
        /// Announced RPC protocol version (the supported constant).
        rpc_version: u32,
    },
    /// An OBS WebSocket answered but speaks an unsupported protocol or
    /// version; carries the typed compatibility failure.
    Incompatible(ProtocolError),
    /// Nothing answerable was found at this endpoint.
    Unreachable,
}

/// Probes one candidate by connecting and reading exactly the Hello
/// frame; the connection is closed immediately afterwards. No
/// authentication is attempted.
#[must_use]
pub fn probe_endpoint(candidate: &DiscoveryCandidate) -> ProbeResult {
    let connect =
        TungsteniteTransport::connect(&candidate.host, candidate.port, CONNECT_TIMEOUT_MS);
    let mut transport = match connect {
        Ok(transport) => transport,
        Err(_) => return ProbeResult::Unreachable,
    };
    let result = read_hello_classified(&mut transport);
    shutdown(&mut transport);
    result
}

fn read_hello_classified<T: crate::transport::ObsTransport>(transport: &mut T) -> ProbeResult {
    let Ok(frame) = transport.receive_text(CONNECT_TIMEOUT_MS) else {
        return ProbeResult::Unreachable;
    };
    match protocol::parse_hello(&frame) {
        Ok(hello) => match protocol::ensure_supported_hello(&hello) {
            Ok(()) => ProbeResult::Compatible {
                obs_websocket_version: hello.obs_websocket_version.clone(),
                rpc_version: hello.rpc_version,
            },
            Err(reason) => ProbeResult::Incompatible(reason),
        },
        Err(reason) => ProbeResult::Incompatible(reason),
    }
}

fn shutdown(transport: &mut TungsteniteTransport) {
    transport.send_close();
    if let Some(stream) = transport.raw_stream() {
        let _ = stream.shutdown(std::net::Shutdown::Both);
    }
}

/// Sweeps candidates in order and returns every non-unreachable result
/// paired with its candidate. Deterministic: input order is preserved and
/// probes are sequential.
#[must_use]
pub fn discover_endpoints(
    candidates: &[DiscoveryCandidate],
) -> Vec<(DiscoveryCandidate, ProbeResult)> {
    candidates
        .iter()
        .map(|candidate| (candidate.clone(), probe_endpoint(candidate)))
        .filter(|(_candidate, result)| !matches!(result, ProbeResult::Unreachable))
        .collect()
}

/// Bounded per-probe socket timeout override for tests.
#[doc(hidden)]
pub fn probe_timeout_for_tests() -> Duration {
    Duration::from_millis(CONNECT_TIMEOUT_MS)
}
