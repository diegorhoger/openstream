//! Deterministic profile switching (issue #19).
//!
//! Pure domain model for switching the active profile by explicit triggers:
//!
//! - **Global hotkeys** ([`SwitchTrigger::Hotkey`]) — an OS-registered
//!   keyboard shortcut. The operating system delivers registered
//!   combinations to the app; nothing here or downstream installs keyboard
//!   hooks, listens to input streams, or reads keystrokes (hard stop of
//!   issue #19; ADR-0006).
//! - **Focused-app matcher** ([`SwitchTrigger::AppFocus`]) — switches when
//!   exactly one explicitly configured application identity holds keyboard
//!   focus. Identity tokens are structural (`obs64.exe`); titles, window
//!   content, and anything beyond the process image name are never modeled.
//!
//! ## Deterministic conflict rules (documented total order)
//!
//! 1. **Configuration time:** two switch rules anywhere in one workspace
//!    may never bind the same trigger. [`SwitchBoard::from_profiles`]
//!    refuses the second binding with
//!    [`DomainError::ConflictingSwitchRule`] — conflicts are resolved by a
//!    deterministic typed rejection, never by silent priority picking.
//!    Disabled rules reserve their trigger exactly like enabled ones so
//!    re-enabling can never introduce a surprise conflict.
//! 2. **Runtime batch order:** when several events arrive within one
//!    evaluation pass they apply lowest-precedence-class first, so the
//!    highest-precedence class present determines the final active profile:
//!    hotkey presses (explicit user gesture) rank above focused-app
//!    automation. Across passes, chronology decides.
//! 3. **Within a class** ambiguity is impossible by construction because of
//!    rule 1 — no tie-break exists or is needed.
//!
//! ## Authority gating
//!
//! Each mechanism requires its own explicit grant
//! (`os.hotkey.register`, `os.focus.read`). [`MechanismGrants`] carries the
//! evaluated authority view and [`resolve`] reports
//! [`Resolution::DeniedNoGrant`] instead of silently ignoring gated events;
//! revoking a grant stops matching immediately at the next evaluation.

use crate::error::DomainError;
use crate::ids::{ProfileId, WorkspaceId};
use crate::limits::{MAX_APP_IDENTITY_BYTES, MAX_HOTKEY_TOKENS, MAX_SWITCH_RULES_PER_PROFILE};
use core::fmt;
use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeSet;
use std::str::FromStr;

/// Modifier keys of a global shortcut. Canonical serialization order is the
/// declaration order here: `ctrl < alt < shift < meta`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Modifier {
    /// Control.
    Ctrl,
    /// Alt (Option).
    Alt,
    /// Shift.
    Shift,
    /// Meta (Win/Super/Command).
    Meta,
}

impl Modifier {
    const fn token(self) -> &'static str {
        match self {
            Self::Ctrl => "ctrl",
            Self::Alt => "alt",
            Self::Shift => "shift",
            Self::Meta => "meta",
        }
    }

    fn parse(token: &str) -> Option<Self> {
        match token {
            "ctrl" => Some(Self::Ctrl),
            "alt" => Some(Self::Alt),
            "shift" => Some(Self::Shift),
            "meta" => Some(Self::Meta),
            _ => None,
        }
    }
}

impl fmt::Display for Modifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.token())
    }
}

/// The non-modifier key of a global shortcut. Closed vocabulary: letters
/// `a`–`z`, digits `0`–`9`, function keys `f1`–`f24`. Bare keys without at
/// least one modifier never validate (a global bare-key shortcut would
/// swallow ordinary typing).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HotkeyKey {
    /// Letter key `a`–`z`.
    Letter(u8),
    /// Digit key `0`–`9`.
    Digit(u8),
    /// Function key `f1`–`f24`.
    Function(u8),
}

impl HotkeyKey {
    fn token(self) -> String {
        match self {
            Self::Letter(b) => char::from(b).to_ascii_lowercase().to_string(),
            Self::Digit(d) => char::from(d).to_string(),
            Self::Function(n) => format!("f{n}"),
        }
    }
}

fn invalid_combo(reason: &'static str) -> DomainError {
    DomainError::InvalidHotkeyCombo { reason }
}

