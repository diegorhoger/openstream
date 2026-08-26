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

/// Maximum UTF-8 byte length of a whole serialized capability identifier
/// (grammar per `CAPABILITY_TAXONOMY.md` §1).
pub const MAX_CAPABILITY_BYTES: usize = 1024;

/// Maximum UTF-8 byte length of one qualifier value inside a capability.
pub const MAX_QUALIFIER_VALUE_BYTES: usize = 256;

/// Maximum number of simultaneously active grant records held by the
/// in-memory grant ledger. Persistence arrives with #15; overflowing this
/// bound fails closed instead of dropping evidence.
pub const MAX_ACTIVE_GRANTS: usize = 1024;

/// Maximum number of audit events retained by the in-memory append-only
/// audit log. Overflow fails closed (`AuditLogFull` behavior via
/// [`crate::error::DomainError::LimitExceeded`]); durable persistence is #15.
pub const MAX_AUDIT_EVENTS: usize = 10_000;

/// Maximum UTF-8 byte length of a secret reference (the structural address
/// of one entry in OS credential storage).
pub const MAX_SECRET_REF_BYTES: usize = 128;

/// Maximum UTF-8 byte length of one secret value. Pinned to the tightest
/// supported platform credential blob limit (Windows `CRED_MAX_CREDENTIAL_
/// BLOB_SIZE`, 5 * 512 bytes) so every backend shares one contract; larger
/// values reject fail closed instead of silently succeeding per platform.
pub const MAX_SECRET_VALUE_BYTES: usize = 2560;

/// Maximum number of switch rules carried by one profile (issue #19).
pub const MAX_SWITCH_RULES_PER_PROFILE: usize = 32;

/// Maximum UTF-8 byte length of one focused-app identity token
/// ([`crate::switching::AppIdentity`], issue #19).
pub const MAX_APP_IDENTITY_BYTES: usize = 64;

/// Maximum number of tokens inside one serialized hotkey combination
/// (modifiers plus key; issue #19). Four modifiers + one key.
pub const MAX_HOTKEY_TOKENS: usize = 5;

/// Maximum number of diagnostic log entries retained in the in-memory
/// structured log ring buffer. Overflow drops oldest entries (tail eviction,
/// never fails closed — diagnostics are best-effort, not evidence).
pub const MAX_DIAGNOSTIC_LOG_ENTRIES: usize = 8_192;

/// Maximum UTF-8 byte length of one diagnostic log message.
pub const MAX_DIAGNOSTIC_MESSAGE_BYTES: usize = 1_024;

/// Maximum number of crash reports retained on disk. Older reports are
/// pruned when the bound is reached (newest-first).
pub const MAX_CRASH_REPORTS: usize = 64;

/// Default telemetry consent is OFF; revocable at any time (taxonomy §3
/// consent column, hard-stop: consent is never implicit).
pub const TELEMETRY_CONSENT_DEFAULT: bool = false;

/// Maximum number of rate-limiter buckets tracked simultaneously.
pub const MAX_RATE_LIMITER_BUCKETS: usize = 256;

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
