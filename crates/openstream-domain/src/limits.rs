//! v1 size limits enforced by this crate.
//!
//! `DOMAIN_MODEL.md` fixes no numeric caps for deck-domain text and
//! collections, so these constants ARE the v1 contract for the crate
//! (`schema_version` 1.0). Per DOMAIN_MODEL.md §1, *tightening* any value
//! here rejects previously valid documents and therefore requires a domain
//! major bump with ADR + migration; *loosening* or adding optional capacity
//! is a minor change. Action-graph limits (128 nodes, depth 16, deadlines)
//! belong to `openstream-engine` (TECHNICAL_SPEC §5) and are out of scope.

/// Maximum UTF-8 byte length for titles, profile names, and control labels.
pub const MAX_TEXT_BYTES: usize = 256;

/// Maximum number of segments in one [`crate::folder::FolderPath`].
pub const MAX_FOLDER_SEGMENTS: usize = 32;

/// Maximum UTF-8 byte length of a single folder-path segment.
pub const MAX_FOLDER_SEGMENT_BYTES: usize = 64;

/// Maximum UTF-8 byte length of a whole serialized folder path.
pub const MAX_FOLDER_PATH_BYTES: usize = 1024;

/// Maximum number of pages per deck.
pub const MAX_PAGES_PER_DECK: usize = 256;

/// Maximum number of controls per page.
pub const MAX_CONTROLS_PER_PAGE: usize = 1024;

/// Maximum number of decks referenced by one profile.
pub const MAX_DECKS_PER_PROFILE: usize = 128;

/// Validates a user-text field (title, profile name, control label): not
/// empty after trimming, at most [`MAX_TEXT_BYTES`] UTF-8 bytes. Returns the
/// typed error naming only the structural field, never the content.
pub fn check_text(field: &'static str, value: &str) -> Result<(), crate::error::DomainError> {
    use crate::error::DomainError;
    if value.trim().is_empty() || value.len() > MAX_TEXT_BYTES {
        return Err(DomainError::TextFieldOutOfRange { field });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{MAX_TEXT_BYTES, check_text};

    #[test]
    fn text_boundaries() {
        assert!(check_text("title", "ok").is_ok());
        assert!(check_text("title", "").is_err());
        assert!(check_text("title", "   ").is_err());
        assert_eq!(check_text("title", &"x".repeat(MAX_TEXT_BYTES)), Ok(()));
        assert!(check_text("title", &"x".repeat(MAX_TEXT_BYTES + 1)).is_err());
        // Multi-byte characters count bytes, not chars.
        let three_byte_chars = "\u{2713}".repeat(MAX_TEXT_BYTES / 3 + 1);
        assert!(three_byte_chars.len() > MAX_TEXT_BYTES);
        assert!(check_text("label", &three_byte_chars).is_err());
    }
}