fn parse_key(token: &str) -> Option<HotkeyKey> {
    let bytes = token.as_bytes();
    if bytes.len() == 1 && bytes[0].is_ascii_lowercase() {
        return Some(HotkeyKey::Letter(bytes[0]));
    }
    if bytes.len() == 1 && bytes[0].is_ascii_digit() {
        return Some(HotkeyKey::Digit(bytes[0]));
    }
    if token.len() >= 2 && token.starts_with('f') && token[1..].chars().all(|c| c.is_ascii_digit())
    {
        let number: u8 = token[1..].parse().ok()?;
        if (1..=24).contains(&number) {
            return Some(HotkeyKey::Function(number));
        }
    }
    None
}

/// One validated global-shortcut combination: at least one modifier plus
/// exactly one key, canonical lowercase form (`ctrl+shift+f5`). Modifiers
/// serialize in canonical [`Modifier`] order regardless of input order.
///
/// Construct through [`FromStr`]; direct construction is impossible outside
/// this module so every combo in memory passed validation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HotkeyCombo {
    modifiers: BTreeSet<Modifier>,
    key: HotkeyKey,
}

impl HotkeyCombo {
    /// The validated modifier set (canonical order).
    pub fn modifiers(&self) -> impl Iterator<Item = Modifier> + '_ {
        self.modifiers.iter().copied()
    }

    /// The validated key.
    #[must_use]
    pub const fn key(&self) -> HotkeyKey {
        self.key
    }
}

impl FromStr for HotkeyCombo {
    type Err = DomainError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        if raw.is_empty() || raw.len() > MAX_HOTKEY_TOKENS * 12 {
            return Err(invalid_combo("empty_or_oversized"));
        }
        let mut modifiers = BTreeSet::new();
        let mut keys: Vec<HotkeyKey> = Vec::new();
        for token in raw.split('+') {
            if token.is_empty() {
                return Err(invalid_combo("empty_token"));
            }
            if let Some(modifier) = Modifier::parse(token) {
                if !modifiers.insert(modifier) {
                    return Err(invalid_combo("duplicate_modifier"));
                }
                continue;
            }
            let Some(key) = parse_key(token) else {
                return Err(invalid_combo("unknown_token"));
            };
            keys.push(key);
        }
        if keys.len() != 1 {
            return Err(if keys.is_empty() {
                invalid_combo("missing_key")
            } else {
                invalid_combo("multiple_keys")
            });
        }
        if modifiers.is_empty() {
            // A global shortcut without any modifier would shadow normal
            // typing everywhere on the desktop; refuse deterministically.
            return Err(invalid_combo("missing_modifier"));
        }
        Ok(Self {
            modifiers,
            key: keys[0],
        })
    }
}

impl fmt::Display for HotkeyCombo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for modifier in self.modifiers() {
            write!(f, "{modifier}+")?;
        }
        f.write_str(&self.key.token())
    }
}

impl Serialize for HotkeyCombo {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for HotkeyCombo {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        HotkeyCombo::from_str(&raw).map_err(DeError::custom)
    }
}

fn invalid_identity(reason: &'static str) -> DomainError {
    DomainError::InvalidAppIdentity { reason }
}

/// Structural identity of one application for focused-app matching: the
/// lowercased process image file name (e.g. `obs64.exe`). Grammar mirrors
/// the launch-adapter identity discipline: bounded lowercase ASCII tokens
/// with `.`, `-`, `_`; no wildcards anywhere; no leading/trailing dot or
/// dash; no empty or double dots.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AppIdentity(String);

impl AppIdentity {
    /// The validated identity string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for AppIdentity {
    type Err = DomainError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        if raw.is_empty() {
            return Err(invalid_identity("empty"));
        }
        if raw.len() > MAX_APP_IDENTITY_BYTES {
            return Err(invalid_identity("too_long"));
        }
        if raw.starts_with(['.', '-']) || raw.ends_with(['.', '-']) {
            return Err(invalid_identity("bad_edges"));
        }
        if !raw
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '_'))
        {
            return Err(invalid_identity("uppercase_or_invalid_character"));
        }
        if raw.contains("..") {
            return Err(invalid_identity("double_dot"));
        }
        Ok(Self(raw.to_owned()))
    }
}

