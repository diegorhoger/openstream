//! `openstream-persistence` — repository traits plus SQLite storage.
//!
//! Owns repository traits, SQLite WAL storage, explicit versioned
//! migrations, soft deletion, and the local-first commit/outbox ordering described in
//! TECHNICAL_SPEC §6. Raw secret values never persist here; they stay in OS
//! credential storage behind the [`vault`] boundary (TECHNICAL_SPEC §4,
//! THREAT_MODEL TB6).
//!
//! Status: M1 security subset of issue #8 implemented — the OS
//! credential-vault abstraction with a real Windows Credential Manager
//! backend and explicit `Unsupported` stubs elsewhere — plus the issue #15
//! local persistence layer: the SQLite WAL realization of the engine's
//! `ExecutionJournal` durability port ([`sqlite`]) with atomic autosave,
//! forward-only schema migrations anchored at a version table, verified
//! backup-before-migrate, integrity-checked opens, and a quarantine-or-
//! restore corruption recovery path. Cloud sync remains out of scope.

pub mod sqlite;
pub mod vault;
