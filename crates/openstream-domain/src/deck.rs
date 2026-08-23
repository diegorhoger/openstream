//! Decks: the versioned container of pages and their folder path
//! (DOMAIN_MODEL.md §3).

use crate::error::DomainError;
use crate::folder::FolderPath;
use crate::ids::{DeckId, WorkspaceId};
use crate::limits::{MAX_PAGES_PER_DECK, check_text};
use crate::page::Page;
use serde::{Deserialize, Serialize};

/// A deck with its ordered pages. Structural edits bump [`Self::revision`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deck {
    /// Durable identifier (UUIDv7).
    pub id: DeckId,
    /// Owning workspace.
    pub workspace_id: WorkspaceId,
    /// User title; validated text.
    pub title: String,
    /// Monotonic structural revision.
    pub revision: u64,
    /// Folder path attribute (folders are not entities).
    pub folder_path: FolderPath,
    /// Ordered pages.
    pub pages: Vec<Page>,
    /// Soft-deletion marker: Unix epoch milliseconds UTC when deleted;
    /// `None` while live. Tombstones keep the identifier forever.
    pub deleted_at: Option<i64>,
}

impl Deck {
    /// Full structural validation of the deck subtree: title limits, page
    /// count, unique ordinals, pages belonging to this deck, and recursive
    /// page/controls validation.
    pub fn validate(&self) -> Result<(), DomainError> {
        check_text("title", &self.title)?;
        if self.pages.len() > MAX_PAGES_PER_DECK {
            return Err(DomainError::LimitExceeded {
                what: "pages per deck",
                limit: MAX_PAGES_PER_DECK,
            });
        }
        for (index, page) in self.pages.iter().enumerate() {
            if page.deck_id != self.id {
                return Err(DomainError::ForeignPageDeck);
            }
            if self.pages[..index]
                .iter()
                .any(|other| other.ordinal == page.ordinal)
            {
                return Err(DomainError::OrdinalConflict);
            }
            page.validate()?;
        }
        Ok(())
    }

    /// The next monotonic structural revision; fails closed at `u64::MAX`
    /// so an edit can never silently reuse the current revision.
    pub const fn next_revision(&self) -> Result<u64, DomainError> {
        match self.revision.checked_add(1) {
            Some(next) => Ok(next),
            None => Err(DomainError::RevisionOverflow),
        }
    }

    /// Returns a copy bumped to [`Self::next_revision`]; fails closed on
    /// overflow (see above).
    pub fn bump_revision(mut self) -> Result<Self, DomainError> {
        self.revision = self.next_revision()?;
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::Deck;
    use crate::error::DomainError;
    use crate::folder::FolderPath;
    use crate::ids::{DeckId, PageId, WorkspaceId};
    use crate::limits::{MAX_PAGES_PER_DECK, MAX_TEXT_BYTES};
    use crate::page::{GridDimensions, Page};
    use std::str::FromStr as _;

    fn uuid7(n: u32) -> String {
        format!("018f6a1c-7b21-7{n:03x}-9f31-{n:012x}")
    }

    fn page_for(deck_id: DeckId, ordinal: u32) -> Page {
        Page {
            id: PageId::from_str(&uuid7(ordinal + 2)).unwrap(),
            deck_id,
            ordinal,
            grid: GridDimensions {
                columns: 4,
                rows: 2,
            },
            controls: Vec::new(),
        }
    }

    fn deck() -> Deck {
        let id = DeckId::from_str(&uuid7(1)).unwrap();
        Deck {
            id,
            workspace_id: WorkspaceId::from_str(&uuid7(0)).unwrap(),
            title: "Studio".into(),
            revision: 7,
            folder_path: FolderPath::parse("live/scene").unwrap(),
            pages: vec![page_for(id, 0)],
            deleted_at: None,
        }
    }

    #[test]
    fn valid_deck_passes_and_soft_delete_round_trips() {
        let d = deck();
        assert_eq!(d.validate(), Ok(()));
        let mut deleted = d;
        deleted.deleted_at = Some(1_755_945_600_000); // synthetic UTC ms
        assert_eq!(deleted.validate(), Ok(()));
        assert_eq!(
            serde_json::to_string(&deleted.deleted_at).unwrap(),
            "1755945600000"
        );
    }

    #[test]
    fn empty_or_oversized_title_rejects() {
        let mut d = deck();
        d.title = " ".into();
        assert_eq!(
            d.validate(),
            Err(DomainError::TextFieldOutOfRange { field: "title" })
        );
        d.title = "x".repeat(MAX_TEXT_BYTES + 1);
        assert!(matches!(
            d.validate(),
            Err(DomainError::TextFieldOutOfRange { .. })
        ));
    }

    #[test]
    fn invalid_folder_path_rejects_on_deserialize() {
        let mut value = serde_json::to_value(deck()).unwrap();
        value["folder_path"] = serde_json::Value::String("bad//path".into());
        let error = serde_json::from_value::<Deck>(value).unwrap_err();
        assert!(
            error.to_string().contains("folder path"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn foreign_page_deck_rejects() {
        let mut d = deck();
        let stranger = DeckId::from_str(&uuid7(9)).unwrap();
        d.pages.push(page_for(stranger, 1));
        assert_eq!(d.validate(), Err(DomainError::ForeignPageDeck));
    }

    #[test]
    fn ordinal_conflict_rejects() {
        let mut d = deck();
        d.pages.push(page_for(d.id, 0));
        assert_eq!(d.validate(), Err(DomainError::OrdinalConflict));
    }

    #[test]
    fn page_limit_enforced() {
        let mut d = deck();
        d.pages.clear();
        for ordinal in 0..MAX_PAGES_PER_DECK as u32 + 1 {
            d.pages.push(page_for(d.id, ordinal));
        }
        match d.validate() {
            Err(DomainError::LimitExceeded { what, limit }) => {
                assert_eq!(what, "pages per deck");
                assert_eq!(limit, MAX_PAGES_PER_DECK);
            }
            other => panic!("expected LimitExceeded, got {other:?}"),
        }
    }

    #[test]
    fn revision_bump_is_monotonic_and_fails_closed_on_overflow() {
        let d = deck();
        assert_eq!(d.next_revision().unwrap(), 8);
        let bumped = d.clone().bump_revision().unwrap();
        assert_eq!(bumped.revision, 8);
        assert!(bumped.revision > d.revision);

        let mut maxed = d;
        maxed.revision = u64::MAX;
        assert_eq!(maxed.next_revision(), Err(DomainError::RevisionOverflow));
        assert!(maxed.bump_revision().is_err());
    }

    #[test]
    fn deck_serializes_with_declared_field_order() {
        let json = serde_json::to_string(&deck()).unwrap();
        let expected_prefix = concat!(
            r#"{"id":"018f6a1c-7b21-7001-9f31-000000000001","#,
            r#""workspace_id":"018f6a1c-7b21-7000-9f31-000000000000","#,
            r#""title":"Studio","revision":7,"folder_path":"live/scene""#
        );
        assert!(json.starts_with(expected_prefix), "{json}");
    }
}
