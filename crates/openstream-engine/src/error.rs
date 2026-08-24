//! Typed engine errors.
//!
//! Every failure is a typed, matchable value; nothing fails silently and no
//! error carries secret or personal data (fail-closed posture; redaction
//! rules). Structural identifiers (node keys, action names) are echo-safe;
//! free text, parameter payloads, and variable values are never echoed.

use crate::domain_ids::ExecutionId;
use crate::journal::JournalLifecycle;
use core::fmt;
use std::collections::BTreeMap;

/// Graph validation failures (stages S1–S4 of `DOMAIN_MODEL.md` §6 as
/// realized by this crate). A graph that produces any of these values never
/// becomes a [`crate::graph::ValidatedGraph`] and can therefore never run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    /// No entry node was set, or the entry key does not match any node.
    MissingEntry,
    /// Two nodes share one key.
    DuplicateNodeKey,
    /// A node key violated the strict identifier grammar.
    InvalidNodeKey,
    /// An edge referenced a node key absent from the graph.
    DanglingEdge {
        /// Source endpoint of the dangling edge.
        from: String,
        /// Target endpoint of the dangling edge.
        to: String,
    },
    /// The graph exceeds [`crate::limits::MAX_GRAPH_NODES`].
    NodeLimitExceeded {
        /// The enforced maximum.
        limit: usize,
    },
    /// The graph exceeds [`crate::limits::MAX_GRAPH_DEPTH`] container
    /// nesting.
    DepthLimitExceeded {
        /// The enforced maximum.
        limit: usize,
    },
    /// A cycle exists in the union of flow and compensation edges
    /// (`DOMAIN_MODEL.md` §5: cycles fail validation unconditionally).
    CycleDetected,
    /// A non-entry node has more than one incoming flow edge, so the graph
    /// is not a well-formed single-parent structure.
    MultipleParents,
    /// Not every non-compensate node is reachable from the entry node over
    /// flow edges (orphaned work would otherwise silently never run).
    UnreachableNode,
    /// A node kind emitted an outgoing edge shape its kind forbids (for
    /// example an action node with a sequence successor).
    IllegalEdgeShape {
        /// The node whose outgoing edges are malformed.
        node: String,
    },
    /// A `sequence` container's child chain is not exactly one simple path.
    MalformedSequenceChain {
        /// The sequence node whose chain is malformed.
        node: String,
    },
    /// A `parallel` container has no children to fan out to.
    EmptyParallel {
        /// The parallel node with no fan-out targets.
        node: String,
    },
    /// A `conditional` node lacks a truth branch or duplicates a branch.
    MalformedConditional {
        /// The conditional node whose branches are malformed.
        node: String,
    },
    /// A `retry` node lacks exactly one body child or repeats attempts
    /// outside `1..=[crate::limits::MAX_RETRY_ATTEMPTS]`.
    MalformedRetry {
        /// The retry node whose body/attempts are malformed.
        node: String,
    },
    /// A compensation link does not connect an action node to a dedicated
    /// compensate node, or links a compensate node twice.
    MalformedCompensationLink,
    /// The action type of an action node is not registered.
    UnknownActionType {
        /// The unregistered action name (structural identifier).
        action: String,
    },
    /// The exact capability requested by an action node is not covered by
    /// any scope declared by its registration (S3 referential stage).
    CapabilityNotDeclared {
        /// The registered action name.
        action: String,
        /// Qualifier-free capability kind requested by the node.
        capability_kind: String,
    },
    /// A `retry` node wraps an action whose adapter did not declare
    /// idempotency (`TECHNICAL_SPEC` §5: retry requires adapter-declared
    /// idempotency).
    RetryRequiresIdempotency {
        /// The retried action name lacking the declaration.
        action: String,
    },
    /// The graph declares failure policy `compensate` while some action
    /// lacks a compensation link or its adapter did not declare safe
    /// compensation (`TECHNICAL_SPEC` §5).
    PolicyCompensateInvalid {
        /// The offending action name.
        action: String,
    },
    /// A deadline override or delay duration exceeded the macro cap or was
    /// zero where positive is required.
    DeadlineOutOfRange {
        /// The node carrying the invalid duration, if node-scoped.
        node: Option<String>,
    },
    /// A conditional operand or transform payload is not a scalar value.
    NonScalarOperand {
        /// The node carrying the malformed value.
        node: String,
    },
    /// A variable name inside a condition or transform violated the strict
    /// grammar.
    InvalidVariableName {
        /// The node carrying the name.
        node: String,
    },
    /// The serialized parameter payload exceeded
    /// [`crate::limits::MAX_ACTION_PARAMS_BYTES`].
    ParamsTooLarge {
        /// The node carrying the oversized payload.
        node: String,
    },
}

