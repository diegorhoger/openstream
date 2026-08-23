//! `openstream-domain` — pure domain types, IDs, and validation.
//!
//! Boundary rules (ADR-0001, TECHNICAL_SPEC §3): this crate imports no UI,
//! database, network, Tauri, or Cloud implementation; its only dependencies
//! are `serde`, `serde_json`, and `uuid` for the typed model itself.
//!
//! Implements the deck-domain subset of `DOMAIN_MODEL.md` v1 (ADR-0005):
//! profiles, decks with folder paths, pages, controls with kinds/policies/
//! visual states, typed invariants with a five-stage fail-closed validation
//! posture scoped to this crate, deterministic serialization, UUIDv7
//! identity, explicit `major.minor` schema versioning with unknown-version
//! rejection, and golden fixtures under `tests/fixtures/`. The remaining
//! core entities (workspaces, action graphs, executions, sync, ...) arrive
//! with their own milestones on top of these foundations.

pub mod control;
pub mod deck;
pub mod document;
pub mod error;
pub mod folder;
pub mod ids;
pub mod limits;
pub mod page;
pub mod profile;
pub mod version;

/// Major version of the versioned domain model this crate implements,
/// anchored to `DOMAIN_MODEL.md` v1 (ADR-0005). Breaking domain changes
/// require a major bump plus ADR, migration proof, and human gate.
pub const DOMAIN_MODEL_MAJOR: u32 = 1;

/// Minor version of the versioned domain model this crate implements,
/// anchored to `DOMAIN_MODEL.md` v1 (ADR-0005).
pub const DOMAIN_MODEL_MINOR: u32 = 0;