impl fmt::Display for AppIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for AppIdentity {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for AppIdentity {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        AppIdentity::from_str(&raw).map_err(DeError::custom)
    }
}

/// One explicit switch trigger bound to one profile.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SwitchTrigger {
    /// Fire when the OS delivers this registered global shortcut.
    Hotkey {
        /// The validated combination (canonical form).
        combo: HotkeyCombo,
    },
    /// Fire while the configured application holds keyboard focus.
    AppFocus {
        /// The validated application identity token.
        app: AppIdentity,
    },
}

impl SwitchTrigger {
    /// Which mechanism class this trigger belongs to.
    #[must_use]
    pub const fn mechanism(&self) -> Mechanism {
        match self {
            Self::Hotkey { .. } => Mechanism::Hotkey,
            Self::AppFocus { .. } => Mechanism::AppFocus,
        }
    }
}

impl fmt::Display for SwitchTrigger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hotkey { combo } => write!(f, "hotkey:{combo}"),
            Self::AppFocus { app } => write!(f, "app_focus:{app}"),
        }
    }
}

/// One explicit profile-switch rule stored inside its target profile.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwitchRule {
    /// Durable identifier (UUIDv7).
    pub id: crate::ids::SwitchRuleId,
    /// Profile switched TO when the trigger fires.
    pub profile_id: ProfileId,
    /// Owning workspace (mirrors the profile's workspace).
    pub workspace_id: WorkspaceId,
    /// The explicit trigger.
    pub trigger: SwitchTrigger,
    /// Disabled rules stay stored but inert (and still reserve their
    /// trigger, keeping re-enabling predictable).
    pub enabled: bool,
}

impl SwitchRule {
    /// Full validation of this rule in isolation (trigger validity is
    /// enforced by construction; ids are typed). Kept as an explicit hook
    /// for future per-rule constraints.
    pub fn validate(&self) -> Result<(), DomainError> {
        Ok(())
    }
}

/// The two switching mechanisms, each behind its own explicit grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mechanism {
    /// OS-registered global shortcuts (`os.hotkey.register`).
    Hotkey,
    /// Focused-app detection (`os.focus.read`).
    AppFocus,
}

impl Mechanism {
    /// The capability kind name gating this mechanism.
    #[must_use]
    pub const fn capability_kind(self) -> &'static str {
        match self {
            Self::Hotkey => "os.hotkey.register",
            Self::AppFocus => "os.focus.read",
        }
    }

    /// Stable wire/state token.
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::Hotkey => "hotkey",
            Self::AppFocus => "app_focus",
        }
    }
}

/// Evaluated authority view over both mechanisms (intersection result of
/// the grant ledger for the built-in switching subject). Recomputed by the
/// caller before every evaluation — never cached authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MechanismGrants {
    /// Global-hotkey registration granted.
    pub hotkey: bool,
    /// Focused-app detection granted.
    pub app_focus: bool,
}

impl MechanismGrants {
    /// Both mechanisms denied (deny-by-default start state).
    #[must_use]
    pub const fn none() -> Self {
        Self {
            hotkey: false,
            app_focus: false,
        }
    }

    /// Authority for one mechanism.
    #[must_use]
    pub const fn allows(self, mechanism: Mechanism) -> bool {
        match mechanism {
            Mechanism::Hotkey => self.hotkey,
            Mechanism::AppFocus => self.app_focus,
        }
    }
}

/// One runtime switching event delivered to the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwitchEvent {
    /// The OS delivered a registered combination press.
    HotkeyPressed(HotkeyCombo),
    /// The focused application identity changed to this value.
    FocusChanged(AppIdentity),
}

impl SwitchEvent {
    /// Precedence rank inside one evaluation batch: higher wins. Hotkeys
    /// (explicit user gestures) outrank focused-app automation.
    #[must_use]
    pub const fn precedence(&self) -> u8 {
        match self {
            Self::HotkeyPressed(_) => 2,
            Self::FocusChanged(_) => 1,
        }
    }

