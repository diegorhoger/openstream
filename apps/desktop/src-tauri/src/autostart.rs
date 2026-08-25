//! Opt-in autostart backend (issue #16).
//!
//! Autostart is OFF by default and changes ONLY through an explicit user
//! action (the tray menu toggle). The mechanism per platform is explicit
//! and documented in `docs/architecture/DESKTOP_LIFECYCLE.md`:
//!
//! - **Windows — shipped:** a `REG_SZ` value under
//!   `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` named
//!   `OpenStream`, holding the quoted path of the running executable.
//!   No elevation, no service, no scheduled task.
//! - **macOS / Linux — honest `Unsupported` for this milestone:** no
//!   LaunchAgent or XDG autostart entry is written by this build. The tray
//!   reports the capability as unavailable instead of pretending.
//!
//! Every failure is typed and closed-vocabulary: no registry text, no
//! paths, and no silent fallback ever flips autostart on.

use std::fmt;

/// Registry subkey holding per-user launch registrations on Windows.
#[cfg(target_os = "windows")]
pub const RUN_SUBKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Value name written under [`RUN_SUBKEY`] for this application.
#[cfg(target_os = "windows")]
pub const RUN_VALUE_NAME: &str = "OpenStream";

/// Which autostart operation failed; closed vocabulary for surfacing.
///
/// Constructed by the Windows registry backend and the test double;
/// reachability is therefore platform-conditional, hence the targeted
/// allowance (never a blanket lint weakening).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutostartOperation {
    /// Reading current registration state.
    Read,
    /// Registering launch-at-login.
    Enable,
    /// Removing the registration.
    Disable,
}

impl fmt::Display for AutostartOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read => f.write_str("read"),
            Self::Enable => f.write_str("enable"),
            Self::Disable => f.write_str("disable"),
        }
    }
}

/// Typed autostart failures; never carries OS message text or paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutostartError {
    /// This platform has no shipped autostart mechanism in this build.
    /// Constructed only by [`UnsupportedAutostart`], which itself exists
    /// only for platforms without a real backend — hence the targeted
    /// allowance for platform-dependent reachability.
    #[allow(dead_code)]
    Unsupported {
        /// Platform label from `std::env::consts::OS` (echo-safe).
        os: &'static str,
    },
    /// The concrete backend refused one operation. Constructed only by
    /// backends with real side effects (Windows registry, test double).
    #[allow(dead_code)]
    BackendRefused {
        /// Which operation was refused.
        operation: AutostartOperation,
    },
}

impl fmt::Display for AutostartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { os } => write!(f, "unsupported-on-{os}"),
            Self::BackendRefused { operation } => write!(f, "{operation}-refused"),
        }
    }
}

/// Whether launch-at-login is currently registered (OS truth).
///
/// Constructed by concrete backends only; reachability is therefore
/// platform-conditional (Windows backend / test double), hence the
/// targeted allowance.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutostartStatus {
    /// A launch registration exists.
    Enabled,
    /// No launch registration exists.
    Disabled,
}

/// Maps a registry key-open failure to an honest answer: ONLY a missing
/// subkey means "nothing registered" (healthy [`AutostartStatus::Disabled`]);
/// every other failure class — access denied, I/O errors, corruption — is a
/// refusal, because guessing Disabled would misreport OS truth as an
/// unchecked toggle.
#[cfg(target_os = "windows")]
fn map_registry_open_error(kind: std::io::ErrorKind) -> Result<AutostartStatus, AutostartError> {
    if kind == std::io::ErrorKind::NotFound {
        Ok(AutostartStatus::Disabled)
    } else {
        Err(AutostartError::BackendRefused {
            operation: AutostartOperation::Read,
        })
    }
}

/// Platform boundary for reading/toggling launch-at-login.
///
/// Implementations must be idempotent where meaningful: disabling an
/// unregistered state is success, not an error. `Send + Sync` lets the
/// composition root hold the backend inside shared shell state.
pub trait AutostartBackend: fmt::Debug + Send + Sync {
    /// Current OS-truth registration status.
    ///
    /// # Errors
    /// Typed backend failures only; never a guessed default.
    fn status(&self) -> Result<AutostartStatus, AutostartError>;

    /// Registers launch-at-login (explicit user action required upstream).
    ///
    /// # Errors
    /// Typed backend failures only; nothing is half-written.
    fn enable(&mut self) -> Result<(), AutostartError>;

