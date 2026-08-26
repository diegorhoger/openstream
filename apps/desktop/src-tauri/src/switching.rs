//! Profile switching service (issue #19).
//!
//! Composes the domain switching model ([`openstream_domain::switching`])
//! with the platform ports ([`crate::hotkeys`], [`crate::focus`]) behind
//! one serialized, deterministic engine:
//!
//! - **Grants explicit per mechanism:** global-hotkey registration
//!   (`os.hotkey.register`) and focused-app detection (`os.focus.read`)
//!   each require their own recorded first-use consent. Authority is
//!   recomputed from the grant ledger before EVERY evaluation â€” never
//!   cached.
//! - **Revocation stops matching immediately:** revoking a mechanism's
//!   grant unregisters that mechanism's shortcuts / stops focus polling on
//!   the very same call and makes its rules inert at the next evaluation.
//! - **Visible degradation:** unsupported platforms, refused registrations
//!   (combinations owned by another application), and unreadable focus all
//!   surface as typed state tokens â€” never silent failure.
//! - **Deterministic lifecycle:** every configuration change reconciles
//!   the applied-registration set against the desired set derived from the
//!   validated board; interleaved add/remove/revoke sequences converge to
//!   exactly the desired registrations with no leaks or double-fires.
//!
//! Consent records live in an in-memory ledger this milestone: after a
//! restart both mechanisms start DENIED until the user re-grants them.
//! That is deliberately conservative (fail closed) and documented in
//! `docs/architecture/PROFILE_SWITCHING.md`; durable grant storage arrives
//! with its own milestone.

use std::fmt;
use std::str::FromStr as _;

use openstream_domain::capability::Capability;
use openstream_domain::error::DomainError;
use openstream_domain::grant::{
    CapabilityRequest, ConsentEvidence, ConsentKind, Decision, GrantLedger, ManifestDeclaration,
    SubjectRef,
};
use openstream_domain::ids::{ProfileId, SwitchRuleId};
use openstream_domain::switching::{
    AppIdentity, HotkeyCombo, Mechanism, MechanismGrants, Resolution, SwitchBoard, SwitchEvent,
};
use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Manager as _};

use crate::focus::FocusSource;
use crate::hotkeys::HotkeyRegistrar;
use crate::studio::WorkspaceSnapshot;

/// Built-in subject owning the switching authority.
const SWITCHING_SUBJECT: &str = "builtin:profile-switching";

/// How often one [`SwitchService::poll_tick`] may observe focus. Identity
/// observations only â€” titles/content are never read at any cadence.
pub const FOCUS_POLL_MS: u64 = 500;

/// Typed, closed-vocabulary reasons a mechanism currently cannot work.
/// Every variant renders in user-visible state; nothing degrades silently.
/// Ordered deterministically so issue lists are stable across rebuilds.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum DegradedReason {
    /// The platform build ships no backend for this mechanism.
    Unsupported {
        /// Platform label (echo-safe).
        os: &'static str,
    },
    /// The OS refused registering one combination (platform refusal).
    RegisterRefused {
        /// Canonical contested combination.
        combo: String,
    },
    /// The combination is already registered â€” another application holds
    /// it, or registration state drifted defensively.
    RegisterConflict {
        /// Canonical contested combination.
        combo: String,
    },
    /// Removing a stale registration was refused by the OS.
    UnregisterRefused {
        /// Canonical contested combination.
        combo: String,
    },
    /// The focus observation failed transiently (secure desktop, refusal).
    FocusUnreadable,
}

impl fmt::Display for DegradedReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { os } => write!(f, "unsupported:{os}"),
            Self::RegisterRefused { combo } => write!(f, "register-refused:{combo}"),
            Self::RegisterConflict { combo } => write!(f, "register-conflict:{combo}"),
            Self::UnregisterRefused { combo } => write!(f, "unregister-refused:{combo}"),
            Self::FocusUnreadable => f.write_str("focus-unreadable"),
        }
    }
}

/// One applied profile change, for caller-visible evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedSwitch {
    /// Newly active profile.
    pub profile_id: ProfileId,
    /// Rule that produced the decision.
    pub rule_id: SwitchRuleId,
    /// Which trigger class fired (`hotkey` / `app_focus`).
    pub mechanism_token: &'static str,
}

/// Serializable typed projection consumed by the WebView surfaces. Every
/// degraded condition appears here explicitly â€” an empty issue list IS the
/// contract for "healthy".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MechanismState {
    /// An explicit, unrevoked grant covers this mechanism right now.
    pub granted: bool,
    /// This build ships a working backend on this platform.
    pub supported: bool,
    /// Closed-vocabulary degradation token(s), sorted, empty when healthy.
    pub issues: Vec<String>,
}

/// Serializable typed surface state for the whole switching engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwitchSurfaceState {
    /// Currently active profile id, if any switch happened.
    pub active_profile: Option<String>,
    /// Global-shortcut mechanism state.
    pub hotkeys: MechanismState,
    /// Focused-app mechanism state.
    pub app_focus: MechanismState,
    /// Number of validated switch rules on the live board.
    pub rule_count: usize,
    /// True when persisted rules could not form a valid board (typed
    /// conflict); the board runs EMPTY and visibly degraded in that case.
    pub board_conflict: bool,
}

