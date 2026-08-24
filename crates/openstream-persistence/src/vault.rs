//! OS credential-vault abstraction (`THREAT_MODEL.md` TB6, `SECURITY.md`
//! hard rules).
//!
//! Secret values live only in OS credential storage; SQLite, logs, bundles,
//! sync payloads, and plugin memory never see them. This module defines the
//! single boundary the Engine integration broker uses to resolve a
//! [`SecretRef`] into a [`SecretValue`] for one approved typed operation:
//!
//! - [`CredentialVault`] is the object-safe trait surface (`store`, `load`,
//!   `delete`). Every operation is fail closed: missing entries surface
//!   [`VaultError::NotFound`], platform refusal surfaces
//!   [`VaultError::PlatformFailure`] (no OS error text is propagated, so no
//!   environment detail can leak through the error channel), and values are
//!   adopted only after [`openstream_domain::secret::SecretValue`]
//!   validation.
//! - On Windows the real backend is [`WindowsCredentialVault`], backed by
//!   Windows Credential Manager via the audited safe `keyring` wrapper
//!   (`windows-native` store only). Integration tests run against the live
//!   OS store on Windows.
//! - Every other platform gets [`UnsupportedVault`]: an explicit,
//!   documented stub whose operations return [`VaultError::Unsupported`].
//!   There is **no** plaintext-file fallback, **no** silent substitution,
//!   and **no** best-effort downgrade anywhere — honest capability
//!   reporting per repository norms. macOS keychain and Linux
//!   secret-service backends arrive with their own reviewed platform
//!   milestones.

use openstream_domain::secret::{SecretRef, SecretValue};
use std::fmt;

/// Service namespace under which every OpenStream entry lives in the OS
/// credential store; each secret reference becomes one entry inside it.
#[cfg(target_os = "windows")]
const VAULT_SERVICE: &str = "OpenStream";

/// The one approved operation a vault call performs. Carried by errors so
/// evidence can name the operation class without echoing any value material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VaultOperation {
    /// Persist (create or overwrite) the value behind a reference.
    Store,
    /// Resolve a reference into its value.
    Load,
    /// Delete the entry behind a reference.
    Delete,
}

impl fmt::Display for VaultOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl VaultOperation {
    /// Canonical lowercase token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Store => "store",
            Self::Load => "load",
            Self::Delete => "delete",
        }
    }
}

/// Typed vault failures. Variants carry only structural data: operation
/// kind and platform tag. No OS message text, reference, or value ever
/// enters an error (redaction rules, TM-LOG-01).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum VaultError {
    /// No credential-vault backend exists for this platform. OpenStream
    /// never falls back to plaintext storage; the capability gap is
    /// reported honestly instead.
    Unsupported {
        /// Platform tag (`macos`, `linux`, ...).
        platform: &'static str,
    },
    /// The referenced entry does not exist in the vault.
    NotFound {
        /// The operation that discovered the absence.
        operation: VaultOperation,
    },
    /// The stored entry could not be interpreted as a valid value (e.g.
    /// written out of band). Fail closed; callers must not use it.
    Corrupt {
        /// The operation that read the unusable entry.
        operation: VaultOperation,
    },
    /// The OS credential store refused the operation.
    PlatformFailure {
        /// The refused operation.
        operation: VaultOperation,
    },
}

impl VaultError {
    /// The operation this failure belongs to, when one applies.
    #[must_use]
    pub const fn operation(&self) -> Option<VaultOperation> {
        match self {
            Self::Unsupported { .. } => None,
            Self::NotFound { operation }
            | Self::Corrupt { operation }
            | Self::PlatformFailure { operation } => Some(*operation),
        }
    }
}

impl fmt::Display for VaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { platform } => {
                write!(
                    f,
                    "no credential vault backend for {platform} (fail closed)"
                )
            }
            Self::NotFound { operation } => write!(f, "{operation}: secret not found"),
            Self::Corrupt { operation } => {
                write!(f, "{operation}: stored secret is not usable")
            }
            Self::PlatformFailure { operation } => {
                write!(f, "{operation}: platform credential store refused")
            }
        }
    }
}

impl std::error::Error for VaultError {}

/// Object-safe boundary over OS credential storage. Implementations must be
/// usable from the engine runtime (`Send + Sync`) and must never log,
/// cache, or expose values outside returned [`SecretValue`] guards.
pub trait CredentialVault: fmt::Debug + Send + Sync {
    /// Persists (creating or overwriting) the value behind `secret_ref`.
    fn store(&self, secret_ref: &SecretRef, value: &SecretValue) -> Result<(), VaultError>;

