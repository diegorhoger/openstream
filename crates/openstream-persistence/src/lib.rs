//! `openstream-persistence` — repository traits plus SQLite storage.
//!
//! Owns repository traits, SQLite WAL storage, explicit versioned migrations,
//! soft deletion, and the local-first commit/outbox ordering described in
//! TECHNICAL_SPEC §6. Raw secret values never persist here; they stay in OS
//! credential storage behind the [`vault`] boundary (TECHNICAL_SPEC §4,
//! THREAT_MODEL TB6).
//!
//! Status: M1 security subset of issue #8 implemented — the OS
//! credential-vault abstraction with a real Windows Credential Manager
//! backend and explicit `Unsupported` stubs elsewhere. SQLite schema v1 and
//! migration tests arrive with the persistence milestones (#15).

pub mod vault;