/// Typed failures of consent operations (closed vocabulary).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwitchServiceError {
    /// A required record (grant) does not exist; revoking nothing fails
    /// closed instead of silently succeeding.
    NotFound {
        /// Structural entity class ("grant").
        entity: &'static str,
    },
    /// Domain rejection from the underlying ledger operation.
    Domain(DomainError),
}

impl fmt::Display for SwitchServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { entity } => write!(f, "not_found:{entity}"),
            Self::Domain(error) => write!(f, "{error}"),
        }
    }
}

/// One explicit user consent action over one mechanism, as delivered from
/// the WebView (closed vocabulary; unknown tokens reject fail-closed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentAction {
    /// Grant global-shortcut registration (first use).
    GrantHotkey,
    /// Revoke global-shortcut registration.
    RevokeHotkey,
    /// Grant focused-app detection (first use).
    GrantAppFocus,
    /// Revoke focused-app detection.
    RevokeAppFocus,
}

impl ConsentAction {
    fn parse(token: &str) -> Option<Self> {
        match token {
            "grant_hotkey" => Some(Self::GrantHotkey),
            "revoke_hotkey" => Some(Self::RevokeHotkey),
            "grant_app_focus" => Some(Self::GrantAppFocus),
            "revoke_app_focus" => Some(Self::RevokeAppFocus),
            _ => None,
        }
    }
}

/// The serialized switching engine. All mutation goes through methods that
/// recompute authority, reconcile registrations, and refresh typed state;
/// callers serialize access externally (one mutex in the shell state).
pub struct SwitchService {
    subject: SubjectRef,
    ledger: GrantLedger,
    manifest: ManifestDeclaration,
    board: SwitchBoard,
    board_conflict: bool,
    active: Option<ProfileId>,
    registrar: Box<dyn HotkeyRegistrar>,
    focus_source: Box<dyn FocusSource>,
    applied_hotkeys: Vec<HotkeyCombo>,
    issues: [Vec<DegradedReason>; 2],
    last_focus: Result<Option<AppIdentity>, ()>,
}

impl fmt::Debug for SwitchService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SwitchService")
            .field("board_rules", &self.board.len())
            .field("board_conflict", &self.board_conflict)
            .field("active", &self.active.map(|id| id.to_string()))
            .field("applied_hotkeys", &self.applied_hotkeys)
            .finish_non_exhaustive()
    }
}

impl SwitchService {
    /// Builds the service over injected platform ports. Starts fully
    /// denied (no grants), no active profile, no registrations.
    #[must_use]
    pub fn new(registrar: Box<dyn HotkeyRegistrar>, focus_source: Box<dyn FocusSource>) -> Self {
        Self {
            subject: SubjectRef::from_str(SWITCHING_SUBJECT)
                .expect("fixed subject literal validates"),
            ledger: GrantLedger::new(),
            manifest: ManifestDeclaration::try_new(vec![
                Capability::OsHotkeyRegister,
                Capability::OsFocusRead,
            ])
            .expect("built-in declaration validates"),
            board: SwitchBoard::empty(),
            board_conflict: false,
            active: None,
            registrar,
            focus_source,
            applied_hotkeys: Vec::new(),
            issues: [Vec::new(), Vec::new()],
            last_focus: Ok(None),
        }
    }

    fn index_of(mechanism: Mechanism) -> usize {
        match mechanism {
            Mechanism::Hotkey => 0,
            Mechanism::AppFocus => 1,
        }
    }

    /// Freshly recomputed authority view â€” never cached between calls.
    fn granted(&self, capability: &Capability) -> bool {
        matches!(
            self.ledger.evaluate(
                &CapabilityRequest {
                    subject: self.subject.clone(),
                    capability: capability.clone(),
                },
                &self.manifest,
            ),
            Decision::Granted { .. }
        )
    }

    fn grants(&self) -> MechanismGrants {
        MechanismGrants {
            hotkey: self.granted(&Capability::OsHotkeyRegister),
            app_focus: self.granted(&Capability::OsFocusRead),
        }
    }

    /// Rebuilds the board from the workspace snapshot and reconciles the
    /// applied registration set. Persisted configurations violating the
    /// cross-profile conflict rules leave the board EMPTY with a visible
    /// conflict flag â€” fail closed, never a silent partial pick.
    pub fn sync_workspace(&mut self, snapshot: &WorkspaceSnapshot) {
        let profiles: Vec<&openstream_domain::profile::Profile> = snapshot
            .profiles
            .iter()
            .map(|document| &document.profile)
            .collect();
        self.board_conflict = false;
        self.board = match SwitchBoard::from_profiles(profiles) {
            Ok(board) => board,
            Err(_) => {
                self.board_conflict = true;
                SwitchBoard::empty()
            }
        };
        self.reconcile();
    }