    /// Removes the launch registration (idempotent).
    ///
    /// # Errors
    /// Typed backend failures only.
    fn disable(&mut self) -> Result<(), AutostartError>;
}

/// Backend for platforms without a shipped autostart mechanism this
/// milestone. Every operation reports [`AutostartError::Unsupported`] so
/// surfaces can render honest "unavailable" states instead of faking.
///
/// Constructed only on platforms without the real backend; Windows builds
/// ship [`WindowsRegistryAutostart`] instead, hence the targeted
/// `dead_code` allowance rather than a weakened lint.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub struct UnsupportedAutostart {
    os: &'static str,
}

#[allow(dead_code)]
impl UnsupportedAutostart {
    /// Backend reporting unsupported for the running host platform.
    #[must_use]
    pub fn for_current_os() -> Self {
        Self {
            os: match std::env::consts::OS {
                "macos" => "macos",
                "linux" => "linux",
                other => other,
            },
        }
    }
}

impl AutostartBackend for UnsupportedAutostart {
    fn status(&self) -> Result<AutostartStatus, AutostartError> {
        Err(AutostartError::Unsupported { os: self.os })
    }

    fn enable(&mut self) -> Result<(), AutostartError> {
        Err(AutostartError::Unsupported { os: self.os })
    }

    fn disable(&mut self) -> Result<(), AutostartError> {
        Err(AutostartError::Unsupported { os: self.os })
    }
}

/// Production backend selector for the composition root: real Windows
/// registry backend, honest `Unsupported` elsewhere.
#[must_use]
pub fn platform_default_backend() -> Box<dyn AutostartBackend> {
    #[cfg(target_os = "windows")]
    {
        Box::new(WindowsRegistryAutostart::system_default())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Box::new(UnsupportedAutostart::for_current_os())
    }
}

/// Windows registry backend: per-user `Run` key registration.
///
/// The subkey/value pair is injectable so tests exercise the REAL registry
/// code paths against scratch locations without touching the machine's
/// actual launch configuration.
#[cfg(target_os = "windows")]
#[derive(Debug, Clone)]
pub struct WindowsRegistryAutostart {
    subkey: String,
    value_name: String,
}

#[cfg(target_os = "windows")]
impl WindowsRegistryAutostart {
    /// Production location: the standard per-user `Run` key.
    #[must_use]
    pub fn system_default() -> Self {
        Self {
            subkey: RUN_SUBKEY.to_string(),
            value_name: RUN_VALUE_NAME.to_string(),
        }
    }

    /// Explicit location; exercised by the Windows-gated registry smoke
    /// tests against scratch subkeys they clean up afterwards.
    #[cfg(test)]
    #[must_use]
    pub fn with_location(subkey: impl Into<String>, value_name: impl Into<String>) -> Self {
        Self {
            subkey: subkey.into(),
            value_name: value_name.into(),
        }
    }

    fn quoted_exe_path() -> Result<String, AutostartError> {
        std::env::current_exe()
            .map_err(|_| AutostartError::BackendRefused {
                operation: AutostartOperation::Enable,
            })
            .map(|exe| format!("\"{}\"", exe.display()))
    }
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use super::WindowsRegistryAutostart;
    use super::{AutostartBackend, AutostartError, AutostartOperation, AutostartStatus};
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};

    impl WindowsRegistryAutostart {
        fn read_status_raw(&self, root: &RegKey) -> Result<AutostartStatus, AutostartError> {
            let run_key = match root.open_subkey_with_flags(&self.subkey, KEY_READ) {
                Ok(key) => key,
                Err(error) => return super::map_registry_open_error(error.kind()),
            };
            match run_key.get_raw_value(&self.value_name) {
                Ok(_) => Ok(AutostartStatus::Enabled),
                Err(ref error) if error.kind() == std::io::ErrorKind::NotFound => {
                    Ok(AutostartStatus::Disabled)
                }
                Err(_) => Err(AutostartError::BackendRefused {
                    operation: AutostartOperation::Read,
                }),
            }
        }

        fn write_registration(&self, root: &RegKey) -> Result<(), AutostartError> {
            let (run_key, _) =
                root.create_subkey(&self.subkey)
                    .map_err(|_| AutostartError::BackendRefused {
                        operation: AutostartOperation::Enable,
                    })?;
            let quoted = Self::quoted_exe_path()?;
            run_key.set_value(&self.value_name, &quoted).map_err(|_| {
                AutostartError::BackendRefused {
                    operation: AutostartOperation::Enable,
                }
            })
        }

        fn delete_registration(&self, root: &RegKey) -> Result<(), AutostartError> {
            let Ok(run_key) = root.open_subkey_with_flags(&self.subkey, KEY_WRITE) else {
                // Nothing registered at all: disable is idempotent success.
                return Ok(());
            };
            match run_key.delete_value(&self.value_name) {
                Ok(()) => Ok(()),
                Err(ref error) if error.kind() == std::io::ErrorKind::NotFound => {
                    // Nothing registered: disable stays idempotent success.
                    Ok(())
                }
                Err(_) => Err(AutostartError::BackendRefused {
                    operation: AutostartOperation::Disable,
                }),
            }
        }
    }

    impl AutostartBackend for WindowsRegistryAutostart {
        fn status(&self) -> Result<AutostartStatus, AutostartError> {
            self.read_status_raw(&RegKey::predef(HKEY_CURRENT_USER))
        }

        fn enable(&mut self) -> Result<(), AutostartError> {
            self.write_registration(&RegKey::predef(HKEY_CURRENT_USER))
        }

        fn disable(&mut self) -> Result<(), AutostartError> {
            self.delete_registration(&RegKey::predef(HKEY_CURRENT_USER))
        }
    }
}

