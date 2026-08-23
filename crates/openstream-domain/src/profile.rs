//! Profiles: switchable named arrangements of ordered decks
//! (DOMAIN_MODEL.md §3; PRD Stage 1 "profile switching").

use crate::error::DomainError;
use crate::ids::{DeckId, ProfileId, WorkspaceId};
use crate::limits::{MAX_DECKS_PER_PROFILE, check_text};
use serde::{Deserialize, Serialize};

/// A named, ordered arrangement of deck references.
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
}

impl Profile {
    /// Full structural validation: name limits and unique, bounded deck
    /// list. Order is preserved verbatim; it defines the switching order.
    pub fn validate(&self) -> Result<(), DomainError> {
        check_text("name", &self.name)?;
        if self.deck_ids.len() > MAX_DECKS_PER_PROFILE {
            return Err(DomainError::LimitExceeded {
                what: "decks per profile",
                limit: MAX_DECKS_PER_PROFILE,
            });
        }
        for (index, deck_id) in self.deck_ids.iter().enumerate() {
            if self.deck_ids[..index].contains(deck_id) {
                return Err(DomainError::DuplicateDeckRef);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Profile;
    use crate::error::DomainError;
    use crate::ids::{DeckId, ProfileId, WorkspaceId};
    use crate::limits::{MAX_DECKS_PER_PROFILE, MAX_TEXT_BYTES};
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
}
