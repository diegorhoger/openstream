//! Keyboard synthesis backends behind one object-safe boundary.
//!
//! Mirrors the OS credential-vault discipline (`openstream-persistence`):
//!
//! - [`KeyboardSynthesizer`] is the object-safe port every backend
//!   implements. All FFI lives inside the pinned audited `enigo`
//!   dependency (Windows build only); this crate keeps the workspace-wide
//!   `unsafe_code = "forbid"`.
//! - On Windows the real backend is [`WindowsKeyboardBackend`]:
//!   SendInput-class synthesis of validated chord sequences.
//! - Every other platform gets [`UnsupportedKeyboardBackend`]: an explicit,
//!   documented stub whose operations return
//!   [`KeyboardError::Unsupported`]. No X11 hook, no Wayland workaround,
//!   no best-effort downgrade anywhere — honest capability reporting per
//!   repository norms.
//! - [`FakeKeyboardBackend`] records emitted specs in memory for
//!   deterministic tests and CI. It is a test double only and is never a
//!   production fallback.
//!
//! Error values carry structural data only (operation class, platform tag).
//! No OS error text ever enters an error channel.

use crate::spec::ShortcutSpec;
#[cfg(target_os = "windows")]
use crate::spec::{KeyValue, Modifier};
use core::fmt;

/// Static platform tag used in `Unsupported` reports.
pub fn current_platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unsupported-platform"
    }
}

/// Typed synthesis failures. Variants carry no OS message text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyboardError {
    /// No synthesis backend exists for this platform this milestone.
    /// OpenStream never falls back to a weaker mechanism; the capability
    /// gap is reported honestly instead.
    Unsupported {
        /// Platform tag (`macos`, `linux`, ...).
        platform: &'static str,
    },
    /// The OS refused the synthesis operation (permission window closed,
    /// desktop session unavailable class). The OS error text is dropped;
    /// only the class survives.
    PlatformFailure,
}

impl fmt::Display for KeyboardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { platform } => {
                write!(
                    f,
                    "no keyboard synthesis backend for {platform} (fail closed)"
                )
            }
            Self::PlatformFailure => f.write_str("platform refused keyboard synthesis"),
        }
    }
}

impl std::error::Error for KeyboardError {}

/// Object-safe boundary over synthetic key emission. Implementations must
/// be usable from the engine runtime (`Send + Sync`) and must only ever
/// *send* events: capture, listening, or logging of user input is outside
/// this contract forever.
pub trait KeyboardSynthesizer: fmt::Debug + Send + Sync {
    /// Synthesizes one validated shortcut sequence. Chords apply in order:
    /// modifiers press (canonical ctrl→alt→shift→meta), the main key
    /// clicks, then modifiers release in reverse.
    ///
    /// # Errors
    /// [`KeyboardError::Unsupported`] without a platform backend;
    /// [`KeyboardError::PlatformFailure`] when the OS refuses the effect.
    fn emit(&self, shortcut: &ShortcutSpec) -> Result<(), KeyboardError>;
}

/// Real Windows backend: SendInput-class synthesis through the pinned
/// `enigo` wrapper (`default-features = false`; no other platform's stack
/// is compiled). Holds no state: one synthesizer instance is created per
/// emit so nothing outlives the call.
#[cfg(target_os = "windows")]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct WindowsKeyboardBackend;

#[cfg(target_os = "windows")]
impl WindowsKeyboardBackend {
    /// Creates the backend handle. Construction cannot fail; failures
    /// surface per-operation as typed [`KeyboardError`] values.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    fn map_modifier(modifier: Modifier) -> enigo::Key {
        match modifier {
            Modifier::Ctrl => enigo::Key::Control,
            Modifier::Alt => enigo::Key::Alt,
            Modifier::Shift => enigo::Key::Shift,
            Modifier::Meta => enigo::Key::Meta,
        }
    }

    fn map_key(key: KeyValue) -> enigo::Key {
        match key {
            KeyValue::Char(c) => enigo::Key::Unicode(c),
            KeyValue::Function(n) => match n {
                1 => enigo::Key::F1,
                2 => enigo::Key::F2,
                3 => enigo::Key::F3,
                4 => enigo::Key::F4,
                5 => enigo::Key::F5,
                6 => enigo::Key::F6,
                7 => enigo::Key::F7,
                8 => enigo::Key::F8,
                9 => enigo::Key::F9,
                10 => enigo::Key::F10,
                11 => enigo::Key::F11,
                12 => enigo::Key::F12,
                13 => enigo::Key::F13,
                14 => enigo::Key::F14,
                15 => enigo::Key::F15,
                16 => enigo::Key::F16,
                17 => enigo::Key::F17,
                18 => enigo::Key::F18,
                19 => enigo::Key::F19,
                20 => enigo::Key::F20,
                21 => enigo::Key::F21,
                22 => enigo::Key::F22,
                23 => enigo::Key::F23,
                _ => enigo::Key::F24,
            },
            KeyValue::Space => enigo::Key::Space,
            KeyValue::Enter => enigo::Key::Return,
            KeyValue::Tab => enigo::Key::Tab,
            KeyValue::Escape => enigo::Key::Escape,
            KeyValue::Backspace => enigo::Key::Backspace,
            KeyValue::Delete => enigo::Key::Delete,
            KeyValue::Insert => enigo::Key::Insert,
            KeyValue::Home => enigo::Key::Home,
            KeyValue::End => enigo::Key::End,
            KeyValue::PageUp => enigo::Key::PageUp,
            KeyValue::PageDown => enigo::Key::PageDown,
            KeyValue::Left => enigo::Key::LeftArrow,
            KeyValue::Right => enigo::Key::RightArrow,
            KeyValue::Up => enigo::Key::UpArrow,
            KeyValue::Down => enigo::Key::DownArrow,
        }
    }
}

