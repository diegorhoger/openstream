//! Closed-vocabulary member names (`PORTABILITY_BUNDLES.md` §4).
//!
//! A v1 bundle contains exactly three kinds of members:
//!
//! - `manifest.json`
//! - `deck/<canonical-uuidv7>.json`
//! - `profile/<canonical-uuidv7>.json`
//!
//! There is no free-form path surface at all. Every name is checked against
//! this closed grammar before any byte of its payload is interpreted, which
//! is the structural path-traversal defense: separators other than the one
//! kind slash, parent components (`..`), absolute forms, drive letters,
//! backslashes, uppercase spellings, and non-ASCII lookalikes all reject as
//! [`crate::BundleError::IllegalMemberName`] before anything is read.

use crate::error::BundleError;
use openstream_domain::ids::{DeckId, ProfileId};
use std::str::FromStr as _;

/// The manifest member's fixed name.
pub const MANIFEST_NAME: &str = "manifest.json";

/// One validated member name: either the manifest or a typed document
/// entry. Constructed only through [`MemberName::parse`]; every variant
/// carries a canonical identifier that round-trips to the exact name bytes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum MemberName {
    /// The bundle manifest.
    Manifest,
    /// One versioned deck document.
    Deck(DeckId),
    /// One versioned profile document.
    Profile(ProfileId),
}

impl MemberName {
    /// Validates `raw` against the closed grammar and returns the typed
    /// name. Fails closed with [`BundleError::IllegalMemberName`] for every
    /// deviation; the reason code is structural and never echoes input.
    pub fn parse(raw: &str) -> Result<Self, BundleError> {
        if raw.len() > crate::limits::MAX_MEMBER_NAME_BYTES {
            return Err(name_error("name too long"));
        }
        // Reject any byte outside printable ASCII before splitting so
        // control characters, Unicode separators/lookalikes, and NULs can
        // never smuggle through segment matching.
        if raw.bytes().any(|byte| !(0x21..=0x7e).contains(&byte)) {
            return Err(name_error("non-printable-ascii byte"));
        }
        if raw == MANIFEST_NAME {
            return Ok(Self::Manifest);
        }
        let Some((kind, rest)) = raw.split_once('/') else {
            return Err(name_error("missing kind prefix"));
        };
        let Some(id_text) = rest.strip_suffix(".json") else {
            return Err(name_error("missing .json suffix"));
        };
        match kind {
            "deck" => {
                let id = DeckId::from_str(id_text).map_err(|_| name_error("invalid deck id"))?;
                Ok(Self::Deck(id))
            }
            "profile" => {
                let id =
                    ProfileId::from_str(id_text).map_err(|_| name_error("invalid profile id"))?;
                Ok(Self::Profile(id))
            }
            _ => Err(name_error("unknown kind prefix")),
        }
    }

    /// The exact canonical spelling this name serializes to.
    #[must_use]
    pub fn as_str(&self) -> String {
        match self {
            Self::Manifest => MANIFEST_NAME.to_string(),
            Self::Deck(id) => format!("deck/{id}.json"),
            Self::Profile(id) => format!("profile/{id}.json"),
        }
    }

    /// Whether this is the manifest member.
    #[must_use]
    pub const fn is_manifest(&self) -> bool {
        matches!(self, Self::Manifest)
    }
}

fn name_error(reason: &'static str) -> BundleError {
    BundleError::IllegalMemberName { reason }
}

#[cfg(test)]
mod tests {
    use super::{MANIFEST_NAME, MemberName};
    use crate::error::BundleError;
    use openstream_domain::ids::{DeckId, ProfileId};
    use std::str::FromStr as _;

    const DECK_UUID: &str = "018f6a1c-7b21-7002-9f31-000000000002";
    const PROFILE_UUID: &str = "018f6a1c-7b21-7001-9f31-000000000001";

    fn assert_rejected(raw: &str) {
        match MemberName::parse(raw) {
            Err(BundleError::IllegalMemberName { .. }) => {}
            other => panic!("{raw:?} must be rejected, got {other:?}"),
        }
    }

    #[test]
    fn legal_names_parse_and_round_trip() {
        let manifest = MemberName::parse(MANIFEST_NAME).unwrap();
        assert!(manifest.is_manifest());
        assert_eq!(manifest.as_str(), MANIFEST_NAME);

        let deck = MemberName::parse(&format!("deck/{DECK_UUID}.json")).unwrap();
        assert_eq!(deck, MemberName::Deck(DeckId::from_str(DECK_UUID).unwrap()));
        assert_eq!(deck.as_str(), format!("deck/{DECK_UUID}.json"));

        let profile = MemberName::parse(&format!("profile/{PROFILE_UUID}.json")).unwrap();
        assert_eq!(
            profile,
            MemberName::Profile(ProfileId::from_str(PROFILE_UUID).unwrap())
        );
        assert_eq!(profile.as_str(), format!("profile/{PROFILE_UUID}.json"));
    }

    #[test]
    fn traversal_shapes_reject() {
        for hostile in [
            "../escape.json",
            "../../etc/passwd",
            "deck/../../evil.json",
            "deck/../profile/x.json",
            "/absolute/deck.json",
            "//double-slash.json",
            "deck//<id>.json",
            "./relative.json",
            "C:/deck/x.json",
            "deck\\backslash.json",
            "\\\\server\\share\\x.json",
            "manifest.json/",
            "manifest.json/../deck.json",
            "deck/",
            ".json",
            "",
            " ",
            "other/<id>.json",
            "Manifest.json",
            "DECK/<id>.json",
            "deck/.json",
            "deck/x.JSON",
        ] {
            assert_rejected(hostile);
        }
    }

    #[test]
    fn non_canonical_or_non_ascii_ids_reject() {
        // Uppercase spelling is not canonical UUIDv7 text.
        assert_rejected(&format!("deck/{}.json", DECK_UUID.to_uppercase()));
        // Version/variant nibbles wrong (not a v7 uuid).
        assert_rejected("deck/00000000-0000-1000-8000-000000000002.json");
        assert_rejected("deck/not-a-uuid.json");
        assert_rejected("deck/.json");
        // Unicode homoglyphs / fullwidth slash are not ASCII.
        assert_rejected("deck/\u{ff10}186f6a1c.json");
        assert_rejected("deck/a\u{2044}b.json");
        // Control bytes and spaces.
        assert_rejected("deck/a\u{0}b.json");
        assert_rejected("deck/a b.json");
        assert_rejected("deck/a\nb.json");
    }

    #[test]
    fn oversized_names_reject() {
        let long = format!(
            "deck/{}.json",
            "a".repeat(crate::limits::MAX_MEMBER_NAME_BYTES)
        );
        assert_rejected(&long);
    }
}
