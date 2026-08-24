//! Secret references and redacted secret values (`TECHNICAL_SPEC.md` §4,
//! `THREAT_MODEL.md` TB6).
//!
//! - [`SecretRef`] is a *structural address* of one secret in OS credential
//!   storage (e.g. `obs.scene.notes`). It is a name, never secret
//!   material, and its grammar is validated fail closed.
//! - [`SecretValue`] wraps the actual value bytes for the short path between
//!   the OS credential vault and the integration broker. Its [`fmt::Debug`]
//!   prints `[REDACTED]`, its `Serialize`/`Deserialize` implementations
//!   **fail** (secret values are never serialized anywhere — TM-LOG-01 hard
//!   rule), and the buffer is zeroized on drop.
//!
//! Nothing in this module performs IO; resolution happens only behind the
//! credential-vault boundary in `openstream-persistence`.

use crate::error::DomainError;
use crate::limits::{MAX_SECRET_REF_BYTES, MAX_SECRET_VALUE_BYTES};
use core::fmt;
use serde::de::Error as DeError;
use serde::ser::Error as SerError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::str::FromStr;
use zeroize::Zeroize;

/// Maximum number of dot-separated segments in a secret reference.
const MAX_SECRET_REF_SEGMENTS: usize = 8;

/// The placeholder every debug rendering of a secret value shows instead of
/// material.
pub const SECRET_REDACTED: &str = "[REDACTED]";

/// Validated structural reference to one secret stored in OS credential
/// storage. Grammar: 1–8 lowercase dotted segments, each starting with an
/// ASCII letter and continuing with `[a-z0-9_-]`; at most
/// [`MAX_SECRET_REF_BYTES`] UTF-8 bytes total.
///
/// A reference is an address, not secret material; it may appear in grants
/// (`secret.read:<secret_ref>` is internal-only), configuration, and typed
/// errors without violating redaction rules.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SecretRef(String);

impl SecretRef {
    /// Validates a raw reference string fail closed. The rejection reason is
    /// structural only; the rejected input never enters the error.
    pub fn try_new(raw: &str) -> Result<Self, DomainError> {
        if raw.is_empty() || raw.len() > MAX_SECRET_REF_BYTES {
            return Err(DomainError::InvalidSecretRef {
                reason: "reference length out of range",
            });
        }
        if raw.trim() != raw {
            return Err(DomainError::InvalidSecretRef {
                reason: "surrounding whitespace",
            });
        }
        let segments: Vec<&str> = raw.split('.').collect();
        if segments.len() > MAX_SECRET_REF_SEGMENTS {
            return Err(DomainError::InvalidSecretRef {
                reason: "too many segments",
            });
        }
        for segment in segments {
            validate_segment(segment)?;
        }
        Ok(Self(raw.to_string()))
    }

    /// The structural reference string (a name, not secret material).
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn validate_segment(segment: &str) -> Result<(), DomainError> {
    let mut chars = segment.chars();
    let Some(first) = chars.next() else {
        return Err(DomainError::InvalidSecretRef {
            reason: "empty segment",
        });
    };
    if !first.is_ascii_lowercase()
        || !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
    {
        return Err(DomainError::InvalidSecretRef {
            reason: "invalid segment character",
        });
    }
    Ok(())
}

impl fmt::Display for SecretRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for SecretRef {
    type Err = DomainError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        Self::try_new(raw)
    }
}

impl Serialize for SecretRef {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for SecretRef {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        SecretRef::try_new(&raw).map_err(DeError::custom)
    }
}

/// One secret value held in memory only for the shortest possible path
/// between OS credential storage and the integration broker performing one
/// approved operation.
///
/// Guarantees:
/// - [`fmt::Debug`] renders [`SECRET_REDACTED`], never the contents.
/// - `serde::Serialize` always fails: values are never serialized into
///   logs, journals, bundles, sync payloads, or fixtures (TM-LOG-01).
/// - `serde::Deserialize` always fails: no serialized form exists to
///   rehydrate, so nothing can smuggle a value back through serde.
/// - The backing buffer is zeroized when the value is dropped.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretValue {
    value: String,
}

impl SecretValue {
    /// Adopts a secret value fail closed: empty values, embedded NUL bytes,
    /// and values beyond [`MAX_SECRET_VALUE_BYTES`] reject (the byte bound
    /// matches the tightest supported platform blob limit so every backend
    /// shares one contract).
    pub fn try_new(value: String) -> Result<Self, DomainError> {
        if value.is_empty() {
            return Err(DomainError::InvalidSecretValue {
                reason: "value must not be empty",
            });
        }
        if value.len() > MAX_SECRET_VALUE_BYTES {
            return Err(DomainError::InvalidSecretValue {
                reason: "value exceeds the platform blob limit",
            });
        }
        if value.contains('\0') {
            // Zeroize the rejected buffer before it drops unscrubbed.
            let mut rejected = value;
            rejected.zeroize();
            return Err(DomainError::InvalidSecretValue {
                reason: "value must not contain NUL bytes",
            });
        }
        Ok(Self { value })
    }

