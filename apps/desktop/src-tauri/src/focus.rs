//! Focused-app identity source (issue #19).
//!
//! Reads ONLY which application identity (lowercased process image file
//! name, e.g. `obs64.exe`) currently holds keyboard focus. Window titles
//! and content are never read, never stored, never logged â€” the identity
//! token is the entire observation surface.
//!
//! This is not input capture: no hooks, no keystrokes, no message streams.
//! It observes window-focus state, equivalent to what task bars show.
//!
//! Platform matrix (honest):
//!
//! - **Windows â€” shipped:** foreground-window process path via the pinned
//!   `active-win-pos-rs` wrapper.
//! - **macOS / Linux â€” honest `Unsupported` for this milestone:** callers
//!   must render the typed degraded state instead of silently disabling.
//!
//! Every failure is typed and closed-vocabulary.

use std::fmt;

use openstream_domain::switching::AppIdentity;

// Only the shipped Windows backend parses identity tokens from raw file
// names; other platforms never reach a parse site.
#[cfg(target_os = "windows")]
use std::str::FromStr as _;

/// Typed focused-app failures; never carries OS text, paths, or titles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusError {
    /// This platform has no shipped focus source in this build.
    /// Constructed only by [`UnsupportedFocusSource`], which itself exists
    /// only for platforms without a real backend — hence the targeted
    /// allowance for platform-dependent reachability.
    #[allow(dead_code)]
    Unsupported {
        /// Platform label from `std::env::consts::OS` (echo-safe).
        os: &'static str,
    },
    /// The concrete backend could not observe focus right now (e.g. a
    /// secure desktop or an access refusal). Surfaced as a transient typed
    /// failure; never guessed into a stable identity. Constructed only by
    /// backends with real observation side effects (Windows source) —
    /// hence the targeted allowance for platform-dependent reachability.
    #[allow(dead_code)]
    Refused,
}

impl fmt::Display for FocusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { os } => write!(f, "unsupported-on-{os}"),
            Self::Refused => f.write_str("focus-refused"),
        }
    }
}

/// Platform boundary for observing focused-application identity.
///
/// `Send + Sync` lets the composition root hold the source inside shared
/// shell state behind a mutex.
pub trait FocusSource: fmt::Debug + Send + Sync {
    /// True when this build ships a real focus backend on this platform.
    fn supported(&self) -> bool;

    /// The focused application identity right now.
    ///
    /// # Errors
    /// Typed failures only. `Ok(None)` means a healthy "no foreground
    /// application" observation (e.g. the desktop itself has focus);
    /// refusals are errors, never silent `None`s.
    fn focused_app(&mut self) -> Result<Option<AppIdentity>, FocusError>;
}

/// Windows backend: derives the identity from the foreground window's
/// process image file name via the pinned wrapper. Only the file-name
/// portion is ever inspected; full paths and titles are dropped here.
#[cfg(target_os = "windows")]
#[derive(Debug)]
pub struct WindowsFocusSource;

#[cfg(target_os = "windows")]
impl WindowsFocusSource {
    /// Extracts the validated identity from one observation. Kept pure so
    /// tests can drive it without any live window.
    fn identity_from(process_path: &std::path::Path) -> Result<Option<AppIdentity>, FocusError> {
        let Some(file_name) = process_path.file_name() else {
            // System processes can expose pathless entries; that is an
            // observation without an identity, not a failure.
            return Ok(None);
        };
        let raw = file_name.to_string_lossy().to_ascii_lowercase();
        AppIdentity::from_str(&raw)
            .map(Some)
            .map_err(|_| FocusError::Refused)
    }
}

#[cfg(target_os = "windows")]
impl FocusSource for WindowsFocusSource {
    fn supported(&self) -> bool {
        true
    }

    fn focused_app(&mut self) -> Result<Option<AppIdentity>, FocusError> {
        match active_win_pos_rs::get_active_window() {
            Ok(window) => Self::identity_from(&window.process_path),
            Err(_) => Err(FocusError::Refused),
        }
    }
}

/// Backend for platforms without a shipped focus mechanism this milestone.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub struct UnsupportedFocusSource {
    os: &'static str,
}

#[allow(dead_code)]
impl UnsupportedFocusSource {
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

impl FocusSource for UnsupportedFocusSource {
    fn supported(&self) -> bool {
        false
    }

    fn focused_app(&mut self) -> Result<Option<AppIdentity>, FocusError> {
        Err(FocusError::Unsupported { os: self.os })
    }
}

/// Production selector for the composition root: real Windows source,
/// honest `Unsupported` elsewhere.
#[must_use]
pub fn platform_default_focus_source() -> Box<dyn FocusSource> {
    #[cfg(target_os = "windows")]
    {
        Box::new(WindowsFocusSource)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Box::new(UnsupportedFocusSource::for_current_os())
    }
}

#[cfg(test)]
mod tests {
    use super::{FocusError, FocusSource, UnsupportedFocusSource};

    #[test]
    fn unsupported_backend_reports_honestly() {
        let mut source = UnsupportedFocusSource::for_current_os();
        assert!(!source.supported());
        assert_eq!(
            source.focused_app().unwrap_err(),
            FocusError::Unsupported {
                os: std::env::consts::OS
            }
        );
    }

    #[test]
    fn production_selector_agrees_with_platform_support() {
        let source = super::platform_default_focus_source();
        assert_eq!(
            source.supported(),
            cfg!(target_os = "windows"),
            "support claims must match the shipped platform matrix"
        );
    }

    #[cfg(target_os = "windows")]
    mod windows_identity_tests {
        use super::super::WindowsFocusSource;
        use openstream_domain::switching::AppIdentity;
        use std::path::Path;
        use std::str::FromStr as _;

        #[test]
        fn identity_is_the_lowercased_image_file_name_only() {
            let observed = WindowsFocusSource::identity_from(Path::new(
                r"C:\Program Files\obs-studio\bin\64bit\OBS64.EXE",
            ))
            .expect("parses");
            assert_eq!(
                observed,
                Some(AppIdentity::from_str("obs64.exe").unwrap()),
                "case normalizes to the canonical lowercase grammar"
            );
            // Pathless observations are healthy None, never errors.
            assert_eq!(
                WindowsFocusSource::identity_from(Path::new("")).unwrap(),
                None
            );
            // Grammar-violating names refuse instead of passing through.
            assert!(WindowsFocusSource::identity_from(Path::new("bad name.exe")).is_err());
        }

        #[test]
        fn real_backend_reports_supported_and_answers_typed() {
            use super::super::{FocusSource as _, WindowsFocusSource};
            // Live smoke test on the real OS: whatever the desktop shows,
            // the answer must be either a valid identity, a healthy None,
            // or the typed refusal â€” never a panic or guessed default.
            let mut source = WindowsFocusSource;
            assert!(source.supported());
            match source.focused_app() {
                Ok(Some(identity)) => {
                    assert!(!identity.as_str().is_empty());
                    assert!(identity.as_str().chars().all(|c| c.is_ascii_lowercase()
                        || c.is_ascii_digit()
                        || matches!(c, '.' | '-' | '_')));
                }
                Ok(None) | Err(super::FocusError::Refused) => {}
                other => panic!("unexpected typed outcome: {other:?}"),
            }
        }
    }
}
