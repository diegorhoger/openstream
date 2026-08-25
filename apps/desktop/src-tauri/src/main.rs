//! OpenStream desktop composition root (issue #16).
//!
//! Composes the merged crates into a dependable background desktop shell:
//!
//! - **Single instance:** an exclusive file lock is acquired BEFORE any
//!   window, tray icon, or store connection exists
//!   ([`crate::single_instance`]). A second launch exits silently instead
//!   of ever becoming a second writer.
//! - **Startup crash recovery:** the execution-journal store opens through
//!   the issue #15 pipeline; damaged stores go through the documented
//!   recovery ladder; crash windows reconcile to `outcome_unknown`
//!   ([`crate::recovery`]). The Engine composes over that durable store
//!   with the real system clock ([`crate::clock`]) — the port realization
//!   the engine crate deferred to this composition root.
//! - **System tray:** every visible state renders deterministically from
//!   the typed model in [`crate::menu`]; autostart toggles ONLY through an
//!   explicit user action and is OFF by default.
//! - **Graceful shutdown:** quit (tray menu or OS session end) runs the
//!   fixed, exactly-once step order from [`crate::shutdown`].
//!
//! Authority boundary: the WebView registers NO IPC commands and the
//! capability file grants zero permissions, so effective power stays
//! unchanged from M0 — native lifecycle plus durable evidence only. Per
//! the PR #75 independent gate, NO source-visibility/input-mute OBS consent
//! surface exists here or anywhere else in this milestone.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod clock;
mod menu;
mod paths;
mod recovery;
mod shutdown;
mod single_instance;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

use openstream_engine::journal::{ExecutionJournal as _, JournalLifecycle};
use openstream_engine::runtime::ActionRuntime;
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Manager, RunEvent, Wry};

use crate::autostart::{AutostartBackend, AutostartError, AutostartStatus};
use crate::menu::{AutostartMenuState, ShellHealth, TrayMenuModel, render_tray_menu};
use crate::recovery::SharedJournal;
use crate::shutdown::{ShutdownFailure, ShutdownStep, ShutdownTask, execute_graceful_shutdown};
use crate::single_instance::{InstanceLock, InstanceLockError};

/// Everything the shell owns across its lifetime; managed as Tauri state.
struct ShellHandles {
    health: Mutex<ShellHealth>,
    autostart: Mutex<Box<dyn AutostartBackend>>,
    /// Last refused autostart change (closed-vocabulary token); surfaced
    /// until the next successful operation clears it.
    autostart_failure: Mutex<Option<String>>,
    tray: Mutex<Option<TrayIcon>>,
    runtime: Mutex<Option<ActionRuntime>>,
    journal_store: Mutex<Option<SharedJournal>>,
    instance_lock: Mutex<Option<InstanceLock>>,
    shutdown_started: AtomicBool,
}

/// Locks through poisoning: SQLite transactions and these plain values
/// cannot be corrupted by a panicked holder, so losing access would only
/// hide the truth.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The one application handle, installed before any widget work happens.
static APP: std::sync::OnceLock<AppHandle> = std::sync::OnceLock::new();

fn current_app() -> Option<&'static AppHandle> {
    APP.get()
}

/// Builds the native menu for a rendered model. Separator rows are implied
/// after the first item and before Quit, matching the model contract.
fn native_menu(model: &TrayMenuModel) -> tauri::Result<Menu<Wry>> {
    let app = current_app().expect("handle installed before any menu is built");
    let mut widgets: Vec<Box<dyn IsMenuItem<Wry>>> = Vec::new();
    let last_index = model.items.len().saturating_sub(1);
    for (index, spec) in model.items.iter().enumerate() {
        if index == 1 || index == last_index {
            widgets.push(Box::new(PredefinedMenuItem::separator(app)?));
        }
        match spec.action {
            menu::MenuAction::ToggleAutostart => widgets.push(Box::new(CheckMenuItem::with_id(
                app,
                spec.action.id(),
                &spec.label,
                spec.enabled,
                spec.checked.unwrap_or(false),
                None::<&str>,
            )?)),
            action => widgets.push(Box::new(MenuItem::with_id(
                app,
                action.id(),
                &spec.label,
                spec.enabled,
                None::<&str>,
            )?)),
        }
    }
    let refs: Vec<&dyn IsMenuItem<Wry>> = widgets.iter().map(Box::as_ref).collect();
    Menu::with_items(app, &refs)
}

