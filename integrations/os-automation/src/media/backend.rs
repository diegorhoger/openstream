//! Media transport and volume backends behind one object-safe boundary.
//!
//! Mirrors the keyboard/launch adapter discipline:
//!
//! - [`MediaDeviceController`] is the object-safe port every backend
//!   implements. All FFI lives inside the pinned audited `enigo`
//!   dependency (Windows build only); this crate keeps the workspace-wide
//!   `unsafe_code = "forbid"`.
//! - On Windows the real backend is [`WindowsMediaBackend`]: transport
//!   commands synthesize the standard media keys (play/pause, next,
//!   previous) and volume operations synthesize the standard volume keys
//!   (up/down/mute), all through SendInput-class synthesis inside the same
//!   audited wrapper the keyboard adapter uses. Volume keys act on the OS
//!   default render endpoint — the master device scope this milestone
//!   declares; no per-session or per-endpoint enumeration exists anywhere
//!   in this crate, and none may be approximated by silently widening to a
//!   different scope.
//! - Every other platform gets [`UnsupportedMediaBackend`]: an explicit,
//!   documented stub whose operations return
//!   [`MediaError::Unsupported`]. No fallback of any kind — honest
//!   capability reporting per repository norms.
//! - [`FakeMediaBackend`] records invocations in memory for deterministic
//!   tests and CI, with sticky failure injection for typed-error
//!   classification tests. It is a test double only and is never a
//!   production fallback.
//!
//! Error values carry structural data only (operation class, platform
//! tag). No OS error text ever enters an error channel.

use crate::media::spec::{MediaCommand, VolumeOperation};
use core::fmt;

/// Static platform tag used in `Unsupported` reports (shared with the
/// keyboard and launch adapters).
pub use crate::backend::current_platform;

/// Typed media/volume failures. Variants carry no OS message text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaError {
    /// No media/volume backend exists for this platform this milestone.
    /// OpenStream never falls back to a weaker mechanism; the capability
    /// gap is reported honestly instead.
    Unsupported {
        /// Platform tag (`macos`, `linux`, ...).
        platform: &'static str,
    },
    /// The OS refused the operation. The OS error text is dropped; only
    /// the class survives.
    PlatformFailure,
}

impl fmt::Display for MediaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { platform } => {
                write!(f, "no media/volume backend for {platform} (fail closed)")
            }
            Self::PlatformFailure => f.write_str("platform refused the media/volume effect"),
        }
    }
}

impl std::error::Error for MediaError {}

/// One recorded fake invocation, in call order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaInvocation {
    /// A transport command was requested.
    Transport(MediaCommand),
    /// A volume operation was requested.
    Volume(VolumeOperation),
}

/// Object-safe boundary over OS media-transport and volume effects.
/// Implementations must be usable from the engine runtime (`Send + Sync`)
/// and must only ever *send* one-shot control events: audio capture,
/// session/process enumeration, playback-state polling, or logging of user
/// behavior are outside this contract forever.
pub trait MediaDeviceController: fmt::Debug + Send + Sync {
    /// Sends one validated transport command to the active OS media
    /// session.
    ///
    /// # Errors
    /// [`MediaError::Unsupported`] without a platform backend;
    /// [`MediaError::PlatformFailure`] when the OS refuses the effect.
    fn send_transport(&self, command: &MediaCommand) -> Result<(), MediaError>;

    /// Applies one validated volume operation on the granted device scope
    /// (`master`: the default render endpoint).
    ///
    /// # Errors
    /// [`MediaError::Unsupported`] without a platform backend;
    /// [`MediaError::PlatformFailure`] when the OS refuses the effect.
    fn adjust_volume(&self, operation: &VolumeOperation) -> Result<(), MediaError>;
}

/// Real Windows backend: SendInput-class synthesis through the pinned
/// `enigo` wrapper (`default-features = false`; no other platform's stack
/// is compiled). Holds no state: one synthesizer instance is created per
/// call so nothing outlives it.
///
/// Scope note: the standard volume keys address the default render
/// endpoint (the declared `device=master` scope). This backend implements
/// no per-session or per-endpoint targeting; requests for any other device
/// scope are refused at the port before a backend call can happen, so the
/// mechanism can never exceed the named-device authority the grant names.
#[cfg(target_os = "windows")]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct WindowsMediaBackend;

