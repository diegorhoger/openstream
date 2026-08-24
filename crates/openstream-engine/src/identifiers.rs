//! Protocol-facing identities owned at the engine boundary.
//!
//! Durable entity identifiers live in `openstream-domain::ids` (UUIDv7,
//! canonical lowercase hyphenated). The admission envelope contributes two
//! further identities this crate validates with the same discipline:
//!
//! - [`MessageId`] — the globally unique envelope message id (UUIDv7,
//!   `OSCP_MESSAGES.md` §1); half of the dedupe key.
//! - [`SourceDeviceId`] — the trusted source identity forming the other
//!   half: peer id for LAN/mobile, installation id for desktop-local IPC,
//!   Engine-mapped membership for Cloud relay (`OSCP_MESSAGES.md` §7).
//!
//! Both fail closed on malformed input; neither ever echoes rejected
//! content beyond the structural value itself (identifiers are echo-safe
//! per repository redaction rules).

use core::fmt;
use std::str::FromStr;
use uuid::Uuid;

/// Globally unique envelope message identifier (canonical UUIDv7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MessageId(Uuid);

impl MessageId {
    /// Mints a fresh message id. Engine-side generation exists for tests
    /// and local IPC; remote senders mint their own.
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

impl fmt::Display for MessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for MessageId {
    type Err = InvalidIdentity;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let uuid = Uuid::parse_str(s).map_err(|_| InvalidIdentity)?;
        if uuid.get_version_num() != 7 || s != uuid.to_string() {
            return Err(InvalidIdentity);
        }
        Ok(Self(uuid))
    }
}

/// Trusted source-device identity for the admission dedupe key. Structural,
/// bounded, printable ASCII; no wildcards, whitespace, or control bytes so
/// journal keys stay deterministic and injection-safe.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SourceDeviceId(String);

impl SourceDeviceId {
    /// Maximum byte length of one device identity.
    pub const MAX_BYTES: usize = 128;

    /// Validates and adopts a device identity. Accepts `[A-Za-z0-9]` first
    /// character then `[A-Za-z0-9._:-]`, 1..=[`Self::MAX_BYTES`] bytes.
    ///
    /// # Errors
    /// Returns [`InvalidIdentity`] for empty, oversized, or off-grammar
    /// input.
    pub fn try_new(raw: &str) -> Result<Self, InvalidIdentity> {
        valid_identity(raw).map(Self)
    }

    /// The structural identity string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SourceDeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for SourceDeviceId {
    type Err = InvalidIdentity;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_new(s)
    }
}

fn valid_identity(raw: &str) -> Result<String, InvalidIdentity> {
    if raw.is_empty() || raw.len() > SourceDeviceId::MAX_BYTES {
        return Err(InvalidIdentity);
    }
    let mut chars = raw.chars();
    let first = chars.next().unwrap_or_default();
    if !first.is_ascii_alphanumeric() {
        return Err(InvalidIdentity);
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '-')) {
        return Err(InvalidIdentity);
    }
    Ok(raw.to_string())
}

/// Typed rejection for malformed protocol identities. Deliberately
/// content-free: no rejected text is echoed back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InvalidIdentity;

impl fmt::Display for InvalidIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("identity is not in canonical form")
    }
}

impl std::error::Error for InvalidIdentity {}

/// Identifier grammar shared by node keys, action names, and variable
/// names: lowercase ASCII start, then lowercase ASCII plus `_`, `-` (and
/// `.` for action names when `allow_dot`), 1..=`max` bytes.
pub(crate) fn validate_identifier(raw: &str, allow_dot: bool, max: usize) -> bool {
    if raw.is_empty() || raw.len() > max {
        return false;
    }
    let mut chars = raw.chars();
    let first = chars.next().unwrap_or_default();
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return false;
    }
    chars.all(|c| {
        c.is_ascii_lowercase()
            || c.is_ascii_digit()
            || c == '_'
            || c == '-'
            || (allow_dot && c == '.')
    })
}

#[cfg(test)]
mod tests {
    use super::{InvalidIdentity, MessageId, SourceDeviceId, validate_identifier};
    use std::str::FromStr as _;

    #[test]
    fn message_ids_are_canonical_v7_only() {
        let id = MessageId::generate();
        let text = id.to_string();
        assert_eq!(MessageId::from_str(&text).unwrap(), id);
        assert_eq!(id.as_uuid().get_version_num(), 7);
        // v4 sample rejects; uppercase rejects.
        assert_eq!(
            MessageId::from_str("3b241101-e2bb-4255-8caf-4136c566a962"),
            Err(InvalidIdentity)
        );
        assert!(MessageId::from_str(&text.to_ascii_uppercase()).is_err());
        assert_eq!(MessageId::from_str("nope"), Err(InvalidIdentity));
    }

    #[test]
    fn source_device_grammar_is_strict() {
        assert!(SourceDeviceId::try_new("peer:018f6a1c-7b21-7cc0-9f31-0e3d5a9d4c11").is_ok());
        assert!(SourceDeviceId::try_new("installation.local").is_ok());
        assert!(SourceDeviceId::try_new("").is_err());
        assert!(SourceDeviceId::try_new(" lead").is_err());
        assert!(SourceDeviceId::try_new("trail ").is_err());
        assert!(SourceDeviceId::try_new("has space").is_err());
        assert!(SourceDeviceId::try_new("wild*card").is_err());
        assert!(SourceDeviceId::try_new("-lead").is_err());
        let oversized = "a".repeat(SourceDeviceId::MAX_BYTES + 1);
        assert!(SourceDeviceId::try_new(&oversized).is_err());
        let adopted = SourceDeviceId::try_new("device-1").unwrap();
        assert_eq!(adopted.as_str(), "device-1");
        assert_eq!(SourceDeviceId::from_str("device-1").unwrap(), adopted);
    }

    #[test]
    fn identifier_helper_covers_name_shapes() {
        assert!(validate_identifier("node-1", false, 64));
        assert!(validate_identifier("obs.scene.set", true, 64));
        assert!(!validate_identifier("obs.scene.set", false, 64));
        assert!(!validate_identifier("Upper", false, 64));
        assert!(!validate_identifier("", false, 64));
        assert!(!validate_identifier("x", false, 0));
        assert!(!validate_identifier("toolong", false, 3));
    }
}