#[cfg(all(test, target_os = "windows"))]
mod windows_tests {
    use super::{AutostartBackend, AutostartStatus, RUN_SUBKEY, WindowsRegistryAutostart};
    use std::process::id;
    use std::time::{SystemTime, UNIX_EPOCH};
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_WRITE};

    /// Unique scratch subtree under HKCU; never the production Run key.
    fn scratch_backend() -> (WindowsRegistryAutostart, String) {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let subkey = format!(r"Software\OpenStream\Tests\autostart-{}-{}", id(), nanos);
        let backend = WindowsRegistryAutostart::with_location(&subkey, "OpenStream.Test");
        (backend, subkey)
    }

    /// Serializes every test that touches the REAL registry. Cargo's default
    /// runner executes tests on parallel threads, and this module's scratch
    /// trees share global scaffolding parents (`Software\OpenStream[\\Tests]`)
    /// that one test's cleanup removes when empty. Without the guard, a
    /// concurrent test's create/read can race that removal and observe a
    /// spurious refusal. The returned guard is held for the test's ENTIRE
    /// body (bind it to a named variable, never `let _ = ...`, which drops
    /// immediately).
    fn registry_test_guard() -> std::sync::MutexGuard<'static, ()> {
        static REGISTRY_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        REGISTRY_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn cleanup(subkey: &str) {
        // Remove the whole scratch subtree; a missing parent is fine.
        let leaf = subkey.split('\\').next_back().unwrap_or(subkey);
        let root = RegKey::predef(HKEY_CURRENT_USER);
        let _ = root
            .open_subkey_with_flags(r"Software\OpenStream\Tests", KEY_WRITE)
            .and_then(|parent| parent.delete_subkey_all(leaf));
        // Then remove now-empty test scaffolding parents. delete_subkey
        // refuses non-empty keys, so this can never destroy anything the
        // tests did not create themselves. Removing the parents is safe ONLY
        // because every registry test holds `registry_test_guard` for its
        // whole body: no sibling test can hold or be creating a leaf under
        // this scaffolding while it runs.
        let _ = root
            .open_subkey_with_flags(r"Software\OpenStream", KEY_WRITE)
            .and_then(|parent| parent.delete_subkey("Tests"));
        let _ = root.delete_subkey(r"Software\OpenStream");
    }

    #[test]
    fn registry_open_failures_distinguish_disabled_from_refused() {
        use super::{AutostartError, AutostartOperation, AutostartStatus, map_registry_open_error};
        use std::io::ErrorKind;

        // A missing subkey is a healthy Disabled answer.
        assert_eq!(
            map_registry_open_error(ErrorKind::NotFound),
            Ok(AutostartStatus::Disabled)
        );
        // Access denied or any other registry I/O failure is a refusal:
        // reporting Disabled would show OS truth as an unchecked toggle.
        for refused_kind in [ErrorKind::PermissionDenied, ErrorKind::Other] {
            assert_eq!(
                map_registry_open_error(refused_kind),
                Err(AutostartError::BackendRefused {
                    operation: AutostartOperation::Read,
                })
            );
        }
    }

    #[test]
    fn absent_registration_reads_disabled() {
        let _registry_lock = registry_test_guard();
        let (backend, subkey) = scratch_backend();
        let outcome = backend.status();
        cleanup(&subkey);
        assert_eq!(outcome.expect("status"), AutostartStatus::Disabled);
    }

    #[test]
    fn enable_disable_round_trip_on_the_real_registry() {
        let _registry_lock = registry_test_guard();
        let (mut backend, subkey) = scratch_backend();

        let enabled = backend.enable().and_then(|()| backend.status());
        let after_enable = enabled.clone();

        let disabled = backend.disable().and_then(|()| backend.status());
        let after_disable = disabled.clone();

        cleanup(&subkey);

        assert_eq!(
            after_enable.expect("enabled status"),
            AutostartStatus::Enabled
        );
        assert_eq!(
            after_disable.expect("disabled status"),
            AutostartStatus::Disabled
        );
    }

    #[test]
    fn disable_is_idempotent_and_missing_key_is_disabled() {
        let _registry_lock = registry_test_guard();
        let (mut backend, subkey) = scratch_backend();

        // Disable before anything exists: idempotent success, still disabled.
        let first_disable = backend.disable().and_then(|()| backend.status());

        // Register, then disable twice: second disable stays success.
        backend.enable().expect("enable");
        backend.disable().expect("second disable");
        let final_status = backend.status();

        cleanup(&subkey);

        assert_eq!(
            first_disable.expect("first disable"),
            AutostartStatus::Disabled
        );
        assert_eq!(
            final_status.expect("final status"),
            AutostartStatus::Disabled
        );
    }

    #[test]
    fn system_default_targets_the_documented_run_subkey() {
        assert_eq!(
            WindowsRegistryAutostart::system_default().subkey,
            RUN_SUBKEY
        );
        assert_eq!(
            WindowsRegistryAutostart::system_default().value_name,
            "OpenStream"
        );
    }
}

