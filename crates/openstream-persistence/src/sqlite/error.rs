//! Typed storage failures for the SQLite layer.
//!
//! Redaction discipline matches the vault boundary: no SQLite message text,
//! path fragment, or row content ever enters an error value — only the
//! structural stage/operation class and schema version numbers. Every
//! variant fails closed; callers never guess at a half-applied state
//! (each mutation is one committed transaction, so a refused step leaves
//! the previous durable state intact).

use core::fmt;

/// Where in the open/verify pipeline corruption was detected. Structural
/// only: the checker verdict itself (`PRAGMA integrity_check`) is never
/// echoed because it can embed fragments of stored bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CorruptionStage {
    /// The file could not even be opened/configured as a database.
    Header,
    /// The engine refused the integrity probe itself.
    Probe,
    /// The integrity probe completed and reported damage.
    Content,
}

/// Why a database file was rejected as "not an OpenStream database".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SchemaStage {
    /// Tables exist but none of them is the OpenStream schema anchor.
    Foreign,
    /// The schema anchor exists but its version row is missing or unreadable.
    Anchor,
}

/// Typed failures of the local SQLite storage layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageError {
    /// The database file (or a quarantine/backup target) could not be
    /// opened, created, or replaced. OS error text is dropped.
    Unavailable,
    /// The file exists but failed verification as a healthy OpenStream
    /// database at `stage`.
    Corrupted {
        /// Pipeline stage that detected the damage.
        stage: CorruptionStage,
    },
    /// The file contains tables but not an OpenStream schema (e.g. the path
    /// points at an unrelated SQLite database). Never overwritten in place;
    /// recovery quarantines instead.
    UnrecognizedSchema {
        /// What about the layout was unexpected.
        stage: SchemaStage,
    },
    /// The database was written by a NEWER OpenStream schema. Downgrades
    /// are refused unconditionally (forward-only migrations).
    SchemaTooNew {
        /// Version recorded in the database.
        found: u32,
        /// Highest version this build implements.
        supported: u32,
    },
    /// The migration chain has no step leaving version `from`; the database
    /// cannot be brought forward by this build.
    MigrationMissing {
        /// Version whose successor step is absent.
        from: u32,
    },
    /// A forward migration step failed inside its transaction; the database
    /// rolls back to `from` atomically.
    MigrationFailed {
        /// Version the step started from.
        from: u32,
        /// Version the step would have produced.
        to: u32,
    },
    /// A verified pre-migration backup could not be produced before an
    /// upgrade of an existing database; the migration did NOT run.
    BackupUnavailable {
        /// Version the pending migration targets.
        target_version: u32,
    },
}

impl StorageError {
    /// True when this error reports an unusable/corrupt primary database
    /// for which [`crate::sqlite::recover`] is the documented remedy.
    #[must_use]
    pub const fn is_corruption(&self) -> bool {
        matches!(
            self,
            Self::Corrupted { .. } | Self::UnrecognizedSchema { .. }
        )
    }
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => f.write_str("database file unavailable"),
            Self::Corrupted { stage } => write!(f, "database corrupted ({stage})"),
            Self::UnrecognizedSchema { stage } => {
                write!(f, "not an OpenStream database ({stage})")
            }
            Self::SchemaTooNew { found, supported } => write!(
                f,
                "database schema v{found} is newer than supported v{supported}; downgrade refused"
            ),
            Self::MigrationMissing { from } => {
                write!(f, "no forward migration step from schema v{from}")
            }
            Self::MigrationFailed { from, to } => {
                write!(f, "migration v{from} -> v{to} failed; rolled back")
            }
            Self::BackupUnavailable { target_version } => write!(
                f,
                "pre-migration backup for v{target_version} unavailable; upgrade aborted"
            ),
        }
    }
}

impl std::error::Error for StorageError {}

impl fmt::Display for CorruptionStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Header => "header",
            Self::Probe => "probe",
            Self::Content => "content",
        })
    }
}

impl fmt::Display for SchemaStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Foreign => "foreign-tables",
            Self::Anchor => "anchor",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{CorruptionStage, SchemaStage, StorageError};

    #[test]
    fn display_is_structural_and_redacted() {
        let rendered = format!(
            "{}|{}|{}|{}",
            StorageError::Unavailable,
            StorageError::Corrupted {
                stage: CorruptionStage::Content
            },
            StorageError::SchemaTooNew {
                found: 9,
                supported: 1
            },
            StorageError::UnrecognizedSchema {
                stage: SchemaStage::Foreign
            },
        );
        assert_eq!(
            rendered,
            "database file unavailable|database corrupted (content)|\
             database schema v9 is newer than supported v1; downgrade refused|\
             not an OpenStream database (foreign-tables)"
        );
        assert!(
            StorageError::Corrupted {
                stage: CorruptionStage::Header
            }
            .is_corruption()
        );
        assert!(!StorageError::Unavailable.is_corruption());
    }
}