    /// The event's mechanism.
    #[must_use]
    pub fn mechanism(&self) -> Mechanism {
        match self {
            Self::HotkeyPressed(_) => Mechanism::Hotkey,
            Self::FocusChanged(_) => Mechanism::AppFocus,
        }
    }
}

/// Outcome of resolving one event against the board and grants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// Switch to the resolved profile.
    Switched {
        /// Target profile.
        profile_id: ProfileId,
        /// Rule that produced the decision.
        rule_id: crate::ids::SwitchRuleId,
    },
    /// No enabled rule matched this event; state stays unchanged.
    Unmatched,
    /// The event's mechanism has no active grant; the rules are inert until
    /// consent returns. Surfaced visibly, never silently swallowed.
    DeniedNoGrant {
        /// Gated mechanism.
        mechanism: Mechanism,
    },
}

/// Immutable, validated set of switch rules with deterministic ordering.
///
/// Construction rejects conflicting triggers across ALL profiles (the
/// deterministic conflict resolution of rule 1 above). Iteration order is
/// total and stable: sorted by (mechanism rank, canonical trigger string,
/// rule id), so two boards built from the same profiles always behave
/// identically regardless of insertion order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SwitchBoard {
    rules: Vec<SwitchRule>,
}

fn mechanism_rank(mechanism: Mechanism) -> u8 {
    match mechanism {
        Mechanism::Hotkey => 0,
        Mechanism::AppFocus => 1,
    }
}

impl SwitchBoard {
    /// Builds the board from every profile document's rules, refusing
    /// duplicate triggers deterministically.
    ///
    /// # Errors
    /// [`DomainError::ConflictingSwitchRule`] naming the collided class
    /// when two rules bind the same trigger; [`DomainError::LimitExceeded`]
    /// when a single profile exceeds its rule bound.
    pub fn from_profiles<'a>(
        profiles: impl IntoIterator<Item = &'a crate::profile::Profile>,
    ) -> Result<Self, DomainError> {
        let mut rules: Vec<SwitchRule> = Vec::new();
        for profile in profiles {
            if profile.switch_rules.len() > MAX_SWITCH_RULES_PER_PROFILE {
                return Err(DomainError::LimitExceeded {
                    what: "switch rules per profile",
                    limit: MAX_SWITCH_RULES_PER_PROFILE,
                });
            }
            rules.extend(profile.switch_rules.iter().cloned());
        }
        Self::check_conflicts(&rules)?;
        rules.sort_by(|left, right| {
            (
                mechanism_rank(left.trigger.mechanism()),
                left.trigger.to_string(),
                left.id.to_string(),
            )
                .cmp(&(
                    mechanism_rank(right.trigger.mechanism()),
                    right.trigger.to_string(),
                    right.id.to_string(),
                ))
        });
        Ok(Self { rules })
    }

    /// Empty board; every event resolves [`Resolution::Unmatched`].
    #[must_use]
    pub fn empty() -> Self {
        Self { rules: Vec::new() }
    }

    fn check_conflicts(rules: &[SwitchRule]) -> Result<(), DomainError> {
        for (index, rule) in rules.iter().enumerate() {
            for other in rules[..index].iter() {
                if same_trigger(&rule.trigger, &other.trigger) {
                    return Err(DomainError::ConflictingSwitchRule {
                        kind: rule.trigger.mechanism().token(),
                    });
                }
            }
        }
        Ok(())
    }

    /// All rules in the documented deterministic order.
    #[must_use]
    pub fn rules(&self) -> &[SwitchRule] {
        &self.rules
    }

    /// Number of rules.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// True when no rules exist.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Combos of enabled hotkey rules (canonical order) — the desired
    /// registration set for the platform registrar.
    #[must_use]
    pub fn desired_hotkeys(&self, grants: MechanismGrants) -> Vec<HotkeyCombo> {
        if !grants.hotkey {
            return Vec::new();
        }
        self.rules
            .iter()
            .filter_map(|rule| match (&rule.trigger, rule.enabled) {
                (SwitchTrigger::Hotkey { combo }, true) => Some(combo.clone()),
                _ => None,
            })
            .collect()
    }

    /// Resolves one event against this board under `grants`. Pure.
    #[must_use]
    pub fn resolve(&self, event: &SwitchEvent, grants: MechanismGrants) -> Resolution {
        if !grants.allows(event.mechanism()) {
            return Resolution::DeniedNoGrant {
                mechanism: event.mechanism(),
            };
        }
        for rule in &self.rules {
            if !rule.enabled {
                continue;
            }
            let matched = match (event, &rule.trigger) {
                (SwitchEvent::HotkeyPressed(pressed), SwitchTrigger::Hotkey { combo }) => {
                    pressed == combo
                }
                (SwitchEvent::FocusChanged(app), SwitchTrigger::AppFocus { app: expected }) => {
                    app == expected
                }
                _ => false,
            };
            if matched {
                return Resolution::Switched {
                    profile_id: rule.profile_id,
                    rule_id: rule.id,
                };
            }
        }
        Resolution::Unmatched
    }
}