/// Runtime or registration configuration failures. All fail closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// No clock was supplied to the runtime builder; deadlines and
    /// scheduling are undefined without a time source.
    MissingClock,
    /// An action name violated the strict identifier grammar.
    InvalidActionName,
    /// The same action name was registered twice.
    DuplicateActionName,
    /// A registration declared an internal-only capability scope
    /// (`secret.*`, taxonomy §4); internal capabilities are never
    /// manifest-declarable.
    InternalCapabilityScope,
    /// The configured dedupe retention fell outside the hard bounds fixed
    /// by ADR-0005 decision item 3.
    RetentionOutOfBounds {
        /// Requested retention in milliseconds.
        requested_ms: i64,
        /// Inclusive lower bound in milliseconds.
        min_ms: i64,
        /// Inclusive upper bound in milliseconds.
        max_ms: i64,
    },
}

/// Journal write/read failures surfaced through the port boundary. The
/// runtime treats every variant as "durable evidence unavailable" and fails
/// closed before dispatching any effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JournalError {
    /// A capacity bound of the journal implementation was hit. Evidence is
    /// never dropped to keep accepting writes.
    Capacity {
        /// What overflowed (admissions, open prepared records).
        what: &'static str,
        /// The enforced maximum.
        limit: usize,
    },
    /// The backend refused the write (fault injection, IO failure class).
    Refused,
    /// The referenced execution has no admission record.
    UnknownExecution,
}

impl fmt::Display for JournalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Capacity { what, limit } => {
                write!(f, "journal capacity exceeded for {what} (limit {limit})")
            }
            Self::Refused => f.write_str("journal refused the write (fail closed)"),
            Self::UnknownExecution => f.write_str("execution has no admission record"),
        }
    }
}

impl std::error::Error for JournalError {}

/// Why an admission produced no execution receipt. These outcomes are
/// distinct from terminal states: no effect was dispatched under any of
/// them (except where noted for duplicates, which cite the original run).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdmissionRejection {
    /// The command expired before admission; journaled `expired` and never
    /// queued (`PROTOCOL.md`). Carries the minted execution id.
    Expired {
        /// Execution id assigned for the expired evidence record.
        execution_id: ExecutionId,
    },
    /// The `(source_device_id, message_id)` key was already admitted; the
    /// original effect is cited and never re-run (`OSCP_MESSAGES.md` §7).
    DuplicateSuppressed {
        /// Execution id of the original admitted command.
        original_execution_id: ExecutionId,
        /// Current lifecycle state of the original command.
        original_state: JournalLifecycle,
    },
    /// The journal refused durable evidence writes before any effect could
    /// be dispatched; admission aborted fail-closed.
    JournalRefused {
        /// The journal failure class.
        source: JournalError,
    },
    /// Initial variables exceeded the bounded map/value limits.
    VariablesRejected,
    /// Another execution is actively driving this runtime; begin() is
    /// single-flight by design.
    RuntimeBusy,
}

/// Why a replay of an `outcome_unknown` execution was refused. Replay is
/// permitted only under the strict rules of `OSCP_MESSAGES.md` §7/§8.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayRejection {
    /// No admission exists for the supplied dedupe key.
    UnknownKey,
    /// The original execution is not in `outcome_unknown`; only that state
    /// may be superseded, and only by explicit replay evidence.
    NotOutcomeUnknown,
    /// Some action in the replacement graph runs on an adapter that did not
    /// declare idempotency; replaying it risks duplicate effects.
    RequiresIdempotentAdapters,
    /// The replay command itself arrived past its expiry.
    Expired,
    /// Initial variables exceeded the bounded map/value limits.
    InvalidVariables,
    /// The journal refused the corrective evidence writes.
    JournalRefused {
        /// The journal failure class.
        source: JournalError,
    },
}

