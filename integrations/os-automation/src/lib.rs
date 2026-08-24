//! `openstream-os-automation` — concrete OS automation adapters.
//!
//! Issue #10 ships the first one: the capability-scoped keyboard SHORTCUT
//! (synthesis/send) action behind engine action type
//! [`port::ACTION_TYPE_KEYBOARD_SHORTCUT`]. The adapter composes with the
//! merged engine contracts: registration declares its capability scopes
//! (`os.keyboard.emit`, taxonomy §5), idempotency class, and compensation
//! posture; the runtime enforces the grant intersection immediately before
//! every dispatch, so an ungranted request can never reach this crate.
//!
//! Platform support matrix (honest reporting, no silent fallback):
//!
//! | Platform | Backend | Behavior |
//! |---|---|---|
//! | Windows | [`backend::WindowsKeyboardBackend`] (SendInput-class synthesis via the pinned `enigo` wrapper) | Supported: typed shortcut synthesis |
//! | macOS / Linux (X11 or Wayland) | [`backend::UnsupportedKeyboardBackend`] | Every emit returns [`backend::KeyboardError::Unsupported`]; no fallback exists |
//!
//! Wayland limitation (explicit): global synthetic input has no stable,
//! compositor-independent protocol under Wayland's security model; shipping
//! a per-compositor workaround would widen OS permissions beyond declared
//! behavior. Linux therefore reports `Unsupported` regardless of session
//! type until a reviewed platform milestone ships a backend.
//!
//! Window-scoped delivery is NOT implemented this milestone. The adapter
//! registers exactly the unqualified `os.keyboard.emit` scope, so an
//! `app=<identity>`-qualified node rejects at the engine's manifest
//! intersection (`NotRequestedByManifest`) before anything dispatches; the
//! port additionally refuses such requests with the typed failure code
//! [`port::CODE_WINDOW_SCOPE_UNSUPPORTED`] as defense in depth. Silently
//! delivering to the foreground window under a scoped grant would mismatch
//! granted authority.
//!
//! Hard rule: this adapter only ever *sends* synthetic events. There is no
//! capture, hooking, polling, or logging of user input anywhere in this
//! crate, and none may be added without a security ADR and human gate.

pub mod backend;
pub mod port;
pub mod spec;

#[doc(inline)]
pub use crate::{
    backend::{
        FakeKeyboardBackend, KeyboardError, KeyboardSynthesizer, UnsupportedKeyboardBackend,
        current_platform, platform_keyboard_backend,
    },
    port::{
        ACTION_TYPE_KEYBOARD_SHORTCUT, CODE_CAPABILITY_MISMATCH, CODE_INVALID_CONFIG,
        CODE_PLATFORM_REFUSED, CODE_UNSUPPORTED_PLATFORM, CODE_WINDOW_SCOPE_UNSUPPORTED,
        KeyboardShortcutPort, register_keyboard_shortcut_action,
    },
    spec::{Chord, KeyValue, Modifier, ShortcutConfigError, ShortcutSpec, parse_shortcut_params},
};

/// Real Windows synthesis backend (SendInput-class via `enigo`). Present
/// only on Windows; every other platform reports
/// [`UnsupportedKeyboardBackend`] through
/// [`platform_keyboard_backend`].
#[cfg(target_os = "windows")]
#[doc(inline)]
pub use crate::backend::WindowsKeyboardBackend;
