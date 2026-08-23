//! Explicit domain schema versions (`major.minor`, DOMAIN_MODEL.md §1).
//!
//! Decoding fails closed: foreign majors, minors newer than supported, and
//! unknown members inside `schema_version` all reject during deserialization.

use crate::error::DomainError;
use core::fmt;
use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Explicit schema version carried by every durable or portable document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SchemaVersion {
    /// Domain-model major; foreign majors reject.
    pub major: u32,
    /// Additive-evolution counter inside one major.
    pub minor: u32,
}

impl SchemaVersion {
    /// Highest version this build can read.
    #[must_use]
    pub const fn supported() -> Self {
        Self {
            major: crate::DOMAIN_MODEL_MAJOR,
            minor: crate::DOMAIN_MODEL_MINOR,
        }
    }

    /// Fail-closed readability check (unknown-version rejection): a foreign
    /// major or a minor newer than this build rejects (DOMAIN_MODEL.md §1).
    /// The `as u64` comparison keeps the check correct for any future minor
    /// constant without special-casing zero.
    #[must_use]
    pub const fn is_readable(&self) -> bool {
        self.major == crate::DOMAIN_MODEL_MAJOR
            && (self.minor as u64) <= (crate::DOMAIN_MODEL_MINOR as u64)
    }
}

impl fmt::Display for SchemaVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

impl Serialize for SchemaVersion {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct as _;
        let mut state = serializer.serialize_struct("SchemaVersion", 2)?;
        state.serialize_field("major", &self.major)?;
        state.serialize_field("minor", &self.minor)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for SchemaVersion {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = SchemaVersion;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a schema_version object with integer major and minor fields")
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut major: Option<u32> = None;
                let mut minor: Option<u32> = None;
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "major" => major = Some(map.next_value()?),
                        "minor" => minor = Some(map.next_value()?),
                        // v1.0 marks nothing forward-compatible; unknown
                        // members reject instead of being ignored.
                        _ => return Err(A::Error::custom("unknown field in schema_version")),
                    }
                }
                match (major, minor) {
                    (Some(major), Some(minor)) => SchemaVersion::try_from((major, minor))
                        .map_err(|error| A::Error::custom(error.to_string())),
                    _ => Err(A::Error::custom(
                        "schema_version requires exactly major and minor",
                    )),
                }
            }
        }
        deserializer.deserialize_struct("SchemaVersion", &["major", "minor"], Visitor)
    }
}

impl TryFrom<(u32, u32)> for SchemaVersion {
    type Error = DomainError;

    fn try_from(raw: (u32, u32)) -> Result<Self, Self::Error> {
        let found = Self {
            major: raw.0,
            minor: raw.1,
        };
        if found.is_readable() {
            Ok(found)
        } else {
            Err(DomainError::UnknownSchemaVersion {
                found,
                supported: Self::supported(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SchemaVersion;
    use crate::error::DomainError;

    #[test]
    fn supported_version_is_one_zero() {
        assert_eq!(
            SchemaVersion::supported(),
            SchemaVersion { major: 1, minor: 0 }
        );
    }

    #[test]
    fn readable_accepts_supported_version() {
        assert!(SchemaVersion { major: 1, minor: 0 }.is_readable());
    }

    #[test]
    fn readable_rejects_foreign_major() {
        for major in [0u32, 2, 7, u32::MAX] {
            let v = SchemaVersion { major, minor: 0 };
            assert!(!v.is_readable(), "major {major} must reject");
        }
    }

    #[test]
    fn readable_rejects_minor_newer_than_supported() {
        assert!(!SchemaVersion { major: 1, minor: 1 }.is_readable());
        assert!(
            !SchemaVersion {
                major: 1,
                minor: u32::MAX
            }
            .is_readable()
        );
    }

    #[test]
    fn try_from_accepts_supported_and_rejects_unknown() {
        assert_eq!(
            SchemaVersion::try_from((1, 0)).unwrap(),
            SchemaVersion { major: 1, minor: 0 }
        );
        assert_eq!(
            SchemaVersion::try_from((2, 0)),
            Err(DomainError::UnknownSchemaVersion {
                found: SchemaVersion { major: 2, minor: 0 },
                supported: SchemaVersion::supported(),
            })
        );
        assert!(SchemaVersion::try_from((1, 1)).is_err());
    }

    #[test]
    fn display_is_major_minor() {
        let v = SchemaVersion { major: 1, minor: 0 };
        assert_eq!(v.to_string(), "1.0");
    }

    #[test]
    fn serializes_as_explicit_object() {
        let v = SchemaVersion { major: 1, minor: 0 };
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(json, r#"{"major":1,"minor":0}"#);
    }
}
