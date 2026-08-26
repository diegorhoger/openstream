//! Profiles: switchable named arrangements of ordered decks
//! (DOMAIN_MODEL.md §3; PRD Stage 1 "profile switching").
//!
//! Since issue #19 a profile also carries its own explicit switch rules
//! ([`crate::switching::SwitchRule`]): the triggers (global hotkey,
//! focused-app matcher) that make this profile the active one. The field is
//! optional on the wire (`serde(default)`), so documents written before
//! #19 decode unchanged — an additive-minor domain change per
//! DOMAIN_MODEL.md §1.

use crate::error::DomainError;
use crate::ids::{DeckId, ProfileId, WorkspaceId};
use crate::limits::{MAX_DECKS_PER_PROFILE, MAX_SWITCH_RULES_PER_PROFILE, check_text};
use serde::{Deserialize, Serialize};

/// A named, ordered arrangement of deck references plus the explicit
/// switch rules that activate it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// Durable identifier (UUIDv7).
    pub id: ProfileId,
    /// Owning workspace.
    pub workspace_id: WorkspaceId,
    /// User-visible profile name; validated text.
    pub name: String,
    /// Ordered deck list; a deck appears at most once.
    pub deck_ids: Vec<DeckId>,
    /// Explicit switch rules targeting this profile. Empty for profiles
    /// authored before #19 or without automation. Cross-profile trigger
    /// conflicts are checked at board construction time
    /// ([`crate::switching::SwitchBoard::from_profiles`]), not here — a
    /// single profile cannot see other profiles' bindings.
    #[serde(default)]
    pub switch_rules: Vec<crate::switching::SwitchRule>,
}