    /// Applies one explicit user consent action and immediately reconciles
    /// registrations: grants take effect and revocations tear down in the
    /// SAME call.
    ///
    /// # Errors
    /// [`SwitchServiceError`] for ledger failures; revoking when no grant
    /// exists fails closed with [`SwitchServiceError::NotFound`].
    pub fn apply_consent(
        &mut self,
        action: ConsentAction,
        at_ms: i64,
    ) -> Result<(), SwitchServiceError> {
        let (mechanism, granting) = match action {
            ConsentAction::GrantHotkey | ConsentAction::RevokeHotkey => {
                (Mechanism::Hotkey, action == ConsentAction::GrantHotkey)
            }
            ConsentAction::GrantAppFocus | ConsentAction::RevokeAppFocus => {
                (Mechanism::AppFocus, action == ConsentAction::GrantAppFocus)
            }
        };
        let capability = match mechanism {
            Mechanism::Hotkey => Capability::OsHotkeyRegister,
            Mechanism::AppFocus => Capability::OsFocusRead,
        };
        if granting {
            let consent = ConsentEvidence::try_new(vec![ConsentKind::FirstUse], at_ms)
                .map_err(SwitchServiceError::Domain)?;
            self.ledger
                .create_grant(self.subject.clone(), capability, consent, at_ms)
                .map_err(SwitchServiceError::Domain)?;
        } else {
            let grant_ids: Vec<_> = self
                .ledger
                .active_grants()
                .filter(|grant| {
                    grant.subject() == &self.subject && grant.capability() == &capability
                })
                .map(openstream_domain::grant::Grant::id)
                .collect();
            if grant_ids.is_empty() {
                return Err(SwitchServiceError::NotFound { entity: "grant" });
            }
            for grant_id in grant_ids {
                self.ledger
                    .revoke_grant(grant_id, at_ms)
                    .map_err(SwitchServiceError::Domain)?;
            }
        }
        // Revocation must stop matching immediately: reconcile tears down
        // this mechanism's registrations inside this very call.
        self.reconcile();
        Ok(())
    }

    /// Converges the applied-registration set onto the CURRENT desire
    /// derived from board Ã— grants:
    ///
    /// 1. Removals first â€” anything applied but no longer desired is
    ///    unregistered, so a revoked/disabled mechanism stops listening
    ///    even if later steps fail. A refused removal keeps its applied
    ///    marker (the OS still delivers it; pretending otherwise would be
    ///    dishonest) and surfaces a typed issue.
    /// 2. Additions second â€” desired-but-missing combinations register;
    ///    conflicts/refusals land in the visible-issue list and stay
    ///    retried on every future reconciliation until they converge.
    fn reconcile(&mut self) {
        let grants = self.grants();
        let desired = self.board.desired_hotkeys(grants);
        let registrar_supported = self.registrar.supported();

        // Issue lists are RECOMPUTED from scratch each pass (plus the
        // sticky focus-unreadable episode marker), so repeated
        // reconciliations are idempotent in their visible output.
        let mut hotkey_issues: Vec<DegradedReason> = Vec::new();
        let mut focus_issues: Vec<DegradedReason> = Vec::new();

        // Sticky episode marker while the last observation failed.
        if self.last_focus.is_err() {
            focus_issues.push(DegradedReason::FocusUnreadable);
        }
        // Platform support facts.
        if !registrar_supported && !desired.is_empty() {
            hotkey_issues.push(DegradedReason::Unsupported {
                os: std::env::consts::OS,
            });
        }
        if !self.focus_source.supported() {
            focus_issues.push(DegradedReason::Unsupported {
                os: std::env::consts::OS,
            });
        }

        for combo in self.applied_hotkeys.clone() {
            if desired.contains(&combo) {
                continue;
            }
            if self.registrar.unregister(&combo).is_err() {
                hotkey_issues.push(DegradedReason::UnregisterRefused {
                    combo: combo.to_string(),
                });
                continue;
            }
            self.applied_hotkeys.retain(|applied| applied != &combo);
        }

        if !desired.is_empty() && registrar_supported {
            for combo in desired {
                if self.applied_hotkeys.contains(&combo) {
                    continue;
                }
                match self.registrar.register(&combo) {
                    Ok(()) => self.applied_hotkeys.push(combo),
                    Err(crate::hotkeys::HotkeyError::Conflict { combo }) => {
                        hotkey_issues.push(DegradedReason::RegisterConflict { combo });
                    }
                    Err(crate::hotkeys::HotkeyError::Refused { .. }) => {
                        hotkey_issues.push(DegradedReason::RegisterRefused {
                            combo: combo.to_string(),
                        });
                    }
                    Err(crate::hotkeys::HotkeyError::Unsupported { os }) => {
                        hotkey_issues.push(DegradedReason::Unsupported { os });
                    }
                }
            }
        }

        hotkey_issues.sort();
        hotkey_issues.dedup();
        focus_issues.sort();
        focus_issues.dedup();
        self.issues[Self::index_of(Mechanism::Hotkey)] = hotkey_issues;
        self.issues[Self::index_of(Mechanism::AppFocus)] = focus_issues;
    }

    /// Resolves injected events against the current board and freshly
    /// recomputed authority. Deterministic order matches the domain batch
    /// semantics: ascending precedence first, so among authorized switches
    /// the highest-precedence class present determines the final active
    /// profile (explicit hotkeys outrank focused-app automation).
    /// Denied/unmatched outcomes change nothing — denial never rewinds an
    /// authorized switch.
    #[must_use]
    pub fn handle_events(&mut self, mut events: Vec<SwitchEvent>) -> Vec<AppliedSwitch> {
        events.sort_by_key(|event| event.precedence());
        let mut applied = Vec::new();
        for event in events {
            let mechanism = event.mechanism();
            let grants = self.grants();
            if let Resolution::Switched {
                profile_id,
                rule_id,
            } = self.board.resolve(&event, grants)
            {
                self.active = Some(profile_id);
                applied.push(AppliedSwitch {
                    profile_id,
                    rule_id,
                    mechanism_token: mechanism.token(),
                });
            }
        }
        applied
    }