/// Deterministic test double used by shell/menu tests on every platform.
/// NEVER wired into the production composition root.
#[cfg(test)]
#[derive(Debug, Default)]
pub struct FakeAutostart {
    enabled: bool,
    refuse_next: Option<AutostartOperation>,
    unsupported_os: Option<&'static str>,
}

#[cfg(test)]
impl FakeAutostart {
    pub fn enabled_now() -> Self {
        Self {
            enabled: true,
            ..Self::default()
        }
    }

    pub fn refusing(operation: AutostartOperation) -> Self {
        Self {
            refuse_next: Some(operation),
            ..Self::default()
        }
    }

    pub fn unsupported(os: &'static str) -> Self {
        Self {
            unsupported_os: Some(os),
            ..Self::default()
        }
    }
}

#[cfg(test)]
impl AutostartBackend for FakeAutostart {
    fn status(&self) -> Result<AutostartStatus, AutostartError> {
        if let Some(os) = self.unsupported_os {
            return Err(AutostartError::Unsupported { os });
        }
        if self.refuse_next == Some(AutostartOperation::Read) {
            return Err(AutostartError::BackendRefused {
                operation: AutostartOperation::Read,
            });
        }
        let status = if self.enabled {
            AutostartStatus::Enabled
        } else {
            AutostartStatus::Disabled
        };
        Ok(status)
    }

    fn enable(&mut self) -> Result<(), AutostartError> {
        if self.unsupported_os.is_some() {
            return Err(AutostartError::Unsupported {
                os: self.unsupported_os.unwrap_or("unknown"),
            });
        }
        if self.refuse_next == Some(AutostartOperation::Enable) {
            self.refuse_next = None;
            return Err(AutostartError::BackendRefused {
                operation: AutostartOperation::Enable,
            });
        }
        self.enabled = true;
        Ok(())
    }

    fn disable(&mut self) -> Result<(), AutostartError> {
        if self.unsupported_os.is_some() {
            return Err(AutostartError::Unsupported {
                os: self.unsupported_os.unwrap_or("unknown"),
            });
        }
        if self.refuse_next == Some(AutostartOperation::Disable) {
            self.refuse_next = None;
            return Err(AutostartError::BackendRefused {
                operation: AutostartOperation::Disable,
            });
        }
        self.enabled = false;
        Ok(())
    }
}
