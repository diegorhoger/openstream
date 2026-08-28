//! `openstream-engine` — deterministic action graph execution.
//!
//! Owns validated immutable action DAGs, bounded deterministic scheduling,
//! deadlines, cancellation, retry/compensation policy, journal-first
//! admission with durable prepared/terminal evidence, and honest
//! `outcome_unknown` crash-gap semantics (`TECHNICAL_SPEC` §1, §5;
//! `PROTOCOL.md`; `OSCP_MESSAGES.md` §5–§8; ADR-0005).
//!
//! The Engine is the sole privileged authority in the product: every side
//! effect passes, in order, grant intersection ([`registry`] scopes ∩
//! user grants via [`crate::domain::grant::GrantLedger`]) and a durable
//! preparation record before its adapter port may be invoked, and every
//! timing decision derives from an injected [`clock::Clock`] so runs are
//! bit-for-bit reproducible under [`clock::FakeClock`].
//!
//! Boundary status (issue #9):
//! - **Real now:** validation pipeline S1–S4 for the eight v1 node kinds,
//!   typed registry with capability-scope/idempotency/safe-compensation
//!   declarations, deterministic discrete-event scheduler with concurrency
//!   caps (4 per adapter / 32 global), monotonic deadlines (default 30 s,
//!   macro cap 10 min), failure policies stop/continue/compensate,
//!   cancellation propagation, dedupe window bounds with
//!   `outcome_unknown`-exempt pruning, replay gating, crash-window recovery.
//! - **Ports, not engines:** persistence stays behind
//!   [`journal::ExecutionJournal`] ([`journal::MemoryJournal`] ships;
//!   SQLite is #15) and time behind [`clock::Clock`] (no system-clock
//!   implementation ships until the composition root, #16).
//! - **Fakes only:** concrete OS/OBS adapters are issues #10–#13; this
//!   crate defines the dispatch contract they must satisfy. Issue #14
//!   proves the multi-action semantics end-to-end through those real
//!   registered adapters behind recorded fakes (integration suites in the
//!   adapter crates) and settles residual sibling work at every terminal
//!   so no crash-gap window survives a persisted decision.
//!
//! Failure honesty: terminal states are exactly the authoritative journal
//! subset of `PROTOCOL.md` ([`runtime::TerminalState`]); unknown outcomes
//! are never inferred as success and non-idempotent adapters receive no
//! automatic retry or replay ([`ActionRuntime::replay`] gates both).

pub mod clock;
pub mod domain_ids;
pub mod error;
pub mod failure;
pub mod fixtures;
pub mod graph;
pub mod identifiers;
pub mod journal;
pub mod limits;
pub mod port;
pub mod registry;
pub mod runtime;

/// Major version of the engine contract this crate implements.
pub const ENGINE_MAJOR: u32 = 1;

/// Minor version of the engine contract this crate implements.
pub const ENGINE_MINOR: u32 = 0;

#[doc(inline)]
pub use crate::{
    clock::{Clock, FakeClock},
    domain_ids::ExecutionId,
    error::{
        AdmissionRejection, ConfigError, EngineError, JournalError, ReplayRejection,
        ValidationError,
    },
    failure::{FailurePolicy, FailureReason},
    graph::{EdgeKindInput, NodeIdx, NodeKey, NodeKind, RawGraph, ValidatedGraph},
    identifiers::{InvalidIdentity, MessageId, SourceDeviceId},
    journal::{
        AdmissionEntry, DedupeKey, ExecutionJournal, JournalLifecycle, MemoryJournal, PreparedEntry,
    },
    limits::{
        DEDUPE_DEFAULT_RETENTION_MS, DEDUPE_MAX_RETENTION_MS, DEDUPE_MIN_RETENTION_MS,
        DEFAULT_DEADLINE_MS, MACRO_MAX_DEADLINE_MS, MAX_CONCURRENCY_GLOBAL,
        MAX_CONCURRENCY_PER_ACTION, MAX_GRAPH_DEPTH, MAX_GRAPH_NODES, MAX_RETRY_ATTEMPTS,
        RETRY_BACKOFF_BASE_MS,
    },
    port::{DispatchUnavailable, EffectOutcome, EffectPort, EffectRequest, EffectResponse},
    registry::{ActionRegistration, ActionRegistry, IdempotencyClass},
    runtime::{
        ActionRuntime, CancelSignal, ExecuteRequest, ExecutionHandle, ExecutionReceipt,
        RuntimeBuilder, RuntimeConfig, TerminalState, TimeControl,
    },
};
