//! Typed domain errors.
//!
//! Every failure is a typed, matchable value; nothing fails silently and no
//! error carries secret or personal data (DOMAIN_MODEL.md §6 fail-closed
//! pipeline; taxonomy redaction rules).

use crate::version::SchemaVersion;
use std::fmt;

/// Typed validation failure for any deck-domain document or entity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainError {
    /// Document declares a schema version this reader must not interpret
    /// (foreign major, or a minor newer than supported). Fail closed.
    UnknownSchemaVersion {
        /// Version found in the document.
        found: SchemaVersion,
        /// Highest version this build can read.
        supported: SchemaVersion,
    },
    /// An identifier string was not a canonical lowercase hyphenated UUIDv7.
    InvalidId {
        /// Entity kind whose identifier failed to parse.
        entity: &'static str,
    },
    /// A required text field violated its v1 limits (empty after trim, or
    /// longer than [`crate::limits::MAX_TEXT_BYTES`]).
    TextFieldOutOfRange {
        /// Field name (structural, never user content).
        field: &'static str,
    },
    /// A folder path segment or the whole path violated folder-path rules.
    InvalidFolderPath {
        /// Human-safe structural reason code, not user content.
        reason: FolderPathError,
    },
    /// A page grid dimension was zero.
    ZeroGridDimension,
    /// Control geometry had zero width or height.
    ZeroGeometryExtent,
    /// Text decoding/encoding failed before any semantic validation.
    Encoding {
        /// Structural decoder message (no user content).
        detail: String,
    },
    /// Control geometry does not fit inside the owning page grid.
    GeometryOutsideGrid {
        /// Axis that overflowed: `x`, `y`, `width`, or `height`.
        axis: &'static str,
    },
    /// A collection exceeded its v1 maximum size.
    LimitExceeded {
        /// What overflowed (pages per deck, controls per page, ...).
        what: &'static str,
        /// The enforced maximum.
        limit: usize,
    },
    /// Two controls on one page share an identifier.
    DuplicateControlId,
    /// A profile lists the same deck more than once.
    DuplicateDeckRef,
    /// Two pages of one deck share an ordinal.
    OrdinalConflict,
    /// A control references a page other than the one containing it.
    ForeignControlPage,
    /// A page references a deck other than the one containing it.
    ForeignPageDeck,
    /// A control's interaction policy is not allowed for its kind.
    PolicyNotAllowedForKind,
    /// Structural edit could not produce a new monotonic revision.
    RevisionOverflow,
}

/// Structural reasons a folder path is invalid. Matchable without exposing
/// user text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderPathError {
    /// Path exceeds [`crate::limits::MAX_FOLDER_PATH_BYTES`] UTF-8 bytes.
    PathTooLong,
    /// More than [`crate::limits::MAX_FOLDER_SEGMENTS`] segments.
    TooManySegments,
    /// Empty path segment (`//`).
    EmptySegment,
    /// Segment is `.` or `..`.
    DotSegment,
    /// Segment is only whitespace or has leading/trailing whitespace.
    SegmentWhitespace,
    /// Segment exceeds [`crate::limits::MAX_FOLDER_SEGMENT_BYTES`] bytes.
    SegmentTooLong,
    /// Segment contains `/`, `\`, or a C0/C1 control character.
    ForbiddenCharacter,
}

impl fmt::Display for DomainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownSchemaVersion { found, supported } => write!(
                f,
                "unknown domain schema version {found}; supported up to {supported} (fail closed)"
            ),
            Self::InvalidId { entity } => {
                write!(f, "{entity} id is not a canonical lowercase UUIDv7")
            }
            Self::TextFieldOutOfRange { field } => {
                write!(f, "text field `{field}` empty or over the v1 byte limit")
            }
            Self::InvalidFolderPath { reason } => write!(f, "invalid folder path: {reason:?}"),
            Self::ZeroGridDimension => write!(f, "grid dimensions must be at least 1"),
            Self::ZeroGeometryExtent => write!(f, "control geometry extent must be at least 1"),
            Self::Encoding { detail } => write!(f, "document encoding failure: {detail}"),
            Self::GeometryOutsideGrid { axis } => {
                write!(f, "control geometry `{axis}` falls outside the page grid")
            }
            Self::LimitExceeded { what, limit } => {
                write!(f, "`{what}` exceeds its limit of {limit}")
            }
            Self::DuplicateControlId => write!(f, "duplicate control id on one page"),
            Self::DuplicateDeckRef => write!(f, "profile lists the same deck twice"),
            Self::OrdinalConflict => write!(f, "two pages share one ordinal"),
            Self::ForeignControlPage => write!(f, "control references a foreign page"),
            Self::ForeignPageDeck => write!(f, "page references a foreign deck"),
            Self::PolicyNotAllowedForKind => {
                write!(f, "interaction policy not allowed for this control kind")
            }
            Self::RevisionOverflow => write!(f, "deck revision cannot increase further"),
        }
    }
}

impl std::error::Error for DomainError {}