#[cfg(target_os = "windows")]
impl WindowsMediaBackend {
    /// Creates the backend handle. Construction cannot fail; failures
    /// surface per-operation as typed [`MediaError`] values.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    fn map_command(command: MediaCommand) -> enigo::Key {
        match command {
            MediaCommand::PlayPause => enigo::Key::MediaPlayPause,
            MediaCommand::NextTrack => enigo::Key::MediaNextTrack,
            MediaCommand::PreviousTrack => enigo::Key::MediaPrevTrack,
        }
    }

    fn send_click(enigo: &mut enigo::Enigo, key: enigo::Key) -> Result<(), MediaError> {
        use enigo::{Direction, Keyboard as _};
        enigo
            .key(key, Direction::Click)
            .map_err(|_| MediaError::PlatformFailure)
    }
}

#[cfg(target_os = "windows")]
impl MediaDeviceController for WindowsMediaBackend {
    fn send_transport(&self, command: &MediaCommand) -> Result<(), MediaError> {
        use enigo::{Enigo, Settings};

        let mut enigo =
            Enigo::new(&Settings::default()).map_err(|_| MediaError::PlatformFailure)?;
        Self::send_click(&mut enigo, Self::map_command(*command))
    }

    fn adjust_volume(&self, operation: &VolumeOperation) -> Result<(), MediaError> {
        use enigo::{Enigo, Settings};

        let mut enigo =
            Enigo::new(&Settings::default()).map_err(|_| MediaError::PlatformFailure)?;
        match *operation {
            VolumeOperation::Up { steps } => {
                for _ in 0..steps {
                    Self::send_click(&mut enigo, enigo::Key::VolumeUp)?;
                }
                Ok(())
            }
            VolumeOperation::Down { steps } => {
                for _ in 0..steps {
                    Self::send_click(&mut enigo, enigo::Key::VolumeDown)?;
                }
                Ok(())
            }
            VolumeOperation::ToggleMute => Self::send_click(&mut enigo, enigo::Key::VolumeMute),
        }
    }
}

/// Honest capability report for platforms without a shipped backend.
/// Operations fail closed with [`MediaError::Unsupported`]; this type
/// deliberately controls nothing and captures nothing. Compiled on every
/// platform so the fail-closed contract is testable everywhere;
/// production composition roots only select it via
/// [`platform_media_backend`] off Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UnsupportedMediaBackend {
    platform: &'static str,
}

impl UnsupportedMediaBackend {
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

impl MediaDeviceController for UnsupportedMediaBackend {
    fn send_transport(&self, _command: &MediaCommand) -> Result<(), MediaError> {
        Err(MediaError::Unsupported {
            platform: self.platform,
        })
    }

    fn adjust_volume(&self, _operation: &VolumeOperation) -> Result<(), MediaError> {
        Err(MediaError::Unsupported {
            platform: self.platform,
        })
    }
}

/// Deterministic in-memory fake recording every invocation in order, with
/// optional sticky failure injection for typed-error classification tests.
/// A test double proving the trait contract and driving CI determinism;
/// never a production fallback.
#[derive(Debug, Default)]
pub struct FakeMediaBackend {
    invocations: std::sync::Mutex<Vec<MediaInvocation>>,
    failure: std::sync::Mutex<Option<MediaError>>,
}

impl FakeMediaBackend {
    /// Fresh empty fake that reports success for every operation.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the sticky injected failure (`None` restores success). While
    /// set, every call fails with a copy of it and records nothing.
    pub fn set_failure(&self, failure: Option<MediaError>) {
        *self
            .failure
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = failure;
    }

