//! Data-directory resolution for the desktop shell (issue #16).
//!
//! The resolved directory hosts exactly two documented artifacts — the
//! execution-journal store (`recovery::JOURNAL_FILE_NAME`) and the
//! single-instance lock file (`single_instance::LOCK_FILE_NAME`); see
//! `docs/architecture/DESKTOP_LIFECYCLE.md`. Nothing else is ever written
//! there by this crate: no telemetry, no caches, no hidden state.
//!
//! Resolution is a pure function of an injected environment reader over an
//! explicit [`DesktopPlatform`], so every platform branch is testable on any
/// host. The production entry point selects the platform from
/// `std::env::consts::OS`.
use std::path::PathBuf;

/// Application directory name under the per-OS base directory.
pub const APP_DIR_NAME: &str = "OpenStream";

/// Environment reader abstraction (production passes `std::env::var`).
pub type EnvReader<'a> = dyn Fn(&str) -> Option<String> + 'a;

/// Desktop platforms with distinct data-directory conventions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopPlatform {
    /// Windows: `%APPDATA%\<app>`.
    Windows,
    /// macOS: `~/Library/Application Support/<app>`.
    MacOS,
    /// Linux and other Unix-like systems: `$XDG_DATA_HOME/<app>` or
    /// `~/.local/share/<app>` (XDG base-directory specification).
    LinuxLike,
}

impl DesktopPlatform {
    /// The platform of the running host.
    #[must_use]
    pub fn current() -> Self {
        match std::env::consts::OS {
            "windows" => Self::Windows,
            "macos" => Self::MacOS,
            _ => Self::LinuxLike,
        }
    }
}

fn non_empty(value: Option<String>) -> Option<PathBuf> {
    value
        .filter(|candidate| !candidate.trim().is_empty())
        .map(PathBuf::from)
}

fn home_dir(env: &EnvReader) -> Option<PathBuf> {
    non_empty(env("HOME"))
}

/// Resolves the data directory for an explicit platform.
///
/// Returns `None` when the platform's base directory cannot be determined;
/// the shell then starts in a degraded, persistence-free mode rather than
/// guessing a location.
#[must_use]
pub fn resolve_data_dir_for(platform: DesktopPlatform, env: &EnvReader) -> Option<PathBuf> {
    match platform {
        DesktopPlatform::Windows => non_empty(env("APPDATA")).map(|base| base.join(APP_DIR_NAME)),
        DesktopPlatform::MacOS => home_dir(env).map(|home| {
            home.join("Library")
                .join("Application Support")
                .join(APP_DIR_NAME)
        }),
        DesktopPlatform::LinuxLike => {
            if let Some(xdg) = env("XDG_DATA_HOME")
                .filter(|value| !value.trim().is_empty())
                .and_then(|value| {
                    // The XDG spec requires an absolute path; relative values
                    // are ignored rather than misinterpreted. `has_root`
                    // keeps this pure function's semantics identical across
                    // host platforms (a rooted POSIX path stays recognized
                    // even when the test host is Windows).
                    let path = PathBuf::from(&value);
                    path.has_root().then_some(path)
                })
            {
                return Some(xdg.join(APP_DIR_NAME));
            }
            home_dir(env).map(|home| home.join(".local").join("share").join(APP_DIR_NAME))
        }
    }
}

/// Resolves the data directory for the running host.
#[must_use]
pub fn resolve_data_dir(env: &EnvReader) -> Option<PathBuf> {
    resolve_data_dir_for(DesktopPlatform::current(), env)
}

#[cfg(test)]
mod tests {
    use super::{DesktopPlatform, resolve_data_dir_for};
    use std::collections::HashMap;

    fn env_from<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        let map: HashMap<&str, String> =
            pairs.iter().map(|(k, v)| (*k, (*v).to_string())).collect();
        move |key: &str| map.get(key).cloned()
    }

    #[test]
    fn windows_uses_appdata() {
        let env = env_from(&[("APPDATA", r"C:\Users\u\AppData\Roaming")]);
        assert_eq!(
            resolve_data_dir_for(DesktopPlatform::Windows, &env),
            Some(r"C:\Users\u\AppData\Roaming\OpenStream".into())
        );
    }

    #[test]
    fn windows_requires_appdata() {
        let env = env_from(&[]);
        assert_eq!(resolve_data_dir_for(DesktopPlatform::Windows, &env), None);
    }

    #[test]
    fn macos_places_under_library_application_support() {
        let env = env_from(&[("HOME", "/Users/u")]);
        assert_eq!(
            resolve_data_dir_for(DesktopPlatform::MacOS, &env),
            Some("/Users/u/Library/Application Support/OpenStream".into())
        );
    }

    #[test]
    fn linux_prefers_absolute_xdg_data_home() {
        let env = env_from(&[("XDG_DATA_HOME", "/xdg"), ("HOME", "/Users/u")]);
        assert_eq!(
            resolve_data_dir_for(DesktopPlatform::LinuxLike, &env),
            Some("/xdg/OpenStream".into())
        );
    }

    #[test]
    fn linux_falls_back_to_local_share_when_xdg_is_relative_or_missing() {
        let relative = env_from(&[("XDG_DATA_HOME", "relative/path"), ("HOME", "/Users/u")]);
        assert_eq!(
            resolve_data_dir_for(DesktopPlatform::LinuxLike, &relative),
            Some("/Users/u/.local/share/OpenStream".into())
        );

        let missing = env_from(&[("HOME", "/Users/u")]);
        assert_eq!(
            resolve_data_dir_for(DesktopPlatform::LinuxLike, &missing),
            Some("/Users/u/.local/share/OpenStream".into())
        );
    }

    #[test]
    fn empty_environment_values_are_rejected_everywhere() {
        let blank_windows = env_from(&[("APPDATA", "   ")]);
        assert_eq!(
            resolve_data_dir_for(DesktopPlatform::Windows, &blank_windows),
            None
        );

        let blank_home = env_from(&[("HOME", ""), ("XDG_DATA_HOME", "")]);
        assert_eq!(
            resolve_data_dir_for(DesktopPlatform::LinuxLike, &blank_home),
            None
        );
    }
}