    /// One poll tick: drains OS-delivered hotkey presses, observes focus
    /// ONLY while the mechanism is granted AND supported (revocation stops
    /// the observation loop itself, not just matching), and applies the
    /// batch deterministically.
    #[must_use]
    pub fn poll_tick(&mut self) -> Vec<AppliedSwitch> {
        let mut events = self
            .registrar
            .drain_pressed()
            .into_iter()
            .map(SwitchEvent::HotkeyPressed)
            .collect::<Vec<_>>();

        if self.granted(&Capability::OsFocusRead) && self.focus_source.supported() {
            match self.focus_source.focused_app() {
                Ok(Some(app)) => {
                    let changed = match &self.last_focus {
                        Ok(previous) => previous.as_ref() != Some(&app),
                        Err(()) => true,
                    };
                    self.last_focus = Ok(Some(app.clone()));
                    self.sync_focus_episode_marker();
                    if changed {
                        events.push(SwitchEvent::FocusChanged(app));
                    }
                }
                Ok(None) => {
                    self.last_focus = Ok(None);
                    self.sync_focus_episode_marker();
                }
                Err(_) => {
                    // Mark once per degradation episode; recovery clears it.
                    self.last_focus = Err(());
                    self.sync_focus_episode_marker();
                }
            }
        }

        self.handle_events(events)
    }

    /// Keeps the visible focus-unreadable marker exactly in sync with the
    /// observation episode state (present while the last read failed,
    /// cleared by the first healthy read afterwards).
    fn sync_focus_episode_marker(&mut self) {
        let index = Self::index_of(Mechanism::AppFocus);
        let failed = self.last_focus.is_err();
        self.issues[index].retain(|reason| *reason != DegradedReason::FocusUnreadable);
        if failed {
            self.issues[index].push(DegradedReason::FocusUnreadable);
        }
        self.issues[index].sort();
        self.issues[index].dedup();
    }

    /// The typed surface projection (visible state, never silent).
    #[must_use]
    pub fn state(&self) -> SwitchSurfaceState {
        let grants = self.grants();
        let mechanism_state = |mechanism: Mechanism| {
            let index = Self::index_of(mechanism);
            MechanismState {
                granted: grants.allows(mechanism),
                supported: match mechanism {
                    Mechanism::Hotkey => self.registrar.supported(),
                    Mechanism::AppFocus => self.focus_source.supported(),
                },
                issues: self.issues[index]
                    .iter()
                    .map(std::string::ToString::to_string)
                    .collect(),
            }
        };
        SwitchSurfaceState {
            active_profile: self.active.map(|id| id.to_string()),
            hotkeys: mechanism_state(Mechanism::Hotkey),
            app_focus: mechanism_state(Mechanism::AppFocus),
            rule_count: self.board.len(),
            board_conflict: self.board_conflict,
        }
    }

    /// Number of combinations currently registered with the OS.
    #[cfg(test)]
    #[must_use]
    pub fn applied_registration_count(&self) -> usize {
        self.applied_hotkeys.len()
    }

    /// The append-only audit trail accumulated through consent changes.
    #[cfg(test)]
    #[must_use]
    pub fn audit_log(&self) -> &openstream_domain::audit::AuditLog {
        self.ledger.audit_log()
    }
}

/// Result payload for the WebView switching-state command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SwitchLoadResult {
    /// Typed engine state (mechanisms, active profile, issues).
    pub state: SwitchSurfaceState,
}

/// Loads the typed switching surface state.
///
/// # Errors
/// [`CommandError`] with token "switch-unavailable" when the shell state is
/// not yet composed.
#[tauri::command]
pub fn switch_state_load(
    shell: tauri::State<'_, crate::ShellHandles>,
) -> Result<SwitchLoadResult, crate::studio::CommandError> {
    let service = crate::lock(&shell.switching);
    Ok(SwitchLoadResult {
        state: service.state(),
    })
}

/// Applies one EXPLICIT user consent action (first-use grant or revoke)
/// and returns the refreshed typed state. Consent timestamps are minted by
/// the shell clock, never trusted from the caller.
///
/// # Errors
/// [`CommandError`] with closed tokens: "unknown-consent-action" or the
/// mapped ledger refusal ("not_found:grant", domain failures).
#[tauri::command]
pub fn switch_consent(
    shell: tauri::State<'_, crate::ShellHandles>,
    action: String,
) -> Result<SwitchLoadResult, crate::studio::CommandError> {
    let parsed = ConsentAction::parse(&action).ok_or(crate::studio::CommandError {
        token: "unknown-consent-action".to_owned(),
    })?;
    let at_ms = now_ms();
    let outcome = {
        let mut service = crate::lock(&shell.switching);
        service
            .apply_consent(parsed, at_ms)
            .map(|()| SwitchLoadResult {
                state: service.state(),
            })
    };
    outcome.map_err(|error| crate::studio::CommandError {
        token: error.to_string(),
    })
}

fn now_ms() -> i64 {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default(),
    )
    .unwrap_or(0)
}

