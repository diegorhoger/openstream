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
//! Authority boundary: the WebView surface is the Studio editor (issue #17)
//! and exposes exactly four local commands — load/apply/undo/redo over the
//! validated domain documents, autosaved through the #15 pipeline. No OBS
//! consent surface exists here or anywhere else in this milestone (PR #75
//! gate): action *configuration* is not part of this milestone's op
//! vocabulary at all. The capability file still grants zero plugin/core
//! permissions.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod clock;
mod menu;
mod paths;
mod recovery;
mod shutdown;
mod single_instance;
mod studio;

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

impl ShellHandles {
    /// Exactly-once gate for the graceful-shutdown sequence: the winner
    /// flips the flag and runs the sequencer (its first task re-renders the
    /// tray into the shutting-down presentation); every loser observes the
    /// flip and does nothing. All controlled exit paths — tray Quit and OS
    /// session end alike — funnel through this single gate via
    /// `RunEvent::ExitRequested`.
    fn begin_shutdown(&self) -> bool {
        self.shutdown_started
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }
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

/// Runs the fixed graceful-shutdown order at most once per process: an
/// atomic compare-exchange gate decides a single winner before any task is
/// built, so the tray's shutting-down presentation is guaranteed to render
/// exactly once, ahead of teardown. A crash skips the sequencer entirely —
/// that window belongs to journal-backed restart recovery, not to this
/// path.
fn run_graceful_shutdown() {
    let Some(app) = current_app() else {
        return;
    };
    let handles = app.state::<ShellHandles>();
    if !handles.begin_shutdown() {
        return;
    }
    execute_shutdown_sequence();
}

/// The ordered, failure-tolerant teardown itself (stateless; callable only
/// after the gate above has flipped).
fn execute_shutdown_sequence() {
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

/// Everything prepared before the Tauri builder runs; ownership moves into
/// managed state during setup.
struct StartupPreparation {
    instance_lock: Option<InstanceLock>,
    runtime: Option<ActionRuntime>,
    journal_store: Option<SharedJournal>,
    health: ShellHealth,
}

/// Typed startup refusals (closed vocabulary, safe to log verbatim).
const REFUSED_ALREADY_RUNNING: &str = "already-running";
const REFUSED_GUARD_UNAVAILABLE: &str = "guard-unavailable";
const REFUSED_DATA_DIR_UNKNOWN: &str = "data-directory-unknown";

/// Health strictly from durable startup facts. Precedence: pending review
/// beats recovery-with-loss beats ready — a restored/quarantined store must
/// never present as plain "running", but review-required evidence is the
/// most urgent truth when both apply.
fn shell_health_from(report: &recovery::StartupReport) -> ShellHealth {
    if report.unknown_outcome_executions > 0 {
        ShellHealth::NeedsReview {
            unknown_outcome_executions: report.unknown_outcome_executions,
        }
    } else if matches!(
        report.store_outcome,
        recovery::StoreOutcome::Recovered { .. }
    ) {
        ShellHealth::JournalRecovered
    } else {
        ShellHealth::Ready
    }
}

/// Prepares everything startup needs, or refuses. FAIL CLOSED on
/// exclusivity: if the single-instance guard cannot be acquired for ANY
/// reason other than a confirmed second launch, or no data directory can
/// be resolved at all, the process refuses to start rather than running an
/// unguarded shell (`Err` tokens are closed-vocabulary and logged).
///
/// The happy path acquires the lock BEFORE composing the journal so a
/// second launch never becomes a second store connection.
fn prepare_startup(data_dir: Option<&std::path::Path>) -> Result<StartupPreparation, &'static str> {
    let Some(dir) = data_dir else {
        return Err(REFUSED_DATA_DIR_UNKNOWN);
    };

    let instance_lock = match InstanceLock::acquire(dir) {
        Ok(held) => held,
        Err(InstanceLockError::AlreadyRunning) => return Err(REFUSED_ALREADY_RUNNING),
        Err(_) => return Err(REFUSED_GUARD_UNAVAILABLE),
    };

    // Startup composition; crash recovery happens inside.
    let (health, composition) = match recovery::compose_shell_runtime(dir) {
        Ok(composed) => {
            let health = shell_health_from(&composed.report);
            if health == ShellHealth::JournalRecovered {
                eprintln!(
                    "openstream-desktop: journal recovered from damage; some execution history may be missing"
                );
            }
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
    };

    let (runtime, journal_store) = match composition {
        Some(composed) => (Some(composed.runtime), Some(composed.journal)),
        None => (None, None),
    };

    Ok(StartupPreparation {
        instance_lock: Some(instance_lock),
        runtime,
        journal_store,
        health,
    })
}

fn main() {
    let env_reader = |key: &str| std::env::var(key).ok();
    let data_dir = paths::resolve_data_dir(&env_reader);

    // Single-instance FIRST and FAIL CLOSED: a second launch exits this
    // launch silently; an unavailable guard refuses startup outright so an
    // unguarded shell can never exist.
    let preparation = match prepare_startup(data_dir.as_deref()) {
        Ok(prepared) => prepared,
        Err(reason) => {
            if reason == REFUSED_ALREADY_RUNNING {
                eprintln!("OpenStream is already running; exiting this launch.");
                return;
            }
            eprintln!("openstream-desktop: refusing to start ({reason}).");
            std::process::exit(1);
        }
    };
    let StartupPreparation {
        instance_lock,
        runtime,
        journal_store,
        health,
    } = preparation;

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            studio::studio_load,
            studio::studio_apply,
            studio::studio_undo,
            studio::studio_redo
        ])
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

