//! Launch backends behind one object-safe boundary.
//!
//! Mirrors the keyboard-adapter discipline (`crate::backend`):
//!
//! - [`LaunchBackend`] is the object-safe port every backend implements.
//!   All FFI lives inside pinned audited dependencies; this crate keeps
//!   the workspace-wide `unsafe_code = "forbid"`.
//! - On Windows the real backend is [`WindowsLaunchBackend`]:
//!   application identities launch through direct CreateProcess-class
//!   spawning (`std::process::Command` — no shell interpreter, no argument
//!   interpolation, zero arguments this milestone, cleared environment,
//!   explicit working directory, nulled standard handles), and file/URL
//!   opens delegate to ShellExecute-class default-handler resolution via
//!   the pinned `open =5.4.1` wrapper with its opt-in
//!   `shellexecute-on-windows` feature enabled: opens run the direct
//!   `ShellExecuteW` path through the detached launcher (`that_detached`),
//!   with no shell intermediary anywhere.
//! - Every other platform gets [`UnsupportedLaunchBackend`]: an explicit,
//!   documented stub whose operations return [`LaunchError::Unsupported`].
//!   No fallback of any kind — honest capability reporting per repository
//!   norms.
//! - [`FakeLaunchBackend`] records invocations in memory for deterministic
//!   tests and CI. It is a test double only and is never a production
//!   fallback.
//!
//! Revalidation before spawn (taxonomy §6) lives in the real backend: an
//! application identity must still resolve inside its approved selection
//! map and the resolved executable must still exist at launch time;
//! otherwise the launch fails closed with [`LaunchError::MissingTarget`].
//!
//! Error values carry structural data only. No OS error text ever enters
//! an error channel.

use crate::launch::spec::{ApplicationTarget, FileTarget, UrlTarget};
use core::fmt;

/// Static platform tag used in `Unsupported` reports (shared with the
/// keyboard adapter's reporting).
pub use crate::backend::current_platform;

/// Typed launch failures. Variants carry no OS message text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LaunchError {
    /// No launch backend exists for this platform this milestone. OpenStream
    /// never falls back to a weaker mechanism; the capability gap is
    /// reported honestly instead.
    Unsupported {
        /// Platform tag (`macos`, `linux`, ...).
        platform: &'static str,
    },
    /// The approved target is gone or was never resolvable: the identity is
    /// outside the approved selection map, or the file/executable no longer
    /// exists. Explicit missing-target failure, never silent success.
    MissingTarget,
    /// The OS refused the launch operation. The OS error text is dropped;
    /// only the class survives.
    PlatformFailure,
}

impl fmt::Display for LaunchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { platform } => {
                write!(f, "no launch backend for {platform} (fail closed)")
            }
            Self::MissingTarget => f.write_str("launch target is missing"),
            Self::PlatformFailure => f.write_str("platform refused the launch"),
        }
    }
}

impl std::error::Error for LaunchError {}

/// One recorded fake invocation, in call order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchInvocation {
    /// An application launch was requested.
    Application(ApplicationTarget),
    /// A file open was requested.
    File(FileTarget),
    /// A URL open was requested.
    Url(UrlTarget),
}

/// Object-safe boundary over OS launch effects. Implementations must be
/// usable from the engine runtime (`Send + Sync`) and must only ever
/// *launch*: process enumeration, termination, window management, or input
/// synthesis are outside this contract forever.
pub trait LaunchBackend: fmt::Debug + Send + Sync {
    /// Launches one approved application identity with no arguments.
    ///
    /// # Errors
    /// [`LaunchError::Unsupported`] without a platform backend;
    /// [`LaunchError::MissingTarget`] when the identity cannot be resolved
    /// to a present executable; [`LaunchError::PlatformFailure`] when the
    /// OS refuses the spawn.
    fn launch_application(&self, target: &ApplicationTarget) -> Result<(), LaunchError>;

    /// Opens one approved file with its OS default handler.
    ///
    /// # Errors
    /// Same classes as [`Self::launch_application`], with
    /// [`LaunchError::MissingTarget`] when the file no longer exists.
    fn open_file(&self, target: &FileTarget) -> Result<(), LaunchError>;

    /// Opens one approved URL with the OS default handler.
    ///
    /// # Errors
    /// [`LaunchError::Unsupported`] without a platform backend;
    /// [`LaunchError::PlatformFailure`] when the OS refuses the open (a
    /// URL has no local existence to check, so no missing-target class
    /// applies).
    fn open_url(&self, target: &UrlTarget) -> Result<(), LaunchError>;
}