/// One poll tick driven by the shell worker thread: re-syncs rules from
/// the studio snapshot (authoring changes take effect within one tick),
/// drains OS hotkey deliveries, and observes focus under grant. Never
/// panics across states; missing composition just skips a tick.
pub fn poll_tick(app: &AppHandle) {
    let snapshot = app.state::<crate::studio::StudioState>().snapshot();
    let shell = app.state::<crate::ShellHandles>();
    if shell.shutdown_started.load(Ordering::SeqCst) {
        return;
    }
    let mut service = crate::lock(&shell.switching);
    if let Some(snapshot) = snapshot.as_ref() {
        service.sync_workspace(snapshot);
    }
    let applied = service.poll_tick();
    for switch in applied {
        // Operator-visible evidence; profile ids are opaque identifiers,
        // never user content.
        eprintln!(
            "openstream-switching: active profile {} via {}",
            switch.profile_id, switch.mechanism_token
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{ConsentAction, DegradedReason, SwitchService, SwitchServiceError};
    use crate::focus::{FocusError, FocusSource};
    use crate::hotkeys::HotkeyRegistrar;
    use crate::studio::WorkspaceSnapshot;
    use openstream_domain::document::ProfileDocument;
    use openstream_domain::ids::{ProfileId, SwitchRuleId, WorkspaceId};
    use openstream_domain::profile::Profile;
    use openstream_domain::switching::{
        AppIdentity, HotkeyCombo, SwitchEvent, SwitchRule, SwitchTrigger,
    };
    use std::str::FromStr as _;
    use std::sync::{Arc, Mutex};

    fn uuid7(seed: u32) -> String {
        format!("018f6a1c-7b21-7{seed:03x}-9f31-{seed:012x}")
    }

    fn profile_id(seed: u32) -> ProfileId {
        ProfileId::from_str(&uuid7(seed)).unwrap()
    }

    fn workspace_id() -> WorkspaceId {
        WorkspaceId::from_str(&uuid7(1)).unwrap()
    }

    fn combo(raw: &str) -> HotkeyCombo {
        HotkeyCombo::from_str(raw).unwrap()
    }

    fn app(raw: &str) -> AppIdentity {
        AppIdentity::from_str(raw).unwrap()
    }

    fn rule(id: u32, profile: u32, trigger: SwitchTrigger) -> SwitchRule {
        SwitchRule {
            id: SwitchRuleId::from_str(&uuid7(id)).unwrap(),
            profile_id: profile_id(profile),
            workspace_id: workspace_id(),
            trigger,
            enabled: true,
        }
    }

    fn hotkey_trigger(raw: &str) -> SwitchTrigger {
        SwitchTrigger::Hotkey { combo: combo(raw) }
    }

    fn focus_trigger(raw: &str) -> SwitchTrigger {
        SwitchTrigger::AppFocus { app: app(raw) }
    }

    fn profile_document(id: u32, rules: Vec<SwitchRule>) -> ProfileDocument {
        ProfileDocument::new(Profile {
            id: profile_id(id),
            workspace_id: workspace_id(),
            name: format!("profile-{id}"),
            deck_ids: vec![],
            switch_rules: rules,
        })
    }

    fn snapshot_of(profiles: Vec<ProfileDocument>) -> WorkspaceSnapshot {
        WorkspaceSnapshot {
            decks: Vec::new(),
            profiles,
        }
    }

    /// Records every call so lifecycle tests assert exact registration
    /// sequences (order, pairing, no leaks). Clones share one log.
    #[derive(Clone)]
    struct RecordingRegistrar {
        calls: Arc<Mutex<Vec<String>>>,
        conflict_on: Arc<Mutex<Vec<String>>>,
    }

    impl Default for RecordingRegistrar {
        fn default() -> Self {
            Self {
                calls: Arc::new(Mutex::new(Vec::new())),
                conflict_on: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    impl std::fmt::Debug for RecordingRegistrar {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("RecordingRegistrar")
        }
    }

    impl RecordingRegistrar {
        fn trace(&self) -> Vec<String> {
            self.calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        }

        fn conflict_on(&self, combo_raw: &str) {
            self.conflict_on
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(combo_raw.to_owned());
        }

        fn release_conflicts(&self) {
            self.conflict_on
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clear();
        }
    }

    impl HotkeyRegistrar for RecordingRegistrar {
        fn supported(&self) -> bool {
            true
        }

        fn register(&mut self, c: &HotkeyCombo) -> Result<(), crate::hotkeys::HotkeyError> {
            let canonical = c.to_string();
            let conflicted = self
                .conflict_on
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .contains(&canonical);
            let mut calls = self
                .calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if conflicted {
                calls.push(format!("conflict:{canonical}"));
                return Err(crate::hotkeys::HotkeyError::Conflict { combo: canonical });
            }
            calls.push(format!("register:{canonical}"));
            Ok(())
        }

        fn unregister(&mut self, c: &HotkeyCombo) -> Result<(), crate::hotkeys::HotkeyError> {
            self.calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(format!("unregister:{}", c));
            Ok(())
        }
    }

    /// Registrar reporting an unsupported platform (visible degradation).
    #[derive(Debug, Default)]
    struct UnsupportedRegistrar;

    impl HotkeyRegistrar for UnsupportedRegistrar {
        fn supported(&self) -> bool {
            false
        }

        fn register(&mut self, _c: &HotkeyCombo) -> Result<(), crate::hotkeys::HotkeyError> {
            Err(crate::hotkeys::HotkeyError::Unsupported {
                os: std::env::consts::OS,
            })
        }

        fn unregister(&mut self, _c: &HotkeyCombo) -> Result<(), crate::hotkeys::HotkeyError> {
            Err(crate::hotkeys::HotkeyError::Unsupported {
                os: std::env::consts::OS,
            })
        }
    }

    /// Scripted focus observations shared by clones.
    type FocusAnswer = Result<Option<AppIdentity>, FocusError>;
    /// One shared answer queue.
    type FocusScript = Arc<Mutex<Vec<FocusAnswer>>>;

    /// Scriptable focus source; clones share one answer queue.
    #[derive(Clone, Default)]
    struct ScriptedFocus {
        answers: FocusScript,
    }

    impl std::fmt::Debug for ScriptedFocus {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("ScriptedFocus")
        }
    }

    impl ScriptedFocus {
        fn answering(answers: &[FocusAnswer]) -> Self {
            Self {
                answers: Arc::new(Mutex::new(answers.to_vec())),
            }
        }
    }

    impl FocusSource for ScriptedFocus {
        fn supported(&self) -> bool {
            true
        }

        fn focused_app(&mut self) -> Result<Option<AppIdentity>, FocusError> {
            let mut queue = self
                .answers
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if queue.is_empty() {
                // Steady state beyond the script.
                return Ok(None);
            }
            queue.remove(0)
        }
    }

    /// Focus backend for a platform without support (visible degradation).
    #[derive(Debug, Default)]
    struct UnsupportedFocus;

    impl FocusSource for UnsupportedFocus {
        fn supported(&self) -> bool {
            false
        }

        fn focused_app(&mut self) -> Result<Option<AppIdentity>, FocusError> {
            Err(FocusError::Unsupported {
                os: std::env::consts::OS,
            })
        }
    }

    const COMBO_A: &str = "ctrl+alt+f5";

    fn service_with(
        registrar: Box<dyn HotkeyRegistrar>,
        focus: Box<dyn FocusSource>,
    ) -> SwitchService {
        SwitchService::new(registrar, focus)
    }

    fn grant_both(service: &mut SwitchService) {
        service
            .apply_consent(ConsentAction::GrantHotkey, 100)
            .unwrap();
        service
            .apply_consent(ConsentAction::GrantAppFocus, 101)
            .unwrap();
    }

    #[test]
    fn starts_denied_with_no_active_profile_and_no_registrations() {
        let snapshot = snapshot_of(vec![profile_document(
            2,
            vec![rule(11, 2, hotkey_trigger(COMBO_A))],
        )]);
        let mut service = service_with(
            Box::<RecordingRegistrar>::default(),
            Box::<ScriptedFocus>::default(),
        );
        service.sync_workspace(&snapshot);
        let state = service.state();
        assert_eq!(state.active_profile, None);
        assert!(!state.hotkeys.granted);
        assert!(!state.app_focus.granted);
        assert_eq!(state.rule_count, 1);
        assert!(!state.board_conflict);
        assert_eq!(service.applied_registration_count(), 0);
        assert!(state.hotkeys.issues.is_empty());
        assert!(state.app_focus.issues.is_empty());
    }

    #[test]
    fn grant_enables_registration_revocation_unregisters_immediately() {
        let snapshot = snapshot_of(vec![profile_document(
            2,
            vec![rule(11, 2, hotkey_trigger(COMBO_A))],
        )]);
        let registrar = Arc::new(RecordingRegistrar::default());
        // The service receives a clone sharing the same call log.
        let mut service = service_with(
            Box::new((*registrar).clone()),
            Box::<ScriptedFocus>::default(),
        );
        service.sync_workspace(&snapshot);

        service
            .apply_consent(ConsentAction::GrantHotkey, 1)
            .unwrap();
        assert_eq!(
            registrar.trace(),
            vec![format!("register:{COMBO_A}")],
            "the grant registers the enabled rule's combo in the same flow"
        );
        assert_eq!(service.applied_registration_count(), 1);

        // Revocation unregisters IMMEDIATELY inside the consent call.
        service
            .apply_consent(ConsentAction::RevokeHotkey, 2)
            .unwrap();
        assert_eq!(
            registrar.trace(),
            vec![
                format!("register:{COMBO_A}"),
                format!("unregister:{COMBO_A}"),
            ]
        );
        assert_eq!(service.applied_registration_count(), 0);

        // Matching stops instantly too: no switch, typed ungranted state.
        let switches = service.handle_events(vec![SwitchEvent::HotkeyPressed(combo(COMBO_A))]);
        assert!(switches.is_empty());
        assert!(!service.state().hotkeys.granted);

        // Audit evidence covers create AND revoke (one event each).
        assert_eq!(service.audit_log().len(), 2, "grant lifecycle audited");
    }

    #[test]
    fn revoking_a_never_granted_mechanism_fails_closed() {
        let mut service = service_with(
            Box::<RecordingRegistrar>::default(),
            Box::<ScriptedFocus>::default(),
        );
        assert_eq!(
            service
                .apply_consent(ConsentAction::RevokeHotkey, 1)
                .unwrap_err(),
            SwitchServiceError::NotFound { entity: "grant" }
        );
    }

    #[test]
    fn disabled_rules_never_register_but_still_reserve_their_trigger() {
        let mut reserved = rule(11, 2, hotkey_trigger(COMBO_A));
        reserved.enabled = false;
        // Same trigger on another profile: the deterministic configuration
        // rejection applies regardless of the reservation being disabled.
        let conflicting = snapshot_of(vec![
            profile_document(2, vec![reserved]),
            profile_document(3, vec![rule(12, 3, hotkey_trigger(COMBO_A))]),
        ]);
        let mut service = service_with(
            Box::<RecordingRegistrar>::default(),
            Box::<ScriptedFocus>::default(),
        );
        service.sync_workspace(&conflicting);
        assert!(
            service.state().board_conflict,
            "cross-profile duplicate triggers degrade visibly"
        );
        assert_eq!(service.state().rule_count, 0);
        service
            .apply_consent(ConsentAction::GrantHotkey, 1)
            .unwrap();
        assert_eq!(
            service.applied_registration_count(),
            0,
            "a degraded board registers nothing"
        );
    }

    #[test]
    fn focus_grant_gates_polling_and_revocation_stops_it_immediately() {
        let snapshot = snapshot_of(vec![profile_document(
            2,
            vec![rule(11, 2, focus_trigger("obs64.exe"))],
        )]);
        let focus = ScriptedFocus::answering(&[Ok(Some(app("obs64.exe")))]);
        let mut service = service_with(
            Box::<RecordingRegistrar>::default(),
            Box::new(focus.clone()),
        );
        service.sync_workspace(&snapshot);

        // Without the grant the poll never reads focus and never switches.
        assert!(service.poll_tick().is_empty());

        // Granting lets the same observation switch.
        service
            .apply_consent(ConsentAction::GrantAppFocus, 1)
            .unwrap();
        let switches = service.poll_tick();
        assert_eq!(switches.len(), 1);
        assert_eq!(switches[0].profile_id, profile_id(2));
        assert_eq!(
            service.state().active_profile,
            Some(profile_id(2).to_string())
        );

        // Revoking stops matching immediately; further polls do nothing.
        service
            .apply_consent(ConsentAction::RevokeAppFocus, 2)
            .unwrap();
        assert!(service.poll_tick().is_empty());
        assert!(!service.state().app_focus.granted);
        let _ = focus; // script shared with the service via clone
    }

    #[test]
    fn focus_observations_fire_only_on_identity_change() {
        let snapshot = snapshot_of(vec![profile_document(
            2,
            vec![rule(11, 2, focus_trigger("obs64.exe"))],
        )]);
        let focus = ScriptedFocus::answering(&[
            Ok(Some(app("obs64.exe"))),
            Ok(Some(app("obs64.exe"))), // unchanged: no new switch
            Ok(Some(app("code.exe"))),  // unrelated app: unmatched
            Err(FocusError::Refused),   // transient failure: visible issue
            Ok(Some(app("obs64.exe"))), // recovery clears the episode
        ]);
        let mut service = service_with(Box::<RecordingRegistrar>::default(), Box::new(focus));
        service.sync_workspace(&snapshot);
        grant_both(&mut service);

        assert_eq!(service.poll_tick().len(), 1, "first observation switches");
        assert!(
            service.poll_tick().is_empty(),
            "unchanged identity stays silent"
        );
        assert!(
            service.poll_tick().is_empty(),
            "unrelated identity is unmatched"
        );

        // The refusal surfaces as a visible typed issue.
        let _ = service.poll_tick();
        assert!(
            service
                .state()
                .app_focus
                .issues
                .contains(&DegradedReason::FocusUnreadable.to_string())
        );

        // Recovery re-detects and clears the degradation.
        let switches = service.poll_tick();
        assert_eq!(switches.len(), 1, "identity change after recovery switches");
        assert!(
            !service
                .state()
                .app_focus
                .issues
                .contains(&DegradedReason::FocusUnreadable.to_string()),
            "recovery clears the visible failure"
        );
    }

    #[test]
    fn registration_conflict_surfaces_visible_issue_and_recovers() {
        let snapshot = snapshot_of(vec![profile_document(
            2,
            vec![rule(11, 2, hotkey_trigger(COMBO_A))],
        )]);
        let registrar = Arc::new(RecordingRegistrar::default());
        registrar.conflict_on(COMBO_A); // foreign application owns it
        let mut service = service_with(
            Box::new((*registrar).clone()),
            Box::<ScriptedFocus>::default(),
        );
        service.sync_workspace(&snapshot);
        service
            .apply_consent(ConsentAction::GrantHotkey, 1)
            .unwrap();

        let state = service.state();
        let expected = DegradedReason::RegisterConflict {
            combo: COMBO_A.to_owned(),
        }
        .to_string();
        assert!(
            state.hotkeys.issues.contains(&expected),
            "the contested combo must surface visibly, got {:?}",
            state.hotkeys.issues
        );
        assert_eq!(service.applied_registration_count(), 0);

        // Foreign owner releases it: the next reconciliation converges.
        registrar.release_conflicts();
        service.sync_workspace(&snapshot);
        assert_eq!(
            registrar.trace(),
            vec![format!("conflict:{COMBO_A}"), format!("register:{COMBO_A}")]
        );
        assert_eq!(service.applied_registration_count(), 1);
        assert!(service.state().hotkeys.issues.is_empty());
    }

    #[test]
    fn unsupported_platform_degrades_visibly_per_mechanism() {
        let snapshot = snapshot_of(vec![
            profile_document(2, vec![rule(11, 2, hotkey_trigger(COMBO_A))]),
            profile_document(3, vec![rule(12, 3, focus_trigger("obs64.exe"))]),
        ]);
        let mut service = service_with(Box::new(UnsupportedRegistrar), Box::new(UnsupportedFocus));
        service.sync_workspace(&snapshot);
        grant_both(&mut service);

        let state = service.state();
        assert_eq!(
            state.hotkeys.issues,
            vec![format!("unsupported:{}", std::env::consts::OS)],
            "hotkey mechanism degrades with a typed token"
        );
        assert_eq!(
            state.app_focus.issues,
            vec![format!("unsupported:{}", std::env::consts::OS)],
            "focus mechanism degrades with a typed token"
        );
        assert_eq!(service.applied_registration_count(), 0);
        assert!(!state.hotkeys.supported);
        assert!(!state.app_focus.supported);
    }

    #[test]
    fn lifecycle_races_converge_to_desired_registrations_exactly() {
        // Interleave rule changes and consent changes; every step must
        // leave applied == desired with an exact, leak-free trace.
        let two_rules = snapshot_of(vec![profile_document(
            2,
            vec![
                rule(11, 2, hotkey_trigger(COMBO_A)),
                rule(13, 2, hotkey_trigger("ctrl+alt+f6")),
            ],
        )]);
        let one_rule = snapshot_of(vec![profile_document(
            2,
            vec![rule(11, 2, hotkey_trigger(COMBO_A))],
        )]);

        let registrar = Arc::new(RecordingRegistrar::default());
        let mut service = service_with(
            Box::new((*registrar).clone()),
            Box::<ScriptedFocus>::default(),
        );

        service.sync_workspace(&two_rules);
        service
            .apply_consent(ConsentAction::GrantHotkey, 1)
            .unwrap();
        assert_eq!(
            registrar.trace(),
            vec![
                format!("register:{COMBO_A}"),
                "register:ctrl+alt+f6".to_string()
            ]
        );

        // Rule removed: only the stale combo unregisters.
        service.sync_workspace(&one_rule);
        assert_eq!(
            registrar.trace()[2..],
            vec!["unregister:ctrl+alt+f6".to_string()][..]
        );
        assert_eq!(service.applied_registration_count(), 1);

        // Re-add the same combo later: exactly one fresh registration.
        service.sync_workspace(&two_rules);
        assert_eq!(
            registrar.trace()[3..],
            vec!["register:ctrl+alt+f6".to_string()][..]
        );
        assert_eq!(service.applied_registration_count(), 2);

        // Revoke-all teardown leaves zero registrations.
        service
            .apply_consent(ConsentAction::RevokeHotkey, 2)
            .unwrap();
        assert_eq!(service.applied_registration_count(), 0);
        let tail = registrar.trace();
        assert_eq!(
            &tail[tail.len() - 2..],
            &[
                format!("unregister:{COMBO_A}"),
                "unregister:ctrl+alt+f6".to_string()
            ][..]
        );
    }

    #[test]
    fn batch_hotkey_overrides_focus_within_one_pass() {
        let snapshot = snapshot_of(vec![
            profile_document(2, vec![rule(11, 2, hotkey_trigger(COMBO_A))]),
            profile_document(3, vec![rule(12, 3, focus_trigger("zoom.exe"))]),
        ]);
        let mut service = service_with(
            Box::<RecordingRegistrar>::default(),
            Box::<ScriptedFocus>::default(),
        );
        service.sync_workspace(&snapshot);
        grant_both(&mut service);

        // One pass carries both events: the explicit hotkey wins even
        // though the automation arrived in the same batch.
        let applied = service.handle_events(vec![
            SwitchEvent::FocusChanged(app("zoom.exe")),
            SwitchEvent::HotkeyPressed(combo(COMBO_A)),
        ]);
        assert_eq!(applied.len(), 2);
        assert_eq!(applied[applied.len() - 1].mechanism_token, "hotkey");
        assert_eq!(
            service.state().active_profile,
            Some(profile_id(2).to_string()),
            "the highest-precedence class determines the final state"
        );

        // Automation alone still works when no explicit event competes.
        let applied = service.handle_events(vec![SwitchEvent::FocusChanged(app("zoom.exe"))]);
        assert_eq!(applied[0].mechanism_token, "app_focus");
        assert_eq!(
            service.state().active_profile,
            Some(profile_id(3).to_string())
        );

        // A denied event never rewinds an authorized switch from the same
        // batch (revoke hotkeys, then send both classes again).
        service
            .apply_consent(ConsentAction::RevokeHotkey, 3)
            .unwrap();
        let active_before = service.state().active_profile.clone();
        let applied = service.handle_events(vec![
            SwitchEvent::FocusChanged(app("zoom.exe")),
            SwitchEvent::HotkeyPressed(combo(COMBO_A)),
        ]);
        assert_eq!(
            applied.len(),
            1,
            "only the authorized mechanism produces a switch record"
        );
        assert_eq!(
            service.state().active_profile,
            active_before,
            "denied hotkeys neither switch nor rewind"
        );
    }
}
