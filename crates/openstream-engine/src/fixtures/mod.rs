//! Simulator fixtures aligned with protocol/codec contract (`OSCP_MESSAGES.md` §11).
//!
//! Contains deterministic fake-state fixtures: F1-F8 patterns for engine
//! simulators, fake clocks, keys, network negotiation, replay/duplication,
//! expiry boundaries, crash windows, recovery chains, error vectors, and
//! cross-language parity identifiers. No secrets or personal data.

pub mod simulator_fixtures;
pub use simulator_fixtures::*;