/// Real Windows backend. Application launches use direct CreateProcess-
/// class spawning of the approved executable path (resolved from the
/// selection map this backend is constructed with); file/URL opens use the
/// pinned `open =5.4.1` wrapper's detached launcher with its opt-in
/// `shellexecute-on-windows` feature enabled — direct `ShellExecuteW`, no
/// shell intermediary.
///
/// Launch posture (SECURITY.md process-execution constraints):
///
/// - no shell interpreter anywhere (`cmd`, PowerShell, and equivalents are
///   never invoked);
/// - no argument interpolation — application launches pass **zero**
///   arguments this milestone (typed argv belongs exclusively to the
///   separately gated `process.execute` registry row);
/// - clean environment (`env_clear`), explicit working directory (the
///   executable's parent), nulled standard handles — nothing is inherited;
/// - no elevation is requested and no privilege is escalated;
/// - revalidation before spawn: the identity must resolve in the approved
///   map and the executable must still exist, else
///   [`LaunchError::MissingTarget`].
#[cfg(target_os = "windows")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsLaunchBackend {
    applications: std::collections::BTreeMap<String, std::path::PathBuf>,
}

#[cfg(target_os = "windows")]
impl WindowsLaunchBackend {
    /// Creates the backend from the user-approved application selections:
    /// each entry maps one validated identity token to the exact
    /// executable selected by the user at approval time.
    #[must_use]
    pub fn new(applications: std::collections::BTreeMap<String, std::path::PathBuf>) -> Self {
        Self { applications }
    }

    fn open_via_handler(raw: &str, has_local_existence: bool) -> Result<(), LaunchError> {
        if has_local_existence && !std::path::Path::new(raw).exists() {
            return Err(LaunchError::MissingTarget);
        }
        open::that_detached(raw).map_err(|_| LaunchError::PlatformFailure)
    }
}

#[cfg(target_os = "windows")]
impl LaunchBackend for WindowsLaunchBackend {
    fn launch_application(&self, target: &ApplicationTarget) -> Result<(), LaunchError> {
        use std::process::{Command, Stdio};

        let Some(exe) = self.applications.get(target.identity()) else {
            return Err(LaunchError::MissingTarget);
        };
        if !exe.is_file() {
            return Err(LaunchError::MissingTarget);
        }
        let mut command = Command::new(exe);
        command.env_clear();
        if let Some(directory) = exe.parent() {
            command.current_dir(directory);
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
            .spawn()
            .map(|_| ())
            .map_err(|_| LaunchError::PlatformFailure)
    }

    fn open_file(&self, target: &FileTarget) -> Result<(), LaunchError> {
        Self::open_via_handler(target.as_str(), true)
    }

    fn open_url(&self, target: &UrlTarget) -> Result<(), LaunchError> {
        Self::open_via_handler(target.as_str(), false)
    }
}

/// Honest capability report for platforms without a shipped backend.
/// Operations fail closed with [`LaunchError::Unsupported`]; this type
/// deliberately launches nothing. Compiled on every platform so the
/// fail-closed contract is testable everywhere; production composition
/// roots only select it via [`platform_launch_backend`] off Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UnsupportedLaunchBackend {
    platform: &'static str,
}

impl UnsupportedLaunchBackend {
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

impl LaunchBackend for UnsupportedLaunchBackend {
    fn launch_application(&self, _target: &ApplicationTarget) -> Result<(), LaunchError> {
        Err(LaunchError::Unsupported {
            platform: self.platform,
        })
    }

    fn open_file(&self, _target: &FileTarget) -> Result<(), LaunchError> {
        Err(LaunchError::Unsupported {
            platform: self.platform,
        })
    }

    fn open_url(&self, _target: &UrlTarget) -> Result<(), LaunchError> {
        Err(LaunchError::Unsupported {
            platform: self.platform,
        })
    }
}

/// Deterministic in-memory fake recording every invocation in order, with
/// optional sticky failure injection for typed-error classification tests.
/// A test double proving the trait contract and driving CI determinism;
/// never a production fallback.
#[derive(Debug, Default)]
pub struct FakeLaunchBackend {
    invocations: std::sync::Mutex<Vec<LaunchInvocation>>,
    failure: std::sync::Mutex<Option<LaunchError>>,
}

impl FakeLaunchBackend {
    /// Fresh empty fake that reports success for every kind.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the sticky injected failure (`None` restores success). While
    /// set, every call fails with a copy of it and records nothing.
    pub fn set_failure(&self, failure: Option<LaunchError>) {
        *self
            .failure
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = failure;
    }

    fn record(&self, invocation: LaunchInvocation) -> Result<(), LaunchError> {
        let failure = *self
            .failure
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(error) = failure {
            return Err(error);
        }
        self.invocations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(invocation);
        Ok(())
    }

