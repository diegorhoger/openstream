//! Typed durable-entity identifiers (UUIDv7, DOMAIN_MODEL.md §2).
//!
//! STUB: parse validation intentionally permissive until tests drive it.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

macro_rules! entity_id {
    ($(#[$doc:meta])* $name:ident, $entity:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(Uuid);

        impl $name {
            /// Mints a fresh identifier (Rust core is the generation
            /// authority; DOMAIN_MODEL.md §2). Time ordering is diagnostic only.
            #[must_use]
            pub fn generate() -> Self {
                Self(Uuid::now_v7())
            }

            /// The raw UUID.
            #[must_use]
            pub const fn as_uuid(&self) -> Uuid {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl FromStr for $name {
            type Err = crate::error::DomainError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                // Canonical form only: parse must succeed, be UUIDv7 with an
                // RFC 4122 variant, and round-trip to the exact lowercase
                // hyphenated spelling that was handed in (DOMAIN_MODEL.md §2).
                let uuid = Uuid::parse_str(s)
                    .map_err(|_| crate::error::DomainError::InvalidId { entity: $entity })?;
                if uuid.get_version_num() != 7 || s != uuid.to_string() {
                    return Err(crate::error::DomainError::InvalidId { entity: $entity });
                }
                Ok(Self(uuid))
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let s = String::deserialize(deserializer)?;
                Self::from_str(&s).map_err(serde::de::Error::custom)
            }
        }
    };
}

entity_id!(
    /// Identifier of a Workspace (referenced here; the workspace entity itself
    /// ships with its own milestone).
    WorkspaceId,
    "workspace"
);
entity_id!(
    /// Identifier of a Profile.
    ProfileId,
    "profile"
);
entity_id!(
    /// Identifier of a Deck.
    DeckId,
    "deck"
);
entity_id!(
    /// Identifier of a Page.
    PageId,
    "page"
);
entity_id!(
    /// Identifier of a Control.
    ControlId,
    "control"
);
entity_id!(
    /// Identifier of one recorded capability grant.
    GrantId,
    "grant"
);
entity_id!(
    /// Identifier of one engine execution (evidence subject in audit events).
    ExecutionId,
    "execution"
);

#[cfg(test)]
mod tests {
    use super::{ControlId, DeckId, ExecutionId, GrantId, PageId, ProfileId, WorkspaceId};
    use std::str::FromStr as _;

    const V4: &str = "3b241101-e2bb-4255-8caf-4136c566a962";
    // Canonical lowercase hyphenated UUIDv7 sample (version nibble `7`).
    const V7: &str = "018f6a1c-7b21-7cc0-9f31-0e3d5a9d4c11";

    macro_rules! id_behavior {
        ($mod_name:ident, $ty:ident) => {
            mod $mod_name {
                use super::*;

                #[test]
                fn generate_round_trips_as_v7() {
                    let id = $ty::generate();
                    let text = id.to_string();
                    let parsed = $ty::from_str(&text).unwrap();
                    assert_eq!(parsed, id);
                    let raw = id.as_uuid();
                    assert_eq!(raw.get_version_num(), 7);
                    assert_eq!(raw.get_variant(), uuid::Variant::RFC4122);
                }

                #[test]
                fn accepts_canonical_lowercase_v7() {
                    let id = $ty::from_str(V7).unwrap();
                    assert_eq!(id.to_string(), V7);
                }

                #[test]
                fn rejects_uppercase_spelling() {
                    // Canonical form is lowercase; uppercase must fail closed.
                    let upper = V7.to_ascii_uppercase();
                    assert!($ty::from_str(&upper).is_err());
                }

                #[test]
                fn rejects_non_uuid_text() {
                    assert!($ty::from_str("not-a-uuid").is_err());
                    assert!($ty::from_str("").is_err());
                }

                #[test]
                fn rejects_other_versions() {
                    assert!($ty::from_str(V4).is_err());
                }

                #[test]
                fn serde_round_trips_as_string() {
                    let id = $ty::from_str(V7).unwrap();
                    let json = serde_json::to_string(&id).unwrap();
                    assert_eq!(json, format!("\"{V7}\""));
                    let back: $ty = serde_json::from_str(&json).unwrap();
                    assert_eq!(back, id);
                }

                #[test]
                fn serde_rejects_non_v7() {
                    let json = format!("\"{V4}\"");
                    assert!(serde_json::from_str::<$ty>(&json).is_err());
                }
            }
        };
    }

    id_behavior!(workspace_id_behavior, WorkspaceId);
    id_behavior!(profile_id_behavior, ProfileId);
    id_behavior!(deck_id_behavior, DeckId);
    id_behavior!(page_id_behavior, PageId);
    id_behavior!(control_id_behavior, ControlId);
    id_behavior!(grant_id_behavior, GrantId);
    id_behavior!(execution_id_behavior, ExecutionId);

    #[test]
    fn generated_ids_are_unique() {
        let a = DeckId::generate();
        let b = DeckId::generate();
        assert_ne!(a, b);
        assert_ne!(a.as_uuid(), b.as_uuid());
    }
}