/// Current autostart presentation: an explicit refusal wins until the next
/// successful operation clears it; otherwise OS truth decides.
fn current_autostart_state(handles: &ShellHandles) -> AutostartMenuState {
    if let Some(token) = lock(&handles.autostart_failure).clone() {
        return AutostartMenuState::Failed { token };
    }
    match lock(&handles.autostart).status() {
        Ok(AutostartStatus::Enabled) => AutostartMenuState::Available { enabled: true },
        Ok(AutostartStatus::Disabled) => AutostartMenuState::Available { enabled: false },
        Err(AutostartError::Unsupported { os }) => AutostartMenuState::Unavailable { os },
        Err(refused) => AutostartMenuState::Failed {
            token: refused.to_string(),
        },
    }
}

/// Re-renders the tray from current typed state. Every visible change goes
/// through here; nothing else touches the widgets.
fn refresh_tray() -> Result<(), ShutdownFailure> {
    let app = current_app().ok_or(ShutdownFailure)?;
    let handles = app.state::<ShellHandles>();
    let health = *lock(&handles.health);
    let autostart = current_autostart_state(&handles);
    let shutting_down = handles.shutdown_started.load(Ordering::SeqCst);
    let model = render_tray_menu(&health, &autostart, shutting_down);

    let tray_guard = lock(&handles.tray);
    let tray = tray_guard.as_ref().ok_or(ShutdownFailure)?;
    let menu = native_menu(&model).map_err(|_| ShutdownFailure)?;
    tray.set_menu(Some(menu)).map_err(|_| ShutdownFailure)?;
    tray.set_tooltip(Some(model.tooltip))
        .map_err(|_| ShutdownFailure)?;
    Ok(())
}

/// Handles one activated tray row by its stable adapter id.
fn route_menu_action(id: &str) {
    let Some(app) = current_app() else {
        return;
    };
    match id {
        "open-studio" => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
        "toggle-autostart" => {
            // Explicit user action ONLY (issue #16): OFF by default, and a
            // refusal surfaces honestly instead of silently retrying.
            let handles = app.state::<ShellHandles>();
            let turn_on = matches!(
                lock(&handles.autostart).status(),
                Ok(AutostartStatus::Disabled)
            );
            let outcome = {
                let mut backend = lock(&handles.autostart);
                if turn_on {
                    backend.enable()
                } else {
                    backend.disable()
                }
            };
            *lock(&handles.autostart_failure) = outcome.err().map(|error| error.to_string());
            let _ = refresh_tray();
        }
        "quit" => app.exit(0),
        _ => {}
    }
}

/// Shutdown tasks against the managed shell state. All are best-effort per
/// the sequencer contract; failures are reported, never fatal.
#[derive(Debug)]
struct MarkShuttingDownTask;

impl ShutdownTask for MarkShuttingDownTask {
    fn step(&self) -> ShutdownStep {
        ShutdownStep::MarkShellShuttingDown
    }

    fn run(&mut self) -> Result<(), ShutdownFailure> {
        refresh_tray()
    }
}

/// Drops one `Mutex<Option<T>>` shell resource (`T: Sized`).
macro_rules! release_task {
    ($name:ident, $step:expr, $field:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug)]
        struct $name;

        impl ShutdownTask for $name {
            fn step(&self) -> ShutdownStep {
                $step
            }

            fn run(&mut self) -> Result<(), ShutdownFailure> {
                let app = current_app().ok_or(ShutdownFailure)?;
                let handles = app.state::<ShellHandles>();
                lock(&handles.$field).take();
                Ok(())
            }
        }
    };
}

release_task!(
    CloseRuntimeTask,
    ShutdownStep::CloseEngineRuntime,
    runtime,
    "Drops the composed engine runtime; its SQLite connection closes cleanly."
);

#[derive(Debug)]
struct CheckpointJournalTask;