    /// Snapshots the recorded invocations in call order.
    #[must_use]
    pub fn invocations(&self) -> Vec<LaunchInvocation> {
        self.invocations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Number of recorded invocations.
    #[must_use]
    pub fn count(&self) -> usize {
        self.invocations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    /// Clears all recorded invocations and any injected failure.
    pub fn clear(&self) {
        self.invocations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
        self.set_failure(None);
    }
}

impl LaunchBackend for FakeLaunchBackend {
    fn launch_application(&self, target: &ApplicationTarget) -> Result<(), LaunchError> {
        self.record(LaunchInvocation::Application(target.clone()))
    }

    fn open_file(&self, target: &FileTarget) -> Result<(), LaunchError> {
        self.record(LaunchInvocation::File(target.clone()))
    }

    fn open_url(&self, target: &UrlTarget) -> Result<(), LaunchError> {
        self.record(LaunchInvocation::Url(target.clone()))
    }
}

/// Returns the platform's launch backend over the given user-approved
/// application selections.
///
/// - Windows: the real backend (CreateProcess-class spawns plus
///   ShellExecuteW-class handler opens).
/// - Everywhere else: the [`UnsupportedLaunchBackend`] stub. Callers keep
///   one code path and receive typed denials at operation time —
///   capability reporting stays honest at the point of use.
#[must_use]
pub fn platform_launch_backend(
    applications: std::collections::BTreeMap<String, std::path::PathBuf>,
) -> Box<dyn LaunchBackend> {
    #[cfg(target_os = "windows")]
    {
        Box::new(WindowsLaunchBackend::new(applications))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = applications;
        Box::new(UnsupportedLaunchBackend::new(current_platform()))
    }
}

#[cfg(test)]
mod tests {
    #[cfg(not(target_os = "windows"))]
    use super::platform_launch_backend;
    use super::{
        FakeLaunchBackend, LaunchBackend, LaunchError, LaunchInvocation, UnsupportedLaunchBackend,
        current_platform,
    };
    use crate::launch::spec::{ApplicationTarget, FileTarget, UrlTarget};
    use std::collections::BTreeMap;

    fn app(identity: &str) -> ApplicationTarget {
        ApplicationTarget::try_new(identity).expect("valid fixture")
    }

    fn file(path: &str) -> FileTarget {
        FileTarget::try_new(path).expect("valid fixture")
    }

    fn url(raw: &str) -> UrlTarget {
        UrlTarget::try_new(raw).expect("valid fixture")
    }

    #[test]
    fn fake_records_invocations_in_order_and_clears() {
        let fake = FakeLaunchBackend::new();
        fake.launch_application(&app("obs-studio")).expect("fake");
        fake.open_file(&file("/stage/cue.txt")).expect("fake");
        fake.open_url(&url("https://example.com/live"))
            .expect("fake");
        assert_eq!(fake.count(), 3);
        assert_eq!(
            fake.invocations(),
            vec![
                LaunchInvocation::Application(app("obs-studio")),
                LaunchInvocation::File(file("/stage/cue.txt")),
                LaunchInvocation::Url(url("https://example.com/live")),
            ]
        );
        fake.clear();
        assert_eq!(fake.count(), 0);
    }

    #[test]
    fn fake_failure_injection_is_sticky_and_blocks_recording() {
        let fake = FakeLaunchBackend::new();
        fake.set_failure(Some(LaunchError::MissingTarget));
        assert_eq!(
            fake.launch_application(&app("obs-studio")),
            Err(LaunchError::MissingTarget)
        );
        assert_eq!(fake.count(), 0, "failed calls must not record");
        fake.set_failure(None);
        fake.launch_application(&app("obs-studio")).expect("fake");
        assert_eq!(fake.count(), 1);
    }

    #[test]
    fn unsupported_platform_fails_closed_with_typed_error() {
        let backend = UnsupportedLaunchBackend::new(current_platform());
        assert_eq!(
            backend.launch_application(&app("obs-studio")),
            Err(LaunchError::Unsupported {
                platform: current_platform()
            })
        );
        assert_eq!(
            backend.open_file(&file("/stage/cue.txt")),
            Err(LaunchError::Unsupported {
                platform: current_platform()
            })
        );
        assert_eq!(
            backend.open_url(&url("https://example.com/live")),
            Err(LaunchError::Unsupported {
                platform: current_platform()
            })
        );
        assert_eq!(
            backend
                .launch_application(&app("x"))
                .unwrap_err()
                .to_string(),
            format!("no launch backend for {} (fail closed)", current_platform())
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn platform_selector_reports_unsupported_off_windows() {
        let error = platform_launch_backend(BTreeMap::new())
            .launch_application(&app("obs-studio"))
            .unwrap_err();
        assert!(matches!(error, LaunchError::Unsupported { .. }));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_backend_revalidates_identity_map_before_spawn() {
        use super::WindowsLaunchBackend;

        let mut applications = BTreeMap::new();
        applications.insert(
            "missing".to_string(),
            std::path::PathBuf::from("Z:\\definitely\\not\\present.exe"),
        );
        let backend = WindowsLaunchBackend::new(applications);
        // Identity outside the approved map fails closed.
        assert_eq!(
            backend.launch_application(&app("unapproved")),
            Err(LaunchError::MissingTarget)
        );
        // Approved identity whose executable vanished fails closed.
        assert_eq!(
            backend.launch_application(&app("missing")),
            Err(LaunchError::MissingTarget)
        );
    }
}