fn same_trigger(left: &SwitchTrigger, right: &SwitchTrigger) -> bool {
    left == right
}

/// Resolves a batch of events in the documented deterministic order:
/// ascending precedence first, so among *authorized* switches the
/// highest-precedence class present determines the final active profile
/// (hotkeys outrank focused-app automation). Every event resolves
/// independently; a grant-denied or unmatched event never masks an earlier
/// [`Resolution::Switched`] outcome — denial changes nothing, it does not
/// rewind.
///
/// Callers apply outcomes strictly in this order: only
/// [`Resolution::Switched`] updates the active profile. Returns every
/// `(event, resolution)` pair in processed order (empty batch → empty vec),
/// giving callers complete intermediate evidence.
#[must_use]
pub fn resolve_batch(
    board: &SwitchBoard,
    grants: MechanismGrants,
    mut events: Vec<SwitchEvent>,
) -> Vec<(SwitchEvent, Resolution)> {
    events.sort_by_key(|event| event.precedence());
    events
        .into_iter()
        .map(|event| {
            let resolution = board.resolve(&event, grants);
            (event, resolution)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        AppIdentity, HotkeyCombo, HotkeyKey, Mechanism, MechanismGrants, Resolution, SwitchBoard,
        SwitchEvent, SwitchRule, SwitchTrigger, resolve_batch,
    };
    use crate::error::DomainError;
    use crate::ids::{ProfileId, SwitchRuleId, WorkspaceId};
    use std::str::FromStr;

    fn uuid7(seed: u32) -> String {
        format!("018f6a1c-7b21-7{seed:03x}-9f31-{seed:012x}")
    }

    fn profile_id(seed: u32) -> ProfileId {
        ProfileId::from_str(&uuid7(seed)).unwrap()
    }

    fn workspace_id() -> WorkspaceId {
        WorkspaceId::from_str(&uuid7(1)).unwrap()
    }

    fn rule_id(seed: u32) -> SwitchRuleId {
        SwitchRuleId::from_str(&uuid7(seed)).unwrap()
    }

    fn combo(raw: &str) -> HotkeyCombo {
        HotkeyCombo::from_str(raw).unwrap()
    }

    fn app(raw: &str) -> AppIdentity {
        AppIdentity::from_str(raw).unwrap()
    }

    fn hotkey_rule(id: u32, profile: u32, combo_raw: &str) -> SwitchRule {
        SwitchRule {
            id: rule_id(id),
            profile_id: profile_id(profile),
            workspace_id: workspace_id(),
            trigger: SwitchTrigger::Hotkey {
                combo: combo(combo_raw),
            },
            enabled: true,
        }
    }

    fn focus_rule(id: u32, profile: u32, app_raw: &str) -> SwitchRule {
        SwitchRule {
            id: rule_id(id),
            profile_id: profile_id(profile),
            workspace_id: workspace_id(),
            trigger: SwitchTrigger::AppFocus { app: app(app_raw) },
            enabled: true,
        }
    }

    fn grants_all() -> MechanismGrants {
        MechanismGrants {
            hotkey: true,
            app_focus: true,
        }
    }

    #[test]
    fn combos_parse_canonically_and_order_insensitively() {
        assert_eq!(combo("ctrl+shift+f5").to_string(), "ctrl+shift+f5");
        // Input order is irrelevant; output is canonical.
        assert_eq!(combo("shift+ctrl+f5").to_string(), "ctrl+shift+f5");
        assert_eq!(combo("meta+alt+a").to_string(), "alt+meta+a");
        assert_eq!(
            combo("shift+meta+ctrl+alt+z").to_string(),
            "ctrl+alt+shift+meta+z"
        );
        assert_eq!(combo("ctrl+0").key(), HotkeyKey::Digit(b'0'));
        assert_eq!(combo("alt+f24").key(), HotkeyKey::Function(24));
    }

    #[test]
    fn combos_reject_fail_closed() {
        for (raw, reason) in [
            ("", "empty_or_oversized"),
            ("f5", "missing_modifier"),
            ("ctrl", "missing_key"),
            ("ctrl+", "empty_token"),
            ("ctrl+f5+f6", "multiple_keys"),
            ("ctrl+ctrl+f5", "duplicate_modifier"),
            ("ctrl+F5", "unknown_token"),
            ("ctrl+space", "unknown_token"),
            ("ctrl+*", "unknown_token"),
            ("ctrl+f25", "unknown_token"),
        ] {
            let error = HotkeyCombo::from_str(raw).unwrap_err();
            assert!(
                matches!(&error, DomainError::InvalidHotkeyCombo { reason: found } if *found == reason),
                "{raw} gave {error:?}"
            );
        }
    }

    #[test]
    fn identities_validate_strictly() {
        assert_eq!(app("obs64.exe").as_str(), "obs64.exe");
        assert!(AppIdentity::from_str("code_x64").is_ok());
        assert!(AppIdentity::from_str("my-app.exe").is_ok());
        for (raw, reason) in [
            ("", "empty"),
            ("OBS64.exe", "uppercase_or_invalid_character"),
            ("obs 64.exe", "uppercase_or_invalid_character"),
            (".obs64", "bad_edges"),
            ("obs64.", "bad_edges"),
            ("-obs64", "bad_edges"),
            ("ob..s64", "double_dot"),
            ("obs*64", "uppercase_or_invalid_character"),
            (&"a".repeat(65), "too_long"),
        ] {
            let error = AppIdentity::from_str(raw).unwrap_err();
            assert!(
                matches!(&error, DomainError::InvalidAppIdentity { reason: found } if *found == reason),
                "{raw} gave {error:?}"
            );
        }
    }

    #[test]
    fn board_orders_rules_deterministically() {
        let board_rules = vec![
            focus_rule(30, 3, "zoom.exe"),
            hotkey_rule(20, 2, "ctrl+shift+p"),
            hotkey_rule(10, 1, "alt+k"),
            focus_rule(40, 4, "chrome.exe"),
        ];
        let board = SwitchBoard::from_profiles([&profile_with(board_rules)]).unwrap();
        let order: Vec<String> = board
            .rules()
            .iter()
            .map(|r| r.trigger.to_string())
            .collect();
        assert_eq!(
            order,
            vec![
                "hotkey:alt+k",
                "hotkey:ctrl+shift+p",
                "app_focus:chrome.exe",
                "app_focus:zoom.exe",
            ]
        );
        // Rebuilding from a differently ordered profile list is identical.
        let rebuilt = SwitchBoard::from_profiles([
            &profile_with(vec![focus_rule(40, 4, "chrome.exe")]),
            &profile_with(vec![
                focus_rule(30, 3, "zoom.exe"),
                hotkey_rule(10, 1, "alt+k"),
            ]),
            &profile_with(vec![hotkey_rule(20, 2, "ctrl+shift+p")]),
        ])
        .unwrap();
        assert_eq!(rebuilt, board);
    }

    fn profile_with(rules: Vec<SwitchRule>) -> crate::profile::Profile {
        crate::profile::Profile {
            id: profile_id(1),
            workspace_id: workspace_id(),
            name: "p".into(),
            deck_ids: Vec::new(),
            switch_rules: rules,
        }
    }

    #[test]
    fn conflicting_hotkeys_reject_across_profiles() {
        let error = SwitchBoard::from_profiles([
            &profile_with(vec![hotkey_rule(10, 1, "ctrl+shift+p")]),
            &profile_with(vec![hotkey_rule(20, 2, "ctrl+shift+p")]),
        ])
        .unwrap_err();
        assert_eq!(error, DomainError::ConflictingSwitchRule { kind: "hotkey" });
    }

    #[test]
    fn conflicting_app_matchers_reject_across_profiles() {
        let error = SwitchBoard::from_profiles([
            &profile_with(vec![focus_rule(10, 1, "obs64.exe")]),
            &profile_with(vec![focus_rule(20, 2, "obs64.exe")]),
        ])
        .unwrap_err();
        assert_eq!(
            error,
            DomainError::ConflictingSwitchRule { kind: "app_focus" }
        );
    }

    #[test]
    fn disabled_rules_still_reserve_their_trigger() {
        let mut reserved = hotkey_rule(20, 2, "ctrl+shift+p");
        reserved.enabled = false;
        let error = SwitchBoard::from_profiles([
            &profile_with(vec![hotkey_rule(10, 1, "ctrl+shift+p")]),
            &profile_with(vec![reserved]),
        ])
        .unwrap_err();
        assert!(matches!(error, DomainError::ConflictingSwitchRule { .. }));
    }

    #[test]
    fn resolution_switches_by_exact_trigger() {
        let board = SwitchBoard::from_profiles([&profile_with(vec![
            hotkey_rule(10, 1, "ctrl+shift+p"),
            focus_rule(20, 2, "obs64.exe"),
        ])])
        .unwrap();
        let grants = grants_all();
        assert_eq!(
            board.resolve(&SwitchEvent::HotkeyPressed(combo("ctrl+shift+p")), grants),
            Resolution::Switched {
                profile_id: profile_id(1),
                rule_id: rule_id(10)
            }
        );
        assert_eq!(
            board.resolve(&SwitchEvent::FocusChanged(app("obs64.exe")), grants),
            Resolution::Switched {
                profile_id: profile_id(2),
                rule_id: rule_id(20)
            }
        );
        assert_eq!(
            board.resolve(&SwitchEvent::HotkeyPressed(combo("alt+x")), grants),
            Resolution::Unmatched
        );
        assert_eq!(
            board.resolve(&SwitchEvent::FocusChanged(app("other.exe")), grants),
            Resolution::Unmatched
        );
    }

    #[test]
    fn revocation_denies_immediately_per_mechanism() {
        let board = SwitchBoard::from_profiles([&profile_with(vec![
            hotkey_rule(10, 1, "ctrl+shift+p"),
            focus_rule(20, 2, "obs64.exe"),
        ])])
        .unwrap();
        // Only the hotkey mechanism revoked: hotkey events surface the
        // typed denial, focus keeps working.
        let hotkey_revoked = MechanismGrants {
            hotkey: false,
            app_focus: true,
        };
        assert_eq!(
            board.resolve(
                &SwitchEvent::HotkeyPressed(combo("ctrl+shift+p")),
                hotkey_revoked
            ),
            Resolution::DeniedNoGrant {
                mechanism: Mechanism::Hotkey
            }
        );
        assert_eq!(
            board.resolve(&SwitchEvent::FocusChanged(app("obs64.exe")), hotkey_revoked),
            Resolution::Switched {
                profile_id: profile_id(2),
                rule_id: rule_id(20)
            }
        );
        // Revoked mechanisms register nothing.
        assert!(board.desired_hotkeys(hotkey_revoked).is_empty());
        assert_eq!(
            board.desired_hotkeys(grants_all()),
            vec![combo("ctrl+shift+p")]
        );
    }

    #[test]
    fn disabled_rules_never_resolve() {
        let mut disabled = hotkey_rule(10, 1, "ctrl+shift+p");
        disabled.enabled = false;
        let board = SwitchBoard::from_profiles([&profile_with(vec![disabled])]).unwrap();
        assert_eq!(
            board.resolve(
                &SwitchEvent::HotkeyPressed(combo("ctrl+shift+p")),
                grants_all()
            ),
            Resolution::Unmatched
        );
        assert!(board.desired_hotkeys(grants_all()).is_empty());
    }

    #[test]
    fn batch_precedence_hotkey_beats_focus() {
        let board = SwitchBoard::from_profiles([&profile_with(vec![
            hotkey_rule(10, 1, "ctrl+shift+p"),
            focus_rule(20, 2, "obs64.exe"),
            hotkey_rule(30, 3, "alt+m"),
            focus_rule(40, 4, "zoom.exe"),
        ])])
        .unwrap();
        let grants = grants_all();

        // Both mechanisms fire in one batch: the hotkey (higher
        // precedence) decides the final active profile even though the
        // focus change processed first.
        let outcomes = resolve_batch(
            &board,
            grants,
            vec![
                SwitchEvent::FocusChanged(app("obs64.exe")),
                SwitchEvent::HotkeyPressed(combo("alt+m")),
            ],
        );
        assert_eq!(outcomes.len(), 2);
        assert_eq!(outcomes[0].0.mechanism(), Mechanism::AppFocus);
        assert_eq!(
            outcomes[0].1,
            Resolution::Switched {
                profile_id: profile_id(2),
                rule_id: rule_id(20)
            }
        );
        assert_eq!(outcomes[1].0.mechanism(), Mechanism::Hotkey);
        assert_eq!(
            outcomes[1].1,
            Resolution::Switched {
                profile_id: profile_id(3),
                rule_id: rule_id(30)
            }
        );

        // Without any hotkey event the focus change stands.
        let outcomes = resolve_batch(
            &board,
            grants_all(),
            vec![SwitchEvent::FocusChanged(app("obs64.exe"))],
        );
        assert_eq!(outcomes.len(), 1);
        assert!(matches!(outcomes[0].1, Resolution::Switched { .. }));

        // An empty batch changes nothing.
        assert!(resolve_batch(&board, grants_all(), Vec::new()).is_empty());

        // A denied event never masks an authorized switch: with both
        // mechanisms revoked every outcome is the typed denial.
        let all_revoked = MechanismGrants {
            hotkey: false,
            app_focus: false,
        };
        let outcomes = resolve_batch(
            &board,
            all_revoked,
            vec![
                SwitchEvent::FocusChanged(app("obs64.exe")),
                SwitchEvent::HotkeyPressed(combo("alt+m")),
            ],
        );
        assert_eq!(
            outcomes[0].1,
            Resolution::DeniedNoGrant {
                mechanism: Mechanism::AppFocus
            }
        );
        assert_eq!(
            outcomes[1].1,
            Resolution::DeniedNoGrant {
                mechanism: Mechanism::Hotkey
            }
        );

        // Revoked hotkeys do not rewind an authorized focus switch that
        // resolved in the same batch.
        let hotkey_revoked = MechanismGrants {
            hotkey: false,
            app_focus: true,
        };
        let outcomes = resolve_batch(
            &board,
            hotkey_revoked,
            vec![
                SwitchEvent::FocusChanged(app("zoom.exe")),
                SwitchEvent::HotkeyPressed(combo("alt+m")),
            ],
        );
        assert_eq!(
            outcomes[1].1,
            Resolution::DeniedNoGrant {
                mechanism: Mechanism::Hotkey
            },
            "the denial is still surfaced honestly"
        );
        // ...but only the authorized focus switch changed state:
        assert!(matches!(&outcomes[0].1, Resolution::Switched { .. }));
    }

    #[test]
    fn serde_round_trips_triggers_and_combos() {
        let trigger = SwitchTrigger::Hotkey {
            combo: combo("ctrl+alt+7"),
        };
        let json = serde_json::to_string(&trigger).unwrap();
        assert_eq!(json, r#"{"kind":"hotkey","combo":"ctrl+alt+7"}"#);
        let back: SwitchTrigger = serde_json::from_str(&json).unwrap();
        assert_eq!(back, trigger);

        let focus = SwitchTrigger::AppFocus {
            app: app("obs64.exe"),
        };
        let json = serde_json::to_string(&focus).unwrap();
        assert_eq!(json, r#"{"kind":"app_focus","app":"obs64.exe"}"#);
        assert!(
            serde_json::from_str::<SwitchTrigger>(r#"{"kind":"hotkey","combo":"f5"}"#).is_err()
        );
        assert!(
            serde_json::from_str::<SwitchTrigger>(r#"{"kind":"app_focus","app":"BAD"}"#).is_err()
        );
    }
}
