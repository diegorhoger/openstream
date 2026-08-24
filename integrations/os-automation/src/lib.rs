//! `openstream-os-automation` — concrete OS automation adapters.
//!
//! Issue #10 ships the first one: the capability-scoped keyboard SHORTCUT
//! (synthesis/send) action behind engine action type
//! [`port::ACTION_TYPE_KEYBOARD_SHORTCUT`]. Issue #11 adds the launch
//! family ([`launch`]): application launch, file open, and URL open behind
//! action types [`launch::ACTION_TYPE_LAUNCH_APPLICATION`],
//! [`launch::ACTION_TYPE_LAUNCH_FILE`], and
//! [`launch::ACTION_TYPE_LAUNCH_URL`], all scoped by the existing taxonomy
//! row `os.application.launch:<identity>`. The adapters compose with the
//! merged engine contracts: registration declares its capability scopes
//! (taxonomy §5), idempotency class, and compensation posture; the runtime
//! enforces the grant intersection immediately before every dispatch, so
//! an ungranted request can never reach this crate.
//!
//! Keyboard platform support matrix (honest reporting, no silent fallback):
//!
//! | Platform | Backend | Behavior |
//! |---|---|---|
//! | Windows | [`backend::WindowsKeyboardBackend`] (SendInput-class synthesis via the pinned `enigo` wrapper) | Supported: typed shortcut synthesis |
//! | macOS / Linux (X11 or Wayland) | [`backend::UnsupportedKeyboardBackend`] | Every emit returns [`backend::KeyboardError::Unsupported`]; no fallback exists |
//!
//! Launch platform support matrix (same discipline):
//!
//! | Platform | Backend | Behavior |
//! |---|---|---|
//! | Windows | [`launch::WindowsLaunchBackend`] (CreateProcess-class direct spawns; ShellExecuteW-class default-handler opens via the pinned `open` wrapper) | Supported: validated application/file/URL targets launch |
//! | macOS / Linux | [`launch::UnsupportedLaunchBackend`] | Every operation returns [`launch::LaunchError::Unsupported`]; no fallback exists |
//!
//! Wayland limitation (explicit): global synthetic input has no stable,
//! compositor-independent protocol under Wayland's security model; shipping
//! a per-compositor workaround would widen OS permissions beyond declared
//! behavior. Linux therefore reports `Unsupported` regardless of session
//! type until a reviewed platform milestone ships a backend. The same
//! honest-reporting rule applies to the launch adapters: without a reviewed
//! backend there is no `xdg-open`-style fallback anywhere.
//!
//! Window-scoped delivery is NOT implemented this milestone. The keyboard
//! adapter registers exactly the unqualified `os.keyboard.emit` scope, so an
//! `app=<identity>`-qualified node rejects at the engine's manifest
//! intersection (`NotRequestedByManifest`) before anything dispatches; the
//! port additionally refuses such requests with the typed failure code
//! [`port::CODE_WINDOW_SCOPE_UNSUPPORTED`] as defense in depth. Silently
//! delivering to the foreground window under a scoped grant would mismatch
//! granted authority.
//!
//! Hard rules: the keyboard adapter only ever *sends* synthetic events —
//! no capture, hooking, polling, or logging of user input exists anywhere,
//! and none may be added without a security ADR and human gate. The launch
//! adapters only ever *launch* approved targets — no shell interpreter, no
//! argument interpolation (zero arguments this milestone), no inherited
//! environment/CWD/standard handles on direct spawns, executable identity
//! revalidated before every spawn, and URL execution confined to the
//! policy allowlist over a closed scheme vocabulary.

pub mod backend;
pub mod launch;
pub mod port;
pub mod spec;

#[doc(inline)]
pub use crate::{
    backend::{
        FakeKeyboardBackend, KeyboardError, KeyboardSynthesizer, UnsupportedKeyboardBackend,
        current_platform, platform_keyboard_backend,
    },
    launch::{
        ACTION_TYPE_LAUNCH_APPLICATION, ACTION_TYPE_LAUNCH_FILE, ACTION_TYPE_LAUNCH_URL,
        ApplicationTarget, FakeLaunchBackend, FileTarget, LaunchBackend, LaunchBinding,
        LaunchConfigError, LaunchError, LaunchInvocation, LaunchKind, LaunchPolicy, LaunchPort,
        LaunchRegistrationError, MAX_IDENTITY_TOKEN_BYTES, MAX_TARGET_BYTES,
        UnsupportedLaunchBackend, UrlScheme, UrlTarget, parse_application_params,
        parse_file_params, parse_url_params, platform_launch_backend, register_launch_actions,
    },
    port::{
        ACTION_TYPE_KEYBOARD_SHORTCUT, CODE_CAPABILITY_MISMATCH, CODE_INVALID_CONFIG,
        CODE_PLATFORM_REFUSED, CODE_UNSUPPORTED_PLATFORM, CODE_WINDOW_SCOPE_UNSUPPORTED,
        KeyboardShortcutPort, register_keyboard_shortcut_action,
    },
    spec::{Chord, KeyValue, Modifier, ShortcutConfigError, ShortcutSpec, parse_shortcut_params},
};

/// Launch failure codes that do not collide with the keyboard adapter's
/// root exports (`CODE_MISSING_TARGET`, `CODE_POLICY_REFUSED`). Codes whose
/// values are shared across adapters remain reachable through
/// [`launch`] / [`port`] module paths to keep one unambiguous name per root
/// export.
#[doc(inline)]
pub use crate::launch::{CODE_MISSING_TARGET, CODE_POLICY_REFUSED};

/// Real Windows synthesis backend (SendInput-class via `enigo`). Present
/// only on Windows; every other platform reports
/// [`UnsupportedKeyboardBackend`] through
/// [`platform_keyboard_backend`].
#[cfg(target_os = "windows")]
#[doc(inline)]
pub use crate::backend::WindowsKeyboardBackend;

/// Real Windows launch backend (CreateProcess-class direct spawns plus
/// ShellExecuteW-class default-handler opens via the pinned `open`
/// wrapper). Present only on Windows; every other platform reports
/// [`UnsupportedLaunchBackend`] through [`platform_launch_backend`].
#[cfg(target_os = "windows")]
#[doc(inline)]
pub use crate::launch::WindowsLaunchBackend;
