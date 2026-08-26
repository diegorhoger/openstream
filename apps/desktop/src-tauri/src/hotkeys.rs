//! Global-shortcut REGISTRATION backend (issue #19).
//!
//! Authority boundary (hard stop of issue #19): this module only ever
//! REGISTERS keyboard shortcuts with the operating system â€” on Windows via
//! `RegisterHotKey`-class registration inside the pinned `global-hotkey`
//! wrapper. The OS delivers the registered combinations to us; no
//! keyboard hooks, input listeners, or keystroke reading exist anywhere in
//! OpenStream.
//!
//! Platform matrix (honest):
//!
//! - **Windows â€” shipped:** `global-hotkey` (`RegisterHotKey` semantics).
//! - **macOS / Linux â€” honest `Unsupported` for this milestone:** no
//!   registrations are made; surfaces report unavailability instead of
//!   pretending (same posture as autostart in issue #16).
//!
//! Every failure is typed and closed-vocabulary: no OS message text is ever
//! surfaced.

use std::fmt;

use openstream_domain::switching::HotkeyCombo;

/// Which registration operation failed; closed vocabulary for surfacing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyOperation {
    /// Registering a combination with the OS.
    Register,
    /// Removing an existing registration.
    Unregister,
}

impl fmt::Display for HotkeyOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Register => f.write_str("register"),
            Self::Unregister => f.write_str("unregister"),
        }
    }
}

/// Typed hotkey-registration failures; never carries OS message text or
/// window titles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyError {
    /// This platform has no shipped registration mechanism in this build.
    /// Constructed only by [`UnsupportedRegistrar`] (and mirrored by the
    /// worker refusal path) — hence the targeted allowance for
    /// platform-dependent reachability.
    #[allow(dead_code)]
    Unsupported {
        /// Platform label from `std::env::consts::OS` (echo-safe).
        os: &'static str,
    },
    /// The OS refused one registration/unregistration operation.
    Refused {
        /// Which operation was refused.
        operation: HotkeyOperation,
    },
    /// The combination is already registered â€” by another application or,
    /// defensively, by ourselves (the engine reconciles desired/applied sets
    /// and never double-registers; seeing this means state drifted).
    Conflict {
        /// Canonical form of the contested combination (structural data,
        /// not personal).
        combo: String,
    },
}

impl fmt::Display for HotkeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { os } => write!(f, "unsupported-on-{os}"),
            Self::Refused { operation } => write!(f, "{operation}-refused"),
            Self::Conflict { combo } => write!(f, "conflict:{combo}"),
        }
    }
}

/// Platform boundary for registering global shortcuts. Registration-based
/// delivery ONLY: implementations must never install hooks or read input.
///
/// `Send + Sync` lets the composition root hold the backend inside shared
/// shell state behind a mutex.
pub trait HotkeyRegistrar: fmt::Debug + Send + Sync {
    /// True when this build ships a real registration backend on the
    /// running platform.
    fn supported(&self) -> bool;

    /// Registers one combination with the OS.
    ///
    /// # Errors
    /// Typed failures only ([`HotkeyError`]); nothing is half-registered.
    fn register(&mut self, combo: &HotkeyCombo) -> Result<(), HotkeyError>;

    /// Removes one registration. Unregistering something that is not
    /// registered stays idempotent success.
    ///
    /// # Errors
    /// Typed failures only.
    fn unregister(&mut self, combo: &HotkeyCombo) -> Result<(), HotkeyError>;

    /// Drains queued OS press deliveries into the canonical combinations
    /// this backend registered. Default: no registrations exist, so no
    /// events ever arrive (unsupported platforms and test doubles).
    /// Registration-based delivery ONLY — implementations must never
    /// observe anything beyond their own registered shortcuts.
    fn drain_pressed(&mut self) -> Vec<HotkeyCombo> {
        Vec::new()
    }
}

/// Backend for platforms without a shipped registration mechanism this
/// milestone: every operation reports [`HotkeyError::Unsupported`] so user
/// surfaces render honest "unavailable" states instead of faking.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub struct UnsupportedRegistrar {
    os: &'static str,
}

#[allow(dead_code)]
impl UnsupportedRegistrar {
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

impl HotkeyRegistrar for UnsupportedRegistrar {
    fn supported(&self) -> bool {
        false
    }

    fn register(&mut self, _combo: &HotkeyCombo) -> Result<(), HotkeyError> {
        Err(HotkeyError::Unsupported { os: self.os })
    }