    /// Resolves `secret_ref` into its value. Missing entries return
    /// [`VaultError::NotFound`]; unreadable entries return
    /// [`VaultError::Corrupt`].
    fn load(&self, secret_ref: &SecretRef) -> Result<SecretValue, VaultError>;

    /// Deletes the entry behind `secret_ref`. Deleting an absent entry
    /// returns [`VaultError::NotFound`] — deletion is honest, not idempotent.
    fn delete(&self, secret_ref: &SecretRef) -> Result<(), VaultError>;
}

/// Real Windows Credential Manager backend. All FFI lives inside the audited
/// `keyring` dependency (`windows-native` store); this crate keeps
/// `unsafe_code = forbid`.
#[cfg(target_os = "windows")]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct WindowsCredentialVault;

#[cfg(target_os = "windows")]
impl WindowsCredentialVault {
    /// Creates the backend handle. Construction cannot fail; failures
    /// surface per-operation as typed [`VaultError`] values.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    fn entry(
        &self,
        secret_ref: &SecretRef,
        operation: VaultOperation,
    ) -> Result<keyring::Entry, VaultError> {
        // Failure here means the platform store itself is unavailable; the
        // OS error text is dropped, only the class is kept.
        keyring::Entry::new(VAULT_SERVICE, secret_ref.as_str())
            .map_err(|_| VaultError::PlatformFailure { operation })
    }

    /// Maps a `keyring` error onto the typed taxonomy without propagating
    /// any OS message text.
    fn classify(operation: VaultOperation, error: &keyring::Error) -> VaultError {
        match error {
            keyring::Error::NoEntry => VaultError::NotFound { operation },
            _ => VaultError::PlatformFailure { operation },
        }
    }
}

#[cfg(target_os = "windows")]
impl CredentialVault for WindowsCredentialVault {
    fn store(&self, secret_ref: &SecretRef, value: &SecretValue) -> Result<(), VaultError> {
        let entry = self.entry(secret_ref, VaultOperation::Store)?;
        entry
            .set_password(value.expose())
            .map_err(|error| Self::classify(VaultOperation::Store, &error))
    }

    fn load(&self, secret_ref: &SecretRef) -> Result<SecretValue, VaultError> {
        let entry = self.entry(secret_ref, VaultOperation::Load)?;
        let password = entry
            .get_password()
            .map_err(|error| Self::classify(VaultOperation::Load, &error))?;
        // Adopt through the domain guard so corrupt/out-of-band entries
        // (empty, NUL byte, oversized) refuse instead of flowing onward.
        SecretValue::try_new(password).map_err(|_| VaultError::Corrupt {
            operation: VaultOperation::Load,
        })
    }

    fn delete(&self, secret_ref: &SecretRef) -> Result<(), VaultError> {
        let entry = self.entry(secret_ref, VaultOperation::Delete)?;
        entry
            .delete_credential()
            .map_err(|error| Self::classify(VaultOperation::Delete, &error))
    }
}

/// Honest capability report for platforms without a shipped backend.
/// Operations fail closed with [`VaultError::Unsupported`]; this type
/// deliberately stores nothing (no files, no environment, no memory).
#[cfg(not(target_os = "windows"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UnsupportedVault {
    platform: &'static str,
}

#[cfg(not(target_os = "windows"))]
impl UnsupportedVault {
    /// Records the platform tag surfaced by every failing operation.
    #[must_use]
    pub const fn new(platform: &'static str) -> Self {
        Self { platform }
    }

    /// The platform tag this stub reports.
    #[must_use]
    pub const fn platform(&self) -> &'static str {
        self.platform
    }
}

#[cfg(not(target_os = "windows"))]
impl CredentialVault for UnsupportedVault {
    fn store(&self, _secret_ref: &SecretRef, _value: &SecretValue) -> Result<(), VaultError> {
        Err(self.unsupported())
    }

    fn load(&self, _secret_ref: &SecretRef) -> Result<SecretValue, VaultError> {
        Err(self.unsupported())
    }

    fn delete(&self, _secret_ref: &SecretRef) -> Result<(), VaultError> {
        Err(self.unsupported())
    }
}

#[cfg(not(target_os = "windows"))]
impl UnsupportedVault {
    fn unsupported(&self) -> VaultError {
        VaultError::Unsupported {
            platform: self.platform,
        }
    }
}

/// Static platform tag used in `Unsupported` reports.
#[cfg(not(target_os = "windows"))]
const fn current_platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unsupported-platform"
    }
}