    /// Borrows the value for direct use by the integration broker. This is
    /// the only way to observe secret material.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.value
    }
}

impl Drop for SecretValue {
    fn drop(&mut self) {
        self.value.zeroize();
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretValue(")?;
        f.write_str(SECRET_REDACTED)?;
        f.write_str(")")
    }
}

impl Serialize for SecretValue {
    fn serialize<S: Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
        Err(SerError::custom(
            "secret values are never serialized (TM-LOG-01)",
        ))
    }
}

impl<'de> Deserialize<'de> for SecretValue {
    fn deserialize<D: Deserializer<'de>>(_deserializer: D) -> Result<Self, D::Error> {
        Err(DeError::custom(
            "secret values cannot be deserialized (TM-LOG-01)",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{SECRET_REDACTED, SecretRef, SecretValue};
    use crate::error::DomainError;
    use crate::limits::{MAX_SECRET_REF_BYTES, MAX_SECRET_VALUE_BYTES};
    use std::str::FromStr as _;

    fn assert_ref_reason(error: &DomainError, reason: &str) {
        match error {
            DomainError::InvalidSecretRef { reason: found } => assert_eq!(*found, reason),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn references_validate_strictly_and_round_trip() {
        for raw in ["obs.scene.notes", "inbox-7", "peer0.device.v3_x", "a"] {
            let parsed = SecretRef::from_str(raw).unwrap();
            assert_eq!(parsed.as_str(), raw);
            assert_eq!(parsed.to_string(), raw);
        }
        for bad in [
            "",
            ".lead",
            "trail.",
            "dou..ble",
            "Upper.case",
            "9digit.start",
            "sp ace",
            " lead",
            "tab\tinside",
            "sym!bol",
        ] {
            assert!(SecretRef::from_str(bad).is_err(), "{bad:?} must reject");
        }
        assert_ref_reason(
            &SecretRef::from_str("").unwrap_err(),
            "reference length out of range",
        );
        let long = format!("a.{}", "b".repeat(MAX_SECRET_REF_BYTES));
        assert_ref_reason(
            &SecretRef::from_str(&long).unwrap_err(),
            "reference length out of range",
        );
        let many = "a.b.c.d.e.f.g.h.i";
        assert_ref_reason(&SecretRef::from_str(many).unwrap_err(), "too many segments");
        assert_eq!(
            SecretRef::from_str("a.b.c.d.e.f.g.h").unwrap().as_str(),
            "a.b.c.d.e.f.g.h"
        );
    }

    #[test]
    fn references_serialize_only_the_structural_name() {
        let reference = SecretRef::from_str("obs.scene.notes").unwrap();
        let json = serde_json::to_string(&reference).unwrap();
        assert_eq!(json, "\"obs.scene.notes\"");
        let back: SecretRef = serde_json::from_str(&json).unwrap();
        assert_eq!(back, reference);
        assert!(serde_json::from_str::<SecretRef>("\"Bad Ref\"").is_err());
    }

    #[test]
    fn values_fail_closed_on_empty_nul_and_oversize() {
        assert!(SecretValue::try_new(String::new()).is_err());
        match SecretValue::try_new("nul\0byte".to_string()).unwrap_err() {
            DomainError::InvalidSecretValue { reason } => {
                assert_eq!(reason, "value must not contain NUL bytes")
            }
            other => panic!("unexpected error {other:?}"),
        }
        let oversized = "x".repeat(MAX_SECRET_VALUE_BYTES + 1);
        assert!(SecretValue::try_new(oversized).is_err());
        // Boundary values accept.
        let max_value = "y".repeat(MAX_SECRET_VALUE_BYTES);
        assert!(SecretValue::try_new(max_value).is_ok());
    }

    #[test]
    fn debug_never_shows_material_and_serialize_always_fails() {
        let marker = "s3cr3t-material-ZQ81";
        let value = SecretValue::try_new(marker.to_string()).unwrap();
        assert_eq!(
            format!("{value:?}"),
            format!("SecretValue({SECRET_REDACTED})")
        );
        assert!(!format!("{value:?}").contains(marker));
        assert!(value.expose() == marker);
        // Serialization fails loudly and the failure message carries no
        // material either.
        let result = serde_json::to_string(&value);
        assert!(result.is_err());
        assert!(!result.unwrap_err().to_string().contains(marker));
        // Deserialization fails closed too.
        assert!(serde_json::from_str::<SecretValue>("\"anything\"").is_err());
    }
}