impl ShutdownTask for CheckpointJournalTask {
    fn step(&self) -> ShutdownStep {
        ShutdownStep::CheckpointExecutionJournal
    }

    fn run(&mut self) -> Result<(), ShutdownFailure> {
        let app = current_app().ok_or(ShutdownFailure)?;
        let handles = app.state::<ShellHandles>();
        match lock(&handles.journal_store).as_ref() {
            Some(store) => store.checkpoint().map_err(|_| ShutdownFailure),
            None => Ok(()),
        }
    }
}

release_task!(
    ReleaseStoreTask,
    ShutdownStep::ReleaseJournalStore,
    journal_store,
    "Drops the shell store handle; the last reference closes the database."
);

release_task!(
    ReleaseLockTask,
    ShutdownStep::ReleaseInstanceLock,
    instance_lock,
    "Releases the single-instance lock file."
);

/// Runs the fixed graceful-shutdown order exactly once per process.
fn run_graceful_shutdown() {
    let mut tasks: Vec<Box<dyn ShutdownTask>> = vec![
        Box::new(MarkShuttingDownTask),
        Box::new(CloseRuntimeTask),
        Box::new(CheckpointJournalTask),
        Box::new(ReleaseStoreTask),
        Box::new(ReleaseLockTask),
    ];
    let report = execute_graceful_shutdown(&mut tasks);
    // Honest teardown evidence on stderr; release builds hide consoles so
    // end users never see it, operators still can.
    eprintln!(
        "openstream-desktop shutdown completed={} clean={}",
        report.completed.len(),
        report.fully_clean()
    );
}

