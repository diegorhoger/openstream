//! `openstream-domain` — pure domain types, IDs, and validation.
//!
//! Boundary rules (ADR-0001, TECHNICAL_SPEC §3): this crate imports no UI,
//! database, network, Tauri, or Cloud implementation; its only dependencies
//! are `serde`, `serde_json`, `uuid` for the typed model itself, and
//! `zeroize` so secret values scrub their buffers on drop.
//!
//! Implements the deck-domain subset of `DOMAIN_MODEL.md` v1 (ADR-0005):
//! profiles, decks with folder paths, pages, controls with kinds/policies/
//! visual states, typed invariants with a five-stage fail-closed validation
//! posture scoped to this crate, deterministic serialization, UUIDv7
//! identity, explicit `major.minor` schema versioning with unknown-version
//! rejection, and golden fixtures under `tests/fixtures/`. It also
//! implements the security subset of issue #8: the typed capability
//! vocabulary (`CAPABILITY_TAXONOMY.md`), deny-by-default grant records
//! with consent/narrowing/revocation, intersection-only authority
//! evaluation, append-only audit evidence events, and validated secret
//! references ([`secret::SecretRef`]) plus redacted in-memory values
//! ([`secret::SecretValue`], never serializable). Secret *values* at rest
//! live only behind OS credential storage (the `openstream-persistence`
//! vault boundary, THREAT_MODEL TB6). The remaining core entities
//! (workspaces, action graphs, executions, sync, ...) arrive with their own
//! milestones on top of these foundations.

pub mod audit;
pub mod capability;
pub mod control;
pub mod deck;
pub mod document;
pub mod error;
pub mod folder;
pub mod grant;
pub mod ids;
pub mod limits;
pub mod page;
pub mod profile;
pub mod secret;
pub mod switching;
pub mod version;

/// Major version of the versioned domain model this crate implements,
/// anchored to `DOMAIN_MODEL.md` v1 (ADR-0005). Breaking domain changes
/// require a major bump plus ADR, migration proof, and human gate.
pub const DOMAIN_MODEL_MAJOR: u32 = 1;

/// Minor version of the versioned domain model this crate implements,
/// anchored to `DOMAIN_MODEL.md` v1 (ADR-0005).
pub const DOMAIN_MODEL_MINOR: u32 = 0;