    fn unregister(&mut self, _combo: &HotkeyCombo) -> Result<(), HotkeyError> {
        Err(HotkeyError::Unsupported { os: self.os })
    }
}

/// Production selector for the composition root: real Windows backend,
/// honest `Unsupported` elsewhere.
#[must_use]
pub fn platform_default_registrar() -> Box<dyn HotkeyRegistrar> {
    #[cfg(target_os = "windows")]
    {
        Box::new(WindowsHotkeyRegistrar)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Box::new(UnsupportedRegistrar::for_current_os())
    }
}

/// Maps the closed domain vocabulary onto the platform's key codes. Kept
/// exhaustive over [`HotkeyKey`] so adding a key token forces this mapping
/// to be revisited.
#[cfg(target_os = "windows")]
fn code_of(key: openstream_domain::switching::HotkeyKey) -> global_hotkey::hotkey::Code {
    use global_hotkey::hotkey::Code as C;
    use openstream_domain::switching::HotkeyKey as K;
    match key {
        K::Letter(b) => match b {
            b'a' => C::KeyA,
            b'b' => C::KeyB,
            b'c' => C::KeyC,
            b'd' => C::KeyD,
            b'e' => C::KeyE,
            b'f' => C::KeyF,
            b'g' => C::KeyG,
            b'h' => C::KeyH,
            b'i' => C::KeyI,
            b'j' => C::KeyJ,
            b'k' => C::KeyK,
            b'l' => C::KeyL,
            b'm' => C::KeyM,
            b'n' => C::KeyN,
            b'o' => C::KeyO,
            b'p' => C::KeyP,
            b'q' => C::KeyQ,
            b'r' => C::KeyR,
            b's' => C::KeyS,
            b't' => C::KeyT,
            b'u' => C::KeyU,
            b'v' => C::KeyV,
            b'w' => C::KeyW,
            b'x' => C::KeyX,
            b'y' => C::KeyY,
            _ => C::KeyZ,
        },
        K::Digit(d) => match d {
            b'0' => C::Digit0,
            b'1' => C::Digit1,
            b'2' => C::Digit2,
            b'3' => C::Digit3,
            b'4' => C::Digit4,
            b'5' => C::Digit5,
            b'6' => C::Digit6,
            b'7' => C::Digit7,
            b'8' => C::Digit8,
            _ => C::Digit9,
        },
        K::Function(n) => match n {
            1 => C::F1,
            2 => C::F2,
            3 => C::F3,
            4 => C::F4,
            5 => C::F5,
            6 => C::F6,
            7 => C::F7,
            8 => C::F8,
            9 => C::F9,
            10 => C::F10,
            11 => C::F11,
            12 => C::F12,
            13 => C::F13,
            14 => C::F14,
            15 => C::F15,
            16 => C::F16,
            17 => C::F17,
            18 => C::F18,
            19 => C::F19,
            20 => C::F20,
            21 => C::F21,
            22 => C::F22,
            23 => C::F23,
            _ => C::F24,
        },
    }
}

/// Windows backend over the pinned `global-hotkey` wrapper:
/// `RegisterHotKey`-class registration with OS-delivered press events.
///
/// The manager handle must outlive every registration; it is dropped with
/// the process, which unregisters everything at once (the OS also releases
/// registrations automatically when the process exits for any reason).
#[cfg(target_os = "windows")]
#[derive(Debug)]
pub struct WindowsHotkeyRegistrar;

#[cfg(target_os = "windows")]
mod windows_impl {
    //! Windows registration worker. `GlobalHotKeyManager` owns a
    //! non-`Send` OS handle, so ALL manager access lives on one dedicated
    //! worker thread; other threads talk to it over a command channel and
    //! block on a reply channel. That also serializes every mutation —
    //! concurrent register/unregister races are impossible by construction.
    //! The id-to-combo index is plain data so the event drain runs on any
    //! thread.

    use super::{HotkeyError, HotkeyOperation};
    use openstream_domain::switching::HotkeyCombo;
    use std::collections::HashMap;
    use std::sync::{LazyLock, Mutex, OnceLock, mpsc};

    enum Command {
        Register {
            hotkey: global_hotkey::hotkey::HotKey,
            combo: HotkeyCombo,
            reply: mpsc::Sender<Result<(), HotkeyError>>,
        },
        Unregister {
            hotkey: global_hotkey::hotkey::HotKey,
            reply: mpsc::Sender<Result<(), HotkeyError>>,
        },
    }

    static COMMANDS: OnceLock<mpsc::Sender<Command>> = OnceLock::new();