fn main() {
    let env_reader = |key: &str| std::env::var(key).ok();
    let data_dir = paths::resolve_data_dir(&env_reader);

    // Single-instance FIRST: a second launch never opens windows, tray,
    // or the journal store.
    let mut instance_lock = None;
    let mut exclusivity_unavailable = false;
    if let Some(dir) = &data_dir {
        match InstanceLock::acquire(dir) {
            Ok(held) => instance_lock = Some(held),
            Err(InstanceLockError::AlreadyRunning) => {
                eprintln!("OpenStream is already running; exiting this launch.");
                return;
            }
            Err(refused) => {
                eprintln!(
                    "openstream-desktop: single-instance guard unavailable ({refused}); continuing without exclusivity."
                );
                exclusivity_unavailable = true;
            }
        }
    }

    // Startup composition; crash recovery happens inside.
    let (health, composition) = match (&data_dir, exclusivity_unavailable) {
        (None, _) => (ShellHealth::DataDirectoryUnknown, None),
        (Some(_), true) => (ShellHealth::PersistenceDegraded, None),
        (Some(dir), false) => match recovery::compose_shell_runtime(dir) {
            Ok(composed) => {
                let health = if composed.report.unknown_outcome_executions > 0 {
                    ShellHealth::NeedsReview {
                        unknown_outcome_executions: composed.report.unknown_outcome_executions,
                    }
                } else {
                    ShellHealth::Ready
                };
                for admission in composed.journal.snapshot_admissions() {
                    if admission.lifecycle == JournalLifecycle::OutcomeUnknown {
                        eprintln!(
                            "openstream-desktop: execution {} awaits review (outcome unknown)",
                            admission.execution_id
                        );
                    }
                }
                (health, Some(composed))
            }
            Err(error) => {
                eprintln!(
                    "openstream-desktop: persistence degraded ({error:?}); starting without the execution journal."
                );
                (ShellHealth::PersistenceDegraded, None)
            }
        },
    };

    // Destructure once so ownership can move into managed state.
    let (runtime, journal_store) = match composition {
        Some(composed) => (Some(composed.runtime), Some(composed.journal)),
        None => (None, None),
    };

    tauri::Builder::default()
        .setup(move |app| {
            let _ = APP.set(app.handle().clone());

            app.manage(ShellHandles {
                health: Mutex::new(health),
                autostart: Mutex::new(autostart::platform_default_backend()),
                autostart_failure: Mutex::new(None),
                tray: Mutex::new(None),
                runtime: Mutex::new(runtime),
                journal_store: Mutex::new(journal_store),
                instance_lock: Mutex::new(instance_lock),
                shutdown_started: AtomicBool::new(false),
            });

            let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/icon.png"))
                .expect("bundled tray icon decodes");
            let tray = TrayIconBuilder::with_id("main-tray")
                .icon(icon)
                .tooltip("OpenStream")
                .on_menu_event(|_app, event| route_menu_action(event.id().as_ref()))
                .build(app)?;
            let handles = app.state::<ShellHandles>();
            *lock(&handles.tray) = Some(tray);

            if refresh_tray().is_err() {
                return Err("tray initialization refused".into());
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            // Background operation: closing the Studio window hides it;
            // quitting is explicit through the tray or session end.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("OpenStream desktop shell failed to start")
        .run(|_app_handle, event| {
            if let RunEvent::ExitRequested { .. } = event {
                run_graceful_shutdown();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::{ShellHandles, current_autostart_state};
    use crate::autostart::{AutostartBackend, AutostartOperation, AutostartStatus, FakeAutostart};
    use crate::menu::AutostartMenuState;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicBool;

    fn handles_with(backend: Box<dyn AutostartBackend>) -> ShellHandles {
        ShellHandles {
            health: Mutex::new(crate::menu::ShellHealth::Ready),
            autostart: Mutex::new(backend),
            autostart_failure: Mutex::new(None),
            tray: Mutex::new(None),
            runtime: Mutex::new(None),
            journal_store: Mutex::new(None),
            instance_lock: Mutex::new(None),
            shutdown_started: AtomicBool::new(false),
        }
    }

    #[test]
    fn enabled_backend_maps_to_available_checked_state() {
        let handles = handles_with(Box::new(FakeAutostart::enabled_now()));
        assert_eq!(
            current_autostart_state(&handles),
            AutostartMenuState::Available { enabled: true }
        );
    }

    #[test]
    fn disabled_backend_maps_to_available_unchecked_state() {
        let handles = handles_with(Box::new(FakeAutostart::default()));
        assert_eq!(
            current_autostart_state(&handles),
            AutostartMenuState::Available { enabled: false }
        );
    }

    #[test]
    fn unsupported_platform_maps_honestly() {
        let handles = handles_with(Box::new(FakeAutostart::unsupported("linux")));
        assert_eq!(
            current_autostart_state(&handles),
            AutostartMenuState::Unavailable { os: "linux" }
        );
    }

    #[test]
    fn refused_change_surfaces_token_until_cleared() {
        let handles = handles_with(Box::new(FakeAutostart::refusing(
            AutostartOperation::Enable,
        )));
        // The refusal only shows once it happened: OS truth first.
        assert_eq!(
            current_autostart_state(&handles),
            AutostartMenuState::Available { enabled: false }
        );

        let refused = {
            let mut backend = super::lock(&handles.autostart);
            backend.enable()
        };
        *super::lock(&handles.autostart_failure) = refused.err().map(|error| error.to_string());

        assert_eq!(
            current_autostart_state(&handles),
            AutostartMenuState::Failed {
                token: "enable-refused".to_string()
            }
        );

        // A later successful operation clears the surfaced refusal.
        {
            let mut backend = super::lock(&handles.autostart);
            backend.enable().expect("second enable succeeds");
        }
        *super::lock(&handles.autostart_failure) = None;
        assert_eq!(
            current_autostart_state(&handles),
            AutostartMenuState::Available { enabled: true }
        );
    }

    #[test]
    fn status_read_refusal_surfaces_as_failed_state() {
        let handles = handles_with(Box::new(FakeAutostart::refusing(AutostartOperation::Read)));
        assert_eq!(
            current_autostart_state(&handles),
            AutostartMenuState::Failed {
                token: "read-refused".to_string()
            }
        );
    }

    #[test]
    fn os_truth_round_trip_through_the_trait_object() {
        let mut backend: Box<dyn AutostartBackend> = Box::new(FakeAutostart::default());
        backend.enable().expect("enable");
        assert_eq!(backend.status(), Ok(AutostartStatus::Enabled));
        backend.disable().expect("disable");
        assert_eq!(backend.status(), Ok(AutostartStatus::Disabled));
    }
}