            // Studio session over the authored-document store; without a
            // resolvable data dir it degrades to autosave-off honestly.
            let studio_state = studio::StudioState::new(data_dir.as_deref());
            app.manage(studio_state);

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
    use super::{
        REFUSED_ALREADY_RUNNING, REFUSED_DATA_DIR_UNKNOWN, REFUSED_GUARD_UNAVAILABLE, ShellHandles,
        current_autostart_state, prepare_startup, shell_health_from,
    };
    use crate::autostart::{AutostartBackend, AutostartOperation, AutostartStatus, FakeAutostart};
    use crate::menu::{AutostartMenuState, ShellHealth};
    use crate::single_instance::InstanceLock;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicBool;
    use tempfile::TempDir;

    fn handles_with(backend: Box<dyn AutostartBackend>) -> ShellHandles {
        ShellHandles {
            health: Mutex::new(ShellHealth::Ready),
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
    fn shutdown_gate_admits_exactly_one_winner() {
        let handles = handles_with(Box::new(FakeAutostart::default()));
        assert!(handles.begin_shutdown(), "first caller wins the gate");
        assert!(!handles.begin_shutdown(), "every later caller must no-op");
    }

    fn report(
        store: crate::recovery::StoreOutcome,
        unknown: usize,
    ) -> crate::recovery::StartupReport {
        crate::recovery::StartupReport {
            store_outcome: store,
            quarantined_files: 0,
            backup_restored: false,
            reconciled_crash_windows: 0,
            unknown_outcome_executions: unknown,
        }
    }

    #[test]
    fn health_mapping_prefers_review_then_recovery_then_ready() {
        use crate::recovery::StoreOutcome;
        use openstream_persistence::sqlite::RecoveryOutcome;

        assert_eq!(
            shell_health_from(&report(StoreOutcome::Fresh, 2)),
            ShellHealth::NeedsReview {
                unknown_outcome_executions: 2
            },
            "review-required evidence is the most urgent truth"
        );
        assert_eq!(
            shell_health_from(&report(
                StoreOutcome::Recovered {
                    outcome: RecoveryOutcome::QuarantinedAndRecreated
                },
                0
            )),
            ShellHealth::JournalRecovered,
            "recovery with potential evidence loss never reads as plain running"
        );
        assert_eq!(
            shell_health_from(&report(
                StoreOutcome::Recovered {
                    outcome: RecoveryOutcome::RestoredFromBackup
                },
                0
            )),
            ShellHealth::JournalRecovered,
            "backup restore also rewinds history; it surfaces identically"
        );
        assert_eq!(
            shell_health_from(&report(StoreOutcome::OpenedExisting, 0)),
            ShellHealth::Ready
        );
    }

    #[test]
    fn prepare_startup_refuses_when_the_guard_is_unavailable() {
        // A FILE where the lock directory belongs makes create_dir_all fail:
        // startup must refuse instead of continuing unguarded (F3).
        let dir = TempDir::new().expect("temp dir");
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, b"not a directory").expect("blocker written");
        assert_eq!(
            prepare_startup(Some(&blocker)).err(),
            Some(REFUSED_GUARD_UNAVAILABLE)
        );
    }

    #[test]
    fn prepare_startup_detects_second_instance_before_any_store_connection() {
        let dir = TempDir::new().expect("temp dir");
        let _holder = InstanceLock::acquire(dir.path()).expect("holder");
        assert_eq!(
            prepare_startup(Some(dir.path())).err(),
            Some(REFUSED_ALREADY_RUNNING)
        );
    }

    #[test]
    fn prepare_startup_happy_path_composes_a_ready_unguarded_free_shell() {
        let dir = TempDir::new().expect("temp dir");
        let prepared = prepare_startup(Some(dir.path())).expect("prepares");
        assert_eq!(prepared.health, ShellHealth::Ready);
        assert!(prepared.instance_lock.is_some(), "lock held for lifetime");
        assert!(prepared.runtime.is_some());
        assert!(prepared.journal_store.is_some());
    }

    #[test]
    fn prepare_startup_without_data_directory_fails_closed() {
        assert_eq!(
            prepare_startup(None).err(),
            Some(REFUSED_DATA_DIR_UNKNOWN),
            "no data dir means no exclusivity and no documented store home"
        );
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