#[cfg(target_os = "windows")]
impl KeyboardSynthesizer for WindowsKeyboardBackend {
    fn emit(&self, shortcut: &ShortcutSpec) -> Result<(), KeyboardError> {
        use enigo::{Direction, Enigo, Keyboard as _, Settings};

        let mut enigo =
            Enigo::new(&Settings::default()).map_err(|_| KeyboardError::PlatformFailure)?;
        for chord in shortcut.chords() {
            for modifier in chord.modifiers() {
                enigo
                    .key(Self::map_modifier(*modifier), Direction::Press)
                    .map_err(|_| KeyboardError::PlatformFailure)?;
            }
            enigo
                .key(Self::map_key(chord.key()), Direction::Click)
                .map_err(|_| KeyboardError::PlatformFailure)?;
            for modifier in chord.modifiers().iter().rev() {
                enigo
                    .key(Self::map_modifier(*modifier), Direction::Release)
                    .map_err(|_| KeyboardError::PlatformFailure)?;
            }
        }
        Ok(())
    }
}

/// Honest capability report for platforms without a shipped backend.
/// Operations fail closed with [`KeyboardError::Unsupported`]; this type
/// deliberately synthesizes nothing and hooks nothing. Compiled on every
/// platform (unlike the credential-vault stub) so the fail-closed contract
/// is testable everywhere; production composition roots only select it via
/// [`platform_keyboard_backend`] off Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UnsupportedKeyboardBackend {
    platform: &'static str,
}

impl UnsupportedKeyboardBackend {
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

impl KeyboardSynthesizer for UnsupportedKeyboardBackend {
    fn emit(&self, _shortcut: &ShortcutSpec) -> Result<(), KeyboardError> {
        Err(KeyboardError::Unsupported {
            platform: self.platform,
        })
    }
}

/// Deterministic in-memory fake recording every emitted spec in order.
/// A test double proving the trait contract and driving CI determinism;
/// never a production fallback.
#[derive(Debug, Default)]
pub struct FakeKeyboardBackend {
    emissions: std::sync::Mutex<Vec<ShortcutSpec>>,
}

impl FakeKeyboardBackend {
    /// Fresh empty fake.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshots the recorded emissions in emission order.
    #[must_use]
    pub fn emissions(&self) -> Vec<ShortcutSpec> {
        self.emissions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Number of recorded emissions.
    #[must_use]
    pub fn count(&self) -> usize {
        self.emissions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    /// Clears all recorded emissions.
    pub fn clear(&self) {
        self.emissions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }
}

impl KeyboardSynthesizer for FakeKeyboardBackend {
    fn emit(&self, shortcut: &ShortcutSpec) -> Result<(), KeyboardError> {
        self.emissions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(shortcut.clone());
        Ok(())
    }
}

/// Returns the platform's keyboard-synthesis backend.
///
/// - Windows: the real SendInput-class backend.
/// - Everywhere else: the [`UnsupportedKeyboardBackend`] stub. Callers keep
///   one code path and receive typed denials at operation time —
///   capability reporting stays honest at the point of use.
#[must_use]
pub fn platform_keyboard_backend() -> Box<dyn KeyboardSynthesizer> {
    #[cfg(target_os = "windows")]
    {
        Box::new(WindowsKeyboardBackend::new())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Box::new(UnsupportedKeyboardBackend::new(current_platform()))
    }
}

#[cfg(test)]
mod tests {
    use super::{FakeKeyboardBackend, KeyboardError, KeyboardSynthesizer, current_platform};
    use crate::spec::{ShortcutSpec, parse_shortcut_params};
    use serde_json::json;

    fn spec(raw: &str) -> ShortcutSpec {
        parse_shortcut_params(&json!({ "keys": raw })).expect("fixture spec must validate")
    }

    #[test]
    fn fake_records_emissions_in_order_and_clears() {
        let fake = FakeKeyboardBackend::new();
        fake.emit(&spec("ctrl+c")).expect("fake always accepts");
        fake.emit(&spec("ctrl+shift+t"))
            .expect("fake always accepts");
        assert_eq!(fake.count(), 2);
        let recorded = fake.emissions();
        assert_eq!(recorded[0], spec("ctrl+c"));
        assert_eq!(recorded[1], spec("ctrl+shift+t"));
        fake.clear();
        assert_eq!(fake.count(), 0);
    }

    #[test]
    fn unsupported_platform_fails_closed_with_typed_error() {
        use super::UnsupportedKeyboardBackend;

        let backend = UnsupportedKeyboardBackend::new(current_platform());
        let error = backend.emit(&spec("ctrl+a")).unwrap_err();
        assert_eq!(
            error,
            KeyboardError::Unsupported {
                platform: current_platform()
            }
        );
        assert_eq!(
            error.to_string(),
            format!(
                "no keyboard synthesis backend for {} (fail closed)",
                current_platform()
            )
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn platform_selector_reports_unsupported_off_windows() {
        use super::platform_keyboard_backend;

        let error = platform_keyboard_backend()
            .emit(&spec("ctrl+a"))
            .unwrap_err();
        assert!(matches!(error, KeyboardError::Unsupported { .. }));
    }
}
