//! Adapter effect ports: the only path through which the engine requests
//! external side effects.
//!
//! Dispatch happens exclusively AFTER (a) durable preparation exists in the
//! journal and (b) the capability grant intersection passed for this exact
//! attempt (`TECHNICAL_SPEC` §1: revalidate immediately before every side
//! effect). The port contract is deliberately tiny and synchronous so a
//! deterministic scheduler can interleave parallel branches: an adapter
//! either completes instantly or reports a virtual duration after which its
//! outcome is observed. Concrete OS/OBS adapters are later milestones
//! (#10–#13); this module defines the contract plus the evidence types.

use crate::domain_ids::ExecutionId;
use crate::graph::NodeKey;
use core::fmt;
use openstream_domain::capability::Capability;

/// One request to perform one side effect. Carries structural identifiers
/// and bounded payloads only — never secrets.
#[derive(Debug, Clone)]
pub struct EffectRequest {
    /// Owning execution.
    pub execution_id: ExecutionId,
    /// Graph node performing this effect.
    pub node_key: NodeKey,
    /// Registered action type name.
    pub action_type: String,
    /// Exact capability already validated against grants for this attempt.
    pub capability: Capability,
    /// Bounded JSON parameters authored in the graph.
    pub params: serde_json::Value,
    /// Deterministic adapter-facing idempotency key derived from
    /// `(source_device_id, message_id, node, attempt)` so adapter-side
    /// collapse is possible under replay (`OSCP_MESSAGES.md` §7).
    pub idempotency_key: String,
    /// Zero-based attempt counter within the owning retry scope.
    pub attempt: u32,
    /// True when this invocation is compensation unwinding.
    pub is_compensation: bool,
}

/// Outcome of one dispatched effect. `Unknown` models the honest crash-gap:
/// the result was lost after (possibly) applying, never inferred either way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectOutcome {
    /// The adapter reports authoritative success.
    Succeeded,
    /// The adapter reports a typed operational failure with a bounded code.
    Failed {
        /// Adapter-chosen structural code (no free text).
        code: String,
    },
    /// The result could not be observed; the execution journals
    /// `outcome_unknown` and never auto-retries.
    Unknown,
}

impl EffectOutcome {
    /// Journal-safe token for evidence records.
    #[must_use]
    pub fn token(&self) -> &str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed { .. } => "failed",
            Self::Unknown => "unknown",
        }
    }

    /// The failure code when this outcome is a typed failure.
    #[must_use]
    pub fn failure_code(&self) -> Option<&str> {
        match self {
            Self::Failed { code } => Some(code),
            _ => None,
        }
    }
}

/// How an effect settles. Delayed effects occupy their concurrency slot
/// until the reported virtual duration elapses on the injected clock.
#[derive(Debug, Clone, PartialEq)]
pub enum EffectResponse {
    /// Completed before returning (occupies no scheduling time).
    Immediate(EffectOutcome),
    /// Completes at `now + duration_ms`; the runtime observes `outcome`
    /// exactly then (fully deterministic under [`crate::clock::FakeClock`]).
    Delayed {
        /// Virtual completion horizon in monotonic milliseconds.
        duration_ms: u64,
        /// Outcome delivered at completion.
        outcome: EffectOutcome,
    },
}

/// Why dispatch was refused outright by the port boundary. Refusals are
/// distinct from adapter-reported failures: nothing was attempted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DispatchUnavailable;

impl fmt::Display for DispatchUnavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("adapter port refused dispatch")
    }
}

impl std::error::Error for DispatchUnavailable {}

/// Object-safe adapter boundary. Implementations must be deterministic
/// under test (outcomes derive from scripted state, never real time) and
/// must never log payloads or secrets.
pub trait EffectPort: fmt::Debug + Send + Sync {
    /// Performs (or schedules) one effect for `request`.
    ///
    /// # Errors
    /// [`DispatchUnavailable`] when the adapter cannot accept work at all
    /// (target connection unavailable class).
    fn invoke(&self, request: EffectRequest) -> Result<EffectResponse, DispatchUnavailable>;
}
