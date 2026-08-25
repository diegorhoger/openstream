//! Typed tray menu states and their deterministic rendering (issue #16).
//!
//! The tray is the shell's only user-visible surface in this milestone.
//! Its every state derives from this pure model: [`render_tray_menu`] maps
//! a [`ShellHealth`] plus an [`AutostartMenuState`] onto exact item labels,
//! enablement, check marks, and tooltip text. The Tauri adapter in
//! `main.rs` only translates these specs onto real widgets; it never
//! invents labels or states.
//!
//! Scope guard (PR #75 independent gate): NO menu state exposes or could
//! ever expose source-visibility / input-mute OBS grants. Such consent
//! surfaces require a security ADR, capability-taxonomy update, and human
//! gate before any implementation exists.

/// Stable identifiers for actionable menu items (adapter-facing ids).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    /// Show and focus the Studio window.
    OpenStudio,
    /// Toggle launch-at-login (explicit user action).
    ToggleAutostart,
    /// Begin graceful shutdown.
    Quit,
}

impl MenuAction {
    /// The widget id string used by the Tauri adapter.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::OpenStudio => "open-studio",
            Self::ToggleAutostart => "toggle-autostart",
            Self::Quit => "quit",
        }
    }
}

/// Shell health as surfaced in the tooltip; derived only from durable facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellHealth {
    /// Journal open, crash windows reconciled, nothing pending review.
    Ready,
    /// At least one execution carries `outcome_unknown` evidence that needs
    /// human review. Never inferred as success; never auto-retried.
    NeedsReview {
        /// Number of executions with unknown outcome after reconciliation.
        unknown_outcome_executions: usize,
    },
    /// Persistence refused to open even after recovery: no execution
    /// authority is composed and nothing pretends otherwise.
    PersistenceDegraded,
    /// No data directory could be resolved on this host.
    DataDirectoryUnknown,
}

/// Tray rendering of the autostart capability for the current platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutostartMenuState {
    /// Toggleable; reflects current OS truth.
    Available {
        /// Whether launch-at-login is currently registered.
        enabled: bool,
    },
    /// This platform has no shipped autostart mechanism.
    Unavailable {
        /// Platform label from `std::env::consts::OS` (echo-safe).
        os: &'static str,
    },
    /// The last explicit user change was refused by the backend.
    Failed {
        /// Closed-vocabulary failure token (`AutostartError` display).
        token: String,
    },
}

/// One rendered menu row handed to the platform adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItemSpec {
    /// Which action fires when activated.
    pub action: MenuAction,
    /// Exact user-visible label.
    pub label: String,
    /// Check-mark state for toggle rows (`None` = not a checkable row).
    pub checked: Option<bool>,
    /// Whether activation is currently possible.
    pub enabled: bool,
}

/// Fully rendered tray presentation: ordered items plus tooltip text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayMenuModel {
    /// Tooltip shown while hovering the tray icon.
    pub tooltip: String,
    /// Ordered menu items; separators are implied before/after autostart.
    pub items: Vec<MenuItemSpec>,
}

const APP_NAME: &str = "OpenStream";

fn health_tooltip(health: &ShellHealth) -> String {
    match health {
        ShellHealth::Ready => format!("{APP_NAME} - running"),
        ShellHealth::NeedsReview {
            unknown_outcome_executions,
        } => format!(
            "{APP_NAME} - running - {} outcome unknown after restart (review required)",
            executions_phrase(*unknown_outcome_executions)
        ),
        ShellHealth::PersistenceDegraded => {
            format!("{APP_NAME} - degraded: execution journal unavailable")
        }
        ShellHealth::DataDirectoryUnknown => {
            format!("{APP_NAME} - degraded: data directory unknown")
        }
    }
}

fn executions_phrase(count: usize) -> String {
    if count == 1 {
        "1 execution".to_string()
    } else {
        format!("{count} executions")
    }
}

fn autostart_item(state: &AutostartMenuState) -> MenuItemSpec {
    match state {
        AutostartMenuState::Available { enabled } => MenuItemSpec {
            action: MenuAction::ToggleAutostart,
            label: "Start automatically at login".to_string(),
            checked: Some(*enabled),
            enabled: true,
        },
        AutostartMenuState::Unavailable { os } => MenuItemSpec {
            action: MenuAction::ToggleAutostart,
            label: format!("Autostart unavailable on {os}"),
            checked: Some(false),
            enabled: false,
        },
        AutostartMenuState::Failed { token } => MenuItemSpec {
            action: MenuAction::ToggleAutostart,
            label: format!("Autostart change failed ({token})"),
            checked: None,
            enabled: false,
        },
    }
}

/// Renders the complete tray model. Deterministic: same inputs, same
/// output, on every platform.
#[must_use]
pub fn render_tray_menu(
    health: &ShellHealth,
    autostart: &AutostartMenuState,
    shutting_down: bool,
) -> TrayMenuModel {
    let tooltip = if shutting_down {
        format!("{APP_NAME} - shutting down")
    } else {
        health_tooltip(health)
    };

    let interactive = !shutting_down;
    let mut autostart_row = autostart_item(autostart);
    if !interactive {
        autostart_row.enabled = false;
    }

    let items = vec![
        MenuItemSpec {
            action: MenuAction::OpenStudio,
            label: "Open Studio".to_string(),
            checked: None,
            enabled: interactive,
        },
        // Adapter inserts one separator after Open Studio.
        autostart_row,
        // Adapter inserts one separator before Quit.
        MenuItemSpec {
            action: MenuAction::Quit,
            label: "Quit OpenStream".to_string(),
            checked: None,
            enabled: interactive,
        },
    ];

    TrayMenuModel { tooltip, items }
}