impl Profile {
    /// Full structural validation: name limits and unique, bounded deck
    /// list, bounded rule count. Order is preserved verbatim; it defines
    /// the switching order.
    pub fn validate(&self) -> Result<(), DomainError> {
        check_text("name", &self.name)?;
        if self.deck_ids.len() > MAX_DECKS_PER_PROFILE {
            return Err(DomainError::LimitExceeded {
                what: "decks per profile",
                limit: MAX_DECKS_PER_PROFILE,
            });
        }
        if self.switch_rules.len() > MAX_SWITCH_RULES_PER_PROFILE {
            return Err(DomainError::LimitExceeded {
                what: "switch rules per profile",
                limit: MAX_SWITCH_RULES_PER_PROFILE,
            });
        }
        for (index, deck_id) in self.deck_ids.iter().enumerate() {
            if self.deck_ids[..index].contains(deck_id) {
                return Err(DomainError::DuplicateDeckRef);
            }
        }
        for (index, rule) in self.switch_rules.iter().enumerate() {
            if self.switch_rules[..index]
                .iter()
                .any(|other| other.trigger == rule.trigger)
            {
                return Err(DomainError::ConflictingSwitchRule {
                    kind: rule.trigger.mechanism().token(),
                });
            }
            if rule.profile_id != self.id || rule.workspace_id != self.workspace_id {
                return Err(DomainError::ForeignSwitchRule);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Profile;
    use crate::error::DomainError;
    use crate::ids::{DeckId, ProfileId, SwitchRuleId, WorkspaceId};
    use crate::limits::{MAX_DECKS_PER_PROFILE, MAX_TEXT_BYTES};
    use crate::switching::{HotkeyCombo, SwitchRule, SwitchTrigger};
    use std::str::FromStr as _;

    fn uuid7(n: u32) -> String {
        format!("018f6a1c-7b21-7{n:03x}-9f31-{n:012x}")
    }

    fn profile() -> Profile {
        Profile {
            id: ProfileId::from_str(&uuid7(1)).unwrap(),
            workspace_id: WorkspaceId::from_str(&uuid7(0)).unwrap(),
            name: "Streaming".into(),
            deck_ids: vec![DeckId::from_str(&uuid7(2)).unwrap()],
            switch_rules: Vec::new(),
        }
    }

    fn hotkey_rule(profile: &Profile, combo_raw: &str) -> SwitchRule {
        SwitchRule {
            id: SwitchRuleId::from_str(&uuid7(9)).unwrap(),
            profile_id: profile.id,
            workspace_id: profile.workspace_id,
            trigger: SwitchTrigger::Hotkey {
                combo: HotkeyCombo::from_str(combo_raw).unwrap(),
            },
            enabled: true,
        }
    }

    #[test]
    fn valid_profile_passes() {
        assert_eq!(profile().validate(), Ok(()));
    }

    #[test]
    fn empty_or_oversized_name_rejects() {
        let mut p = profile();
        p.name = "".into();
        assert_eq!(
            p.validate(),
            Err(DomainError::TextFieldOutOfRange { field: "name" })
        );
        p.name = "x".repeat(MAX_TEXT_BYTES + 1);
        assert!(matches!(
            p.validate(),
            Err(DomainError::TextFieldOutOfRange { .. })
        ));
    }

    #[test]
    fn duplicate_deck_ref_rejects() {
        let mut p = profile();
        p.deck_ids.push(p.deck_ids[0]);
        assert_eq!(p.validate(), Err(DomainError::DuplicateDeckRef));
    }

    #[test]
    fn deck_list_limit_enforced_and_order_preserved() {
        let mut p = profile();
        assert_eq!(
            serde_json::to_string(&p.deck_ids).unwrap(),
            format!("[\"{}\"]", uuid7(2))
        );
        p.deck_ids.clear();
        for n in 0..MAX_DECKS_PER_PROFILE as u32 + 1 {
            p.deck_ids.push(DeckId::from_str(&uuid7(n + 10)).unwrap());
        }
        match p.validate() {
            Err(DomainError::LimitExceeded { what, limit }) => {
                assert_eq!(what, "decks per profile");
                assert_eq!(limit, MAX_DECKS_PER_PROFILE);
            }
            other => panic!("expected LimitExceeded, got {other:?}"),
        }
    }

    #[test]
    fn valid_rule_passes_validation() {
        let mut p = profile();
        p.switch_rules.push(hotkey_rule(&p, "ctrl+shift+p"));
        assert_eq!(p.validate(), Ok(()));
    }

    #[test]
    fn duplicate_trigger_within_one_profile_rejects() {
        let mut p = profile();
        p.switch_rules.push(hotkey_rule(&p, "ctrl+shift+p"));
        p.switch_rules.push(hotkey_rule(&p, "ctrl+shift+p"));
        assert_eq!(
            p.validate(),
            Err(DomainError::ConflictingSwitchRule { kind: "hotkey" })
        );
    }

    #[test]
    fn foreign_switch_rule_rejects() {
        let mut p = profile();
        let mut rule = hotkey_rule(&p, "ctrl+shift+p");
        rule.profile_id = ProfileId::from_str(&uuid7(8)).unwrap();
        p.switch_rules.push(rule);
        assert_eq!(p.validate(), Err(DomainError::ForeignSwitchRule));
    }

    #[test]
    fn rule_limit_enforced() {
        use crate::limits::MAX_SWITCH_RULES_PER_PROFILE;
        use crate::switching::{AppIdentity as App, SwitchTrigger::AppFocus};
        let mut p = profile();
        for n in 0..MAX_SWITCH_RULES_PER_PROFILE {
            p.switch_rules.push(SwitchRule {
                id: SwitchRuleId::from_str(&uuid7(u32::try_from(100 + n).unwrap())).unwrap(),
                profile_id: p.id,
                workspace_id: p.workspace_id,
                trigger: AppFocus {
                    app: App::from_str(&format!("app{n}.exe")).unwrap(),
                },
                enabled: true,
            });
        }
        assert_eq!(p.validate(), Ok(()));
        p.switch_rules.push(hotkey_rule(&p, "ctrl+alt+x"));
        assert!(matches!(
            p.validate(),
            Err(DomainError::LimitExceeded { .. })
        ));
    }

    #[test]
    fn rules_field_defaults_on_decode_for_pre19_documents() {
        // Documents written before #19 carry no switch_rules member; they
        // decode with an empty rule list (additive-minor compatibility).
        let legacy = format!(
            r#"{{"id":"{}","workspace_id":"{}","name":"legacy","deck_ids":[]}}"#,
            uuid7(1),
            uuid7(0)
        );
        let decoded: Profile = serde_json::from_str(&legacy).unwrap();
        assert!(decoded.switch_rules.is_empty());
        assert_eq!(decoded.name, "legacy");
    }
}