    /// Platform hotkey id -> canonical combination, filled only after a
    /// confirmed successful registration (drain evidence source).
    static REGISTERED: LazyLock<Mutex<HashMap<u32, HotkeyCombo>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));

    pub(crate) fn modifiers_of(
        combo: &HotkeyCombo,
    ) -> Result<global_hotkey::hotkey::Modifiers, HotkeyError> {
        use openstream_domain::switching::Modifier as M;
        let mut modifiers = global_hotkey::hotkey::Modifiers::empty();
        for modifier in combo.modifiers() {
            let bit = match modifier {
                M::Ctrl => global_hotkey::hotkey::Modifiers::CONTROL,
                M::Alt => global_hotkey::hotkey::Modifiers::ALT,
                M::Shift => global_hotkey::hotkey::Modifiers::SHIFT,
                M::Meta => global_hotkey::hotkey::Modifiers::META,
            };
            modifiers |= bit;
        }
        // The domain grammar guarantees >= 1 modifier; an empty set means
        // state drifted, so refuse instead of registering a bare key.
        if modifiers.is_empty() {
            return Err(HotkeyError::Refused {
                operation: HotkeyOperation::Register,
            });
        }
        Ok(modifiers)
    }

    fn platform_hotkey(combo: &HotkeyCombo) -> Result<global_hotkey::hotkey::HotKey, HotkeyError> {
        Ok(global_hotkey::hotkey::HotKey::new(
            Some(modifiers_of(combo)?),
            super::code_of(combo.key()),
        ))
    }

    /// Spawns (exactly once) the worker that exclusively owns the manager.
    fn ensure_worker() -> &'static mpsc::Sender<Command> {
        COMMANDS.get_or_init(|| {
            let (sender, receiver) = mpsc::channel::<Command>();
            std::thread::Builder::new()
                .name("openstream-hotkeys".to_owned())
                .spawn(move || {
                    // The manager is created ON the worker thread and never
                    // moves across threads; drop at thread exit unregisters
                    // everything (the OS also releases at process exit).
                    let Ok(manager) = global_hotkey::GlobalHotKeyManager::new() else {
                        for command in receiver {
                            match command {
                                Command::Register { reply, .. } => {
                                    let _ = reply.send(Err(HotkeyError::Refused {
                                        operation: HotkeyOperation::Register,
                                    }));
                                }
                                Command::Unregister { reply, .. } => {
                                    let _ = reply.send(Ok(()));
                                }
                            }
                        }
                        return;
                    };
                    for command in receiver {
                        match command {
                            Command::Register {
                                hotkey,
                                combo,
                                reply,
                            } => {
                                let outcome =
                                    manager.register(hotkey).map_err(|error| match error {
                                        global_hotkey::Error::AlreadyRegistered(_) => {
                                            HotkeyError::Conflict {
                                                combo: combo.to_string(),
                                            }
                                        }
                                        _ => HotkeyError::Refused {
                                            operation: HotkeyOperation::Register,
                                        },
                                    });
                                if outcome.is_ok() {
                                    REGISTERED
                                        .lock()
                                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                                        .insert(hotkey.id(), combo);
                                }
                                let _ = reply.send(outcome);
                            }
                            Command::Unregister { hotkey, reply } => {
                                // Unregistering something not (or no longer)
                                // registered stays idempotent success.
                                let _ = manager.unregister(hotkey);
                                REGISTERED
                                    .lock()
                                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                                    .remove(&hotkey.id());
                                let _ = reply.send(Ok(()));
                            }
                        }
                    }
                })
                .expect("hotkey worker thread spawns");
            sender
        })
    }

    fn request(
        build: impl FnOnce(mpsc::Sender<Result<(), HotkeyError>>) -> Command,
        fallback: HotkeyOperation,
    ) -> Result<(), HotkeyError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        COMMANDS
            .get()
            .unwrap_or_else(|| ensure_worker())
            .send(build(reply_tx))
            .map_err(|_| HotkeyError::Refused {
                operation: fallback,
            })?;
        reply_rx.recv().unwrap_or(Err(HotkeyError::Refused {
            operation: fallback,
        }))
    }

    impl super::WindowsHotkeyRegistrar {
        /// Registers one combination through the worker.
        ///
        /// # Errors
        /// Typed [`HotkeyError`] only; conflict names the contested combo.
        pub fn register_combo(&mut self, combo: &HotkeyCombo) -> Result<(), HotkeyError> {
            let hotkey = platform_hotkey(combo)?;
            let combo_owned = combo.clone();
            request(
                |reply| Command::Register {
                    hotkey,
                    combo: combo_owned,
                    reply,
                },
                HotkeyOperation::Register,
            )
        }

        /// Removes one registration through the worker (idempotent).
        ///
        /// # Errors
        /// Typed [`HotkeyError`] only.
        pub fn unregister_combo(&mut self, combo: &HotkeyCombo) -> Result<(), HotkeyError> {
            let hotkey = platform_hotkey(combo)?;
            request(
                |reply| Command::Unregister { hotkey, reply },
                HotkeyOperation::Unregister,
            )
        }

        /// Drains queued OS press deliveries into canonical combinations.
        pub(crate) fn drain_events(&mut self) -> Vec<HotkeyCombo> {
            let mut pressed = Vec::new();
            let receiver = global_hotkey::GlobalHotKeyEvent::receiver();
            while let Ok(event) = receiver.try_recv() {
                if event.state() == global_hotkey::HotKeyState::Pressed {
                    let combo = REGISTERED
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .get(&event.id())
                        .cloned();
                    if let Some(combo) = combo {
                        pressed.push(combo);
                    }
                }
            }
            pressed
        }
    }
}

#[cfg(target_os = "windows")]
impl HotkeyRegistrar for WindowsHotkeyRegistrar {
    fn supported(&self) -> bool {
        true
    }

    fn register(&mut self, combo: &HotkeyCombo) -> Result<(), HotkeyError> {
        windows_impl::modifiers_of(combo)?;
        self.register_combo(combo)
    }

    fn unregister(&mut self, combo: &HotkeyCombo) -> Result<(), HotkeyError> {
        windows_impl::modifiers_of(combo)?;
        self.unregister_combo(combo)
    }

    fn drain_pressed(&mut self) -> Vec<HotkeyCombo> {
        self.drain_events()
    }
}
