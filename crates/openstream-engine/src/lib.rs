//! `openstream-engine` — deterministic action graph execution.
//!
//! Owns validated immutable action DAGs, bounded concurrency, deadlines,
//! cancellation, retry/compensation policy, and the durable prepared/result
//! journal with honest `outcome_unknown` semantics (TECHNICAL_SPEC §5,
//! PROTOCOL.md). The Engine revalidates immediately before every side effect
//! and is the sole privileged authority in the product.
//!
//! Status: M0 boundary skeleton. Graph validation and execution arrive with
//! the engine milestones; no side effects can be requested through this crate
//! yet.
