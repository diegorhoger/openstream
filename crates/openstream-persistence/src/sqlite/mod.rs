//! SQLite WAL storage: the local persistence layer (issue #15,
//! TECHNICAL_SPEC §2 "SQLite WAL with explicit migrations", §6).
//!
//! - [`SqliteJournal`] implements the engine's `ExecutionJournal` durability
//!   port over SQLite with atomic autosave semantics.
//! - The open pipeline ([`SCHEMA_VERSION`], forward-only migrations with
//!   verified backup-before-migrate, integrity verification) is shared by
//!   every future repository in this crate; new schema versions append one
//!   step to `MIGRATIONS` and one SQL file under the repository's
//!   `migrations/sqlite/` directory.
//! - [`recover`] is the documented corruption remedy: restore a validated
//!   backup when one exists, otherwise quarantine damaged files (preserved
//!   for forensics) and recreate fresh. It never silently destroys or
//!   guesses.
//!
//! Secret values never appear in this layer (SECURITY.md hard rules): the
//! schema stores typed identifiers, timestamps, and closed-vocabulary state
//! tokens only, and tests scan the raw database bytes for secret-material
//! patterns.

mod error;
mod journal;
mod migrations;

pub use self::error::{CorruptionStage, SchemaStage, StorageError};
pub use self::journal::{JournalBounds, SqliteJournal};
pub use self::migrations::{RecoveryOutcome, RecoveryReport, SCHEMA_VERSION, recover};