/// Top-level typed engine error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineError {
    /// Graph validation failed; no runtime object was produced.
    Validation(ValidationError),
    /// Runtime configuration failed.
    Config(ConfigError),
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ValidationError::*;
        match self {
            MissingEntry => f.write_str("graph has no resolvable entry node"),
            DuplicateNodeKey => f.write_str("two nodes share one key"),
            InvalidNodeKey => f.write_str("node key violates the identifier grammar"),
            DanglingEdge { from, to } => write!(f, "edge references missing node ({from} -> {to})"),
            NodeLimitExceeded { limit } => write!(f, "graph exceeds the node limit of {limit}"),
            DepthLimitExceeded { limit } => write!(f, "nesting depth exceeds the limit of {limit}"),
            CycleDetected => f.write_str("graph contains a cycle (fail closed)"),
            MultipleParents => f.write_str("a node has more than one incoming flow edge"),
            UnreachableNode => f.write_str("a node is unreachable from the entry"),
            IllegalEdgeShape { node } => write!(f, "node `{node}` emits an illegal edge shape"),
            MalformedSequenceChain { node } => {
                write!(f, "sequence `{node}` child chain is not one simple path")
            }
            EmptyParallel { node } => write!(f, "parallel `{node}` has no children"),
            MalformedConditional { node } => {
                write!(f, "conditional `{node}` branch set is malformed")
            }
            MalformedRetry { node } => write!(f, "retry `{node}` body or attempts are malformed"),
            MalformedCompensationLink => {
                f.write_str("compensation link does not connect action -> compensate once")
            }
            UnknownActionType { action } => write!(f, "action type `{action}` is not registered"),
            CapabilityNotDeclared {
                action,
                capability_kind,
            } => write!(
                f,
                "action `{action}` requests undeclared capability kind `{capability_kind}`"
            ),
            RetryRequiresIdempotency { action } => {
                write!(
                    f,
                    "retry wraps action `{action}` without declared idempotency"
                )
            }
            PolicyCompensateInvalid { action } => write!(
                f,
                "compensate policy invalid at action `{action}` (no link or unsafe adapter)"
            ),
            DeadlineOutOfRange { node: None } => {
                f.write_str("execution deadline out of range (0 < ms <= macro cap)")
            }
            DeadlineOutOfRange { node: Some(node) } => {
                write!(
                    f,
                    "deadline/duration at `{node}` out of range (0 <= ms <= macro cap)"
                )
            }
            NonScalarOperand { node } => {
                write!(f, "conditional/transform value at `{node}` is not scalar")
            }
            InvalidVariableName { node } => {
                write!(f, "variable name at `{node}` violates the grammar")
            }
            ParamsTooLarge { node } => write!(f, "params payload at `{node}` exceeds the byte cap"),
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingClock => f.write_str("runtime requires an injected clock"),
            Self::InvalidActionName => f.write_str("action name violates the identifier grammar"),
            Self::DuplicateActionName => f.write_str("action name already registered"),
            Self::InternalCapabilityScope => {
                f.write_str("internal-only capability declared as an action scope (fail closed)")
            }
            Self::RetentionOutOfBounds {
                requested_ms,
                min_ms,
                max_ms,
            } => write!(
                f,
                "dedupe retention {requested_ms}ms outside bounds [{min_ms}, {max_ms}]"
            ),
        }
    }
}

impl fmt::Display for AdmissionRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Expired { execution_id } => {
                write!(
                    f,
                    "command expired before admission ({execution_id}); journaled expired"
                )
            }
            Self::DuplicateSuppressed {
                original_execution_id,
                original_state,
            } => write!(
                f,
                "duplicate suppressed; original execution {original_execution_id} is {original_state}"
            ),
            Self::JournalRefused { source } => write!(f, "journal refused evidence: {source}"),
            Self::VariablesRejected => f.write_str("initial variables exceed the bounded limits"),
            Self::RuntimeBusy => f.write_str("another execution is actively driving this runtime"),
        }
    }
}

impl fmt::Display for ReplayRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownKey => f.write_str("no admission exists for this dedupe key"),
            Self::NotOutcomeUnknown => {
                f.write_str("original execution is not outcome_unknown; replay refused")
            }
            Self::RequiresIdempotentAdapters => {
                f.write_str("replay requires every involved adapter to declare idempotency")
            }
            Self::Expired => f.write_str("replay command expired before admission"),
            Self::InvalidVariables => f.write_str("initial variables exceed the bounded limits"),
            Self::JournalRefused { source } => {
                write!(f, "journal refused corrective evidence: {source}")
            }
        }
    }
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(error) => write!(f, "graph validation failed: {error}"),
            Self::Config(error) => write!(f, "engine configuration rejected: {error}"),
        }
    }
}

impl std::error::Error for EngineError {}
impl std::error::Error for AdmissionRejection {}
impl std::error::Error for ReplayRejection {}

impl From<ValidationError> for EngineError {
    fn from(value: ValidationError) -> Self {
        Self::Validation(value)
    }
}

impl From<ConfigError> for EngineError {
    fn from(value: ConfigError) -> Self {
        Self::Config(value)
    }
}

/// Convenience alias for validated initial variable maps handed to
/// [`crate::runtime::ExecuteRequest`].
pub type VariableMap = BTreeMap<String, serde_json::Value>;