/// Returns the platform's credential-vault backend.
///
/// - Windows: the real Windows Credential Manager backend.
/// - Everywhere else: the [`UnsupportedVault`] stub. Callers keep one code
///   path and receive typed denials at operation time — capability
///   reporting stays honest at the point of use.
#[must_use]
pub fn platform_vault() -> Box<dyn CredentialVault> {
    #[cfg(target_os = "windows")]
    {
        Box::new(WindowsCredentialVault::new())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Box::new(UnsupportedVault::new(current_platform()))
    }
}

#[cfg(test)]
mod tests {
    use super::{CredentialVault, SecretRef, SecretValue, VaultError, VaultOperation};
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    #[test]
    fn errors_are_structural_only() {
        let load = VaultOperation::Load.as_str();
        assert_eq!(
            VaultError::NotFound {
                operation: VaultOperation::Load
            }
            .to_string(),
            format!("{load}: secret not found")
        );
        assert_eq!(
            VaultError::Unsupported { platform: "linux" }.to_string(),
            "no credential vault backend for linux (fail closed)"
        );
        let corrupt = VaultError::Corrupt {
            operation: VaultOperation::Delete,
        };
        assert_eq!(corrupt.operation(), Some(VaultOperation::Delete));
        assert_eq!(
            VaultError::Unsupported { platform: "macos" }.operation(),
            None
        );
    }

    #[test]
    fn fake_vault_round_trip_through_trait_object() {
        // Proves object safety and the full contract: store → load equality
        // → delete → NotFound → double-delete NotFound.
        let vault: Box<dyn CredentialVault> = Box::new(FakeVault::default());
        let reference = SecretRef::try_new("obs.test.roundtrip").unwrap();
        let value = SecretValue::try_new("osv-synthetic-value".to_string()).unwrap();

        vault.store(&reference, &value).unwrap();
        let loaded = vault.load(&reference).unwrap();
        assert_eq!(loaded.expose(), "osv-synthetic-value");

        vault.delete(&reference).unwrap();
        assert_eq!(
            vault.load(&reference).unwrap_err(),
            VaultError::NotFound {
                operation: VaultOperation::Load
            }
        );
        assert_eq!(
            vault.delete(&reference).unwrap_err(),
            VaultError::NotFound {
                operation: VaultOperation::Delete
            }
        );
    }

    #[test]
    fn fake_vault_reports_corrupt_out_of_band_entries() {
        let vault = FakeVault::default();
        let reference = SecretRef::try_new("obs.test.corrupt").unwrap();
        vault
            .entries
            .lock()
            .unwrap()
            .insert(reference.as_str().to_string(), String::new());
        assert_eq!(
            vault.load(&reference).unwrap_err(),
            VaultError::Corrupt {
                operation: VaultOperation::Load
            }
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn unsupported_platform_fails_closed_on_every_operation() {
        use super::{current_platform, platform_vault};

        let vault = platform_vault();
        let reference = SecretRef::try_new("obs.test.unsupported").unwrap();
        let value = SecretValue::try_new("osv-synthetic-value".to_string()).unwrap();
        let expected = VaultError::Unsupported {
            platform: current_platform(),
        };
        let stored = vault.store(&reference, &value).unwrap_err();
        let loaded = vault.load(&reference).unwrap_err();
        let deleted = vault.delete(&reference).unwrap_err();
        assert_eq!(stored, expected);
        assert_eq!(loaded, expected);
        assert_eq!(deleted, expected);
    }

    /// Deterministic in-memory fake proving the trait contract; a test
    /// double only, never a production fallback.
    #[derive(Default, Debug)]
    struct FakeVault {
        entries: Mutex<BTreeMap<String, String>>,
    }

    impl CredentialVault for FakeVault {
        fn store(&self, secret_ref: &SecretRef, value: &SecretValue) -> Result<(), VaultError> {
            self.entries
                .lock()
                .unwrap()
                .insert(secret_ref.as_str().to_string(), value.expose().to_string());
            Ok(())
        }

        fn load(&self, secret_ref: &SecretRef) -> Result<SecretValue, VaultError> {
            match self.entries.lock().unwrap().get(secret_ref.as_str()) {
                Some(value) => {
                    SecretValue::try_new(value.clone()).map_err(|_| VaultError::Corrupt {
                        operation: VaultOperation::Load,
                    })
                }
                None => Err(VaultError::NotFound {
                    operation: VaultOperation::Load,
                }),
            }
        }

        fn delete(&self, secret_ref: &SecretRef) -> Result<(), VaultError> {
            let removed = self.entries.lock().unwrap().remove(secret_ref.as_str());
            match removed {
                Some(_) => Ok(()),
                None => Err(VaultError::NotFound {
                    operation: VaultOperation::Delete,
                }),
            }
        }
    }
}