#[cfg(test)]
mod tests {
    use super::{AutostartMenuState, MenuAction, ShellHealth, TrayMenuModel, render_tray_menu};

    fn find(model: &TrayMenuModel, action: MenuAction) -> &super::MenuItemSpec {
        model
            .items
            .iter()
            .find(|item| item.action == action)
            .unwrap_or_else(|| panic!("missing {action:?} row"))
    }

    #[test]
    fn ready_state_renders_studio_toggle_and_quit() {
        let model = render_tray_menu(
            &ShellHealth::Ready,
            &AutostartMenuState::Available { enabled: false },
            false,
        );
        assert_eq!(model.tooltip, "OpenStream - running");
        assert_eq!(model.items.len(), 3);

        let studio = find(&model, MenuAction::OpenStudio);
        assert_eq!(studio.label, "Open Studio");
        assert!(studio.enabled);

        let quit = find(&model, MenuAction::Quit);
        assert_eq!(quit.label, "Quit OpenStream");
        assert!(quit.enabled);
    }

    #[test]
    fn autostart_checkmark_reflects_os_truth() {
        let off = render_tray_menu(
            &ShellHealth::Ready,
            &AutostartMenuState::Available { enabled: false },
            false,
        );
        assert_eq!(find(&off, MenuAction::ToggleAutostart).checked, Some(false));
        assert!(find(&off, MenuAction::ToggleAutostart).enabled);

        let on = render_tray_menu(
            &ShellHealth::Ready,
            &AutostartMenuState::Available { enabled: true },
            false,
        );
        assert_eq!(find(&on, MenuAction::ToggleAutostart).checked, Some(true));
    }

    #[test]
    fn unsupported_platform_disables_the_toggle_honestly() {
        let model = render_tray_menu(
            &ShellHealth::Ready,
            &AutostartMenuState::Unavailable { os: "linux" },
            false,
        );
        let row = find(&model, MenuAction::ToggleAutostart);
        assert_eq!(row.label, "Autostart unavailable on linux");
        assert!(!row.enabled);
    }

    #[test]
    fn backend_failure_surfaces_closed_vocabulary_token() {
        let model = render_tray_menu(
            &ShellHealth::Ready,
            &AutostartMenuState::Failed {
                token: "enable-refused".to_string(),
            },
            false,
        );
        let row = find(&model, MenuAction::ToggleAutostart);
        assert_eq!(row.label, "Autostart change failed (enable-refused)");
        assert!(!row.enabled);
    }

    #[test]
    fn crash_evidence_is_counted_and_pluralized_honestly() {
        let one = render_tray_menu(
            &ShellHealth::NeedsReview {
                unknown_outcome_executions: 1,
            },
            &AutostartMenuState::Available { enabled: false },
            false,
        );
        assert_eq!(
            one.tooltip,
            "OpenStream - running - 1 execution outcome unknown after restart (review required)"
        );

        let many = render_tray_menu(
            &ShellHealth::NeedsReview {
                unknown_outcome_executions: 4,
            },
            &AutostartMenuState::Available { enabled: false },
            false,
        );
        assert!(many.tooltip.contains("4 executions"));
        assert!(many.tooltip.contains("review required"));
    }

    #[test]
    fn degraded_states_name_the_degradation() {
        let store = render_tray_menu(
            &ShellHealth::PersistenceDegraded,
            &AutostartMenuState::Available { enabled: false },
            false,
        );
        assert_eq!(
            store.tooltip,
            "OpenStream - degraded: execution journal unavailable"
        );

        let dir = render_tray_menu(
            &ShellHealth::DataDirectoryUnknown,
            &AutostartMenuState::Available { enabled: false },
            false,
        );
        assert_eq!(dir.tooltip, "OpenStream - degraded: data directory unknown");
    }

    #[test]
    fn shutting_down_freezes_every_action_and_overrides_tooltip() {
        let model = render_tray_menu(
            &ShellHealth::NeedsReview {
                unknown_outcome_executions: 2,
            },
            &AutostartMenuState::Available { enabled: true },
            true,
        );
        assert_eq!(model.tooltip, "OpenStream - shutting down");
        for item in &model.items {
            assert!(
                !item.enabled,
                "{:?} must be disabled while shutting down",
                item.action
            );
        }
        assert_eq!(
            find(&model, MenuAction::ToggleAutostart).checked,
            Some(true)
        );
    }

    #[test]
    fn rendering_is_deterministic() {
        let a = render_tray_menu(
            &ShellHealth::Ready,
            &AutostartMenuState::Available { enabled: true },
            false,
        );
        let b = render_tray_menu(
            &ShellHealth::Ready,
            &AutostartMenuState::Available { enabled: true },
            false,
        );
        assert_eq!(a, b);
    }

    #[test]
    fn action_ids_are_stable_for_the_adapter() {
        assert_eq!(MenuAction::OpenStudio.id(), "open-studio");
        assert_eq!(MenuAction::ToggleAutostart.id(), "toggle-autostart");
        assert_eq!(MenuAction::Quit.id(), "quit");
    }
}
