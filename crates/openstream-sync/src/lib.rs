//! `openstream-sync` — public operation log and merge rules.
//!
//! Owns the versioned operation outbox, field-level LWW merging with
//! hybrid-logical-clock plus actor-ID tie-breaks, tombstone dominance,
//! fractional ordering, `needs_resolution` grid collisions, and the rule that
//! invalid merged graphs stay stored but disabled (TECHNICAL_SPEC §6).
//! Content is encrypted before it can reach any hosted service; this crate
//! never implements hosted coordination itself.
//!
//! Status: M0 boundary skeleton. Merge semantics and log formats arrive with
//! the sync milestones.
