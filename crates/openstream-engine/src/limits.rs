//! v1 engine limits (TECHNICAL_SPEC §5, DOMAIN_MODEL.md §5).
//!
//! These constants ARE the v1 contract for the action engine. Per
//! `DOMAIN_MODEL.md` §1, *tightening* any value here rejects previously
//! valid graphs and therefore requires a domain-major change with ADR +
//! migration; *loosening* or adding optional capacity is a minor change.
//! Values fixed by specification are marked; others pin unspecified
//! contract space the way `openstream-domain::limits` does for the deck
//! domain.

/// Maximum number of nodes in one validated action graph
/// (`DOMAIN_MODEL.md` §5: "maximum 128 nodes"; spec-fixed).
pub const MAX_GRAPH_NODES: usize = 128;

/// Maximum container nesting depth of one validated action graph
/// (`DOMAIN_MODEL.md` §5: "nesting depth 16"; spec-fixed). Containers are
/// `sequence`, `parallel`, `conditional`, and `retry` nodes.
pub const MAX_GRAPH_DEPTH: usize = 16;

/// Default per-invocation deadline in monotonic milliseconds
/// (`TECHNICAL_SPEC` §5: "default deadline 30 seconds"; spec-fixed).
pub const DEFAULT_DEADLINE_MS: u64 = 30_000;

/// Macro maximum for any execution deadline, delay duration, or node
/// deadline override, in monotonic milliseconds (`TECHNICAL_SPEC` §5:
/// "macro maximum 10 minutes"; spec-fixed).
pub const MACRO_MAX_DEADLINE_MS: u64 = 600_000;

/// Maximum simultaneous in-flight delayed effects per registered action
/// adapter (`TECHNICAL_SPEC` §5: "default per-plugin concurrency four";
/// spec-fixed default).
pub const MAX_CONCURRENCY_PER_ACTION: usize = 4;

/// Maximum simultaneous in-flight delayed effects across the whole runtime
/// (`TECHNICAL_SPEC` §5: "global 32"; spec-fixed).
pub const MAX_CONCURRENCY_GLOBAL: usize = 32;

/// Maximum total attempts (first run plus retries) of one `retry` node.
/// Unspecified by DOMAIN_MODEL.md §5; pins the v1 contract. Tightening is a
/// breaking change.
pub const MAX_RETRY_ATTEMPTS: u32 = 8;

/// Base backoff before a retry re-attempt, in monotonic milliseconds;
/// attempt *n* waits `BACKOFF_BASE_MS * 2^(n-1)`. Deterministic by
/// construction; never wall-clock derived.
pub const RETRY_BACKOFF_BASE_MS: u64 = 50;

/// Maximum number of variables carried by one execution. Unspecified by
/// DOMAIN_MODEL.md §5; pins the v1 fail-closed bound against unbounded
/// memory growth from graph-authored writes.
pub const MAX_VARIABLES: usize = 64;

/// Maximum serialized size in UTF-8 bytes of one variable value.
pub const MAX_VARIABLE_VALUE_BYTES: usize = 1024;

/// Maximum serialized size in UTF-8 bytes of the parameter payload of one
/// action node (passed verbatim to its adapter port).
pub const MAX_ACTION_PARAMS_BYTES: usize = 2048;

/// Dedupe retention floor in wall-clock milliseconds
/// (`ADR-0005` decision item 3: hard lower bound 1 hour; spec-fixed).
pub const DEDUPE_MIN_RETENTION_MS: i64 = 3_600_000;

/// Dedupe retention ceiling in wall-clock milliseconds
/// (`ADR-0005` decision item 3: hard upper bound 7 days; spec-fixed).
pub const DEDUPE_MAX_RETENTION_MS: i64 = 604_800_000;

/// Default dedupe retention in wall-clock milliseconds
/// (`ADR-0005` decision item 3: "defaults to 24 hours"; spec-fixed).
pub const DEDUPE_DEFAULT_RETENTION_MS: i64 = 86_400_000;

/// Maximum admission records retained by the in-memory journal. Overflow
/// fails closed instead of dropping evidence (mirrors the audit-log posture
/// of `openstream-domain`). Durable persistence arrives with #15.
pub const MAX_JOURNAL_ADMISSIONS: usize = 10_000;

/// Maximum open (prepared-without-resolution) effect records retained by
/// the in-memory journal. Overflow fails closed; open records are crash-gap
/// evidence and are never silently dropped.
pub const MAX_JOURNAL_OPEN_PREPARED: usize = 4096;