    fn record(&self, invocation: MediaInvocation) -> Result<(), MediaError> {
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
    pub fn invocations(&self) -> Vec<MediaInvocation> {
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

impl MediaDeviceController for FakeMediaBackend {
    fn send_transport(&self, command: &MediaCommand) -> Result<(), MediaError> {
        self.record(MediaInvocation::Transport(*command))
    }

    fn adjust_volume(&self, operation: &VolumeOperation) -> Result<(), MediaError> {
        self.record(MediaInvocation::Volume(*operation))
    }
}

/// Returns the platform's media/volume backend.
///
/// - Windows: the real SendInput-class backend over the standard
///   media/volume keys.
/// - Everywhere else: the [`UnsupportedMediaBackend`] stub. Callers keep
///   one code path and receive typed denials at operation time —
///   capability reporting stays honest at the point of use.
#[must_use]
pub fn platform_media_backend() -> Box<dyn MediaDeviceController> {
    #[cfg(target_os = "windows")]
    {
        Box::new(WindowsMediaBackend::new())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Box::new(UnsupportedMediaBackend::new(current_platform()))
    }
}

#[cfg(test)]
mod tests {
    #[cfg(not(target_os = "windows"))]
    use super::platform_media_backend;
    use super::{
        FakeMediaBackend, MediaDeviceController, MediaError, MediaInvocation,
        UnsupportedMediaBackend, current_platform,
    };
    use crate::media::spec::{MAX_VOLUME_STEPS, MediaCommand, StepDirection, VolumeOperation};

    fn transport(command: MediaCommand) -> MediaInvocation {
        MediaInvocation::Transport(command)
    }

    #[test]
    fn fake_records_invocations_in_order_and_clears() {
        let fake = FakeMediaBackend::new();
        fake.send_transport(&MediaCommand::PlayPause)
            .expect("fake always accepts");
        fake.adjust_volume(&VolumeOperation::Up { steps: 3 })
            .expect("fake always accepts");
        fake.adjust_volume(&VolumeOperation::ToggleMute)
            .expect("fake always accepts");
        assert_eq!(fake.count(), 3);
        assert_eq!(
            fake.invocations(),
            vec![
                transport(MediaCommand::PlayPause),
                MediaInvocation::Volume(VolumeOperation::Up { steps: 3 }),
                MediaInvocation::Volume(VolumeOperation::ToggleMute),
            ]
        );
        fake.clear();
        assert_eq!(fake.count(), 0);
    }

    #[test]
    fn fake_failure_injection_is_sticky_and_blocks_recording() {
        let fake = FakeMediaBackend::new();
        fake.set_failure(Some(MediaError::PlatformFailure));
        assert_eq!(
            fake.send_transport(&MediaCommand::NextTrack),
            Err(MediaError::PlatformFailure)
        );
        assert_eq!(
            fake.adjust_volume(
                &VolumeOperation::new_step(StepDirection::Down, MAX_VOLUME_STEPS).expect("bounded")
            ),
            Err(MediaError::PlatformFailure)
        );
        assert_eq!(fake.count(), 0, "failed calls must not record");
        fake.set_failure(None);
        fake.send_transport(&MediaCommand::NextTrack).expect("fake");
        assert_eq!(fake.count(), 1);
    }

    #[test]
    fn unsupported_platform_fails_closed_with_typed_error() {
        let backend = UnsupportedMediaBackend::new(current_platform());
        assert_eq!(
            backend.send_transport(&MediaCommand::PlayPause),
            Err(MediaError::Unsupported {
                platform: current_platform()
            })
        );
        assert_eq!(
            backend.adjust_volume(&VolumeOperation::ToggleMute),
            Err(MediaError::Unsupported {
                platform: current_platform()
            })
        );
        assert_eq!(
            backend
                .send_transport(&MediaCommand::NextTrack)
                .unwrap_err()
                .to_string(),
            format!(
                "no media/volume backend for {} (fail closed)",
                current_platform()
            )
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn platform_selector_reports_unsupported_off_windows() {
        let error = platform_media_backend()
            .send_transport(&MediaCommand::PlayPause)
            .unwrap_err();
        assert!(matches!(error, MediaError::Unsupported { .. }));
    }
}
