//! Versioned portable/save documents (DOMAIN_MODEL.md §1, §8).
//!
//! A document couples an explicit `schema_version` to one validated entity.
//! Serialization is deterministic: fields emit in declaration order and no
//! unordered maps exist in v1, so the same document always produces the same
//! bytes — the property golden fixtures rely on. Decoding fails closed on
//! foreign majors, minors newer than supported, unknown members, and any
//! entity that fails save-time validation.

use crate::deck::Deck;
use crate::error::DomainError;
use crate::profile::Profile;
use crate::version::SchemaVersion;
use serde::{Deserialize, Serialize};

/// A versioned deck document (deck + pages + controls + folder path).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeckDocument {
    /// Explicit schema version; decoded fail-closed by [`SchemaVersion`].
    pub schema_version: SchemaVersion,
    /// The deck subtree.
    pub deck: Deck,
}

/// A versioned profile document.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileDocument {
    /// Explicit schema version; decoded fail-closed by [`SchemaVersion`].
    pub schema_version: SchemaVersion,
    /// The profile.
    pub profile: Profile,
}

impl DeckDocument {
    /// Stamps the supported version onto a validated-or-later entity. Call
    /// [`Self::validate`] before persisting or exporting.
    #[must_use]
    pub const fn new(deck: Deck) -> Self {
        Self {
            schema_version: SchemaVersion::supported(),
            deck,
        }
    }

    /// Save-time validation of the whole subtree (S1–S2 scope of this
    /// crate; referential/semantic stages live with their registries).
    pub fn validate(&self) -> Result<(), DomainError> {
        if !self.schema_version.is_readable() {
            return Err(DomainError::UnknownSchemaVersion {
                found: self.schema_version,
                supported: SchemaVersion::supported(),
            });
        }
        self.deck.validate()
    }

    /// Deterministic compact JSON serialization.
    pub fn to_json_string(&self) -> Result<String, DomainError> {
        self.validate()?;
        serde_json::to_string(self).map_err(|error| DomainError::Encoding {
            detail: error.to_string(),
        })
    }

    /// Decodes from JSON: unknown versions/members reject during decode,
    /// then save-time validation runs before anything is returned.
    pub fn from_json_str(raw: &str) -> Result<Self, DomainError> {
        let parsed: Self = serde_json::from_str(raw).map_err(|error| DomainError::Encoding {
            detail: error.to_string(),
        })?;
        parsed.validate()?;
        Ok(parsed)
    }
}

impl ProfileDocument {
    /// Stamps the supported version. Call [`Self::validate`] before
    /// persisting or exporting.
    #[must_use]
    pub const fn new(profile: Profile) -> Self {
        Self {
            schema_version: SchemaVersion::supported(),
            profile,
        }
    }

    /// Save-time validation of the whole subtree.
    pub fn validate(&self) -> Result<(), DomainError> {
        if !self.schema_version.is_readable() {
            return Err(DomainError::UnknownSchemaVersion {
                found: self.schema_version,
                supported: SchemaVersion::supported(),
            });
        }
        self.profile.validate()
    }

    /// Deterministic compact JSON serialization.
    pub fn to_json_string(&self) -> Result<String, DomainError> {
        self.validate()?;
        serde_json::to_string(self).map_err(|error| DomainError::Encoding {
            detail: error.to_string(),
        })
    }

    /// Decodes from JSON with full fail-closed checks.
    pub fn from_json_str(raw: &str) -> Result<Self, DomainError> {
        let parsed: Self = serde_json::from_str(raw).map_err(|error| DomainError::Encoding {
            detail: error.to_string(),
        })?;
        parsed.validate()?;
        Ok(parsed)
    }
}
