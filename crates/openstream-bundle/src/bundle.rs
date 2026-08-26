//! Bundle construction and fail-closed parsing (`PORTABILITY_BUNDLES.md` §5).
//!
//! A v1 bundle is a whole-workspace snapshot: every deck and profile
//! document, the manifest first, members canonically ordered. Building
//! validates everything before a single byte is written; parsing verifies
//! framing, hashes, manifest consistency, domain decoding, and workspace
//! semantics before returning anything, so a caller that receives
//! [`ParsedBundle`] holds documents that are individually valid, mutually
//! coherent, and byte-exact against their recorded hashes.

use crate::error::{BundleError, ManifestVersion};
use crate::frame;
use crate::limits::{MAX_MEMBER_COUNT, MAX_MEMBER_UNCOMPRESSED_BYTES};
use crate::manifest::{self, ManifestEntry};
use crate::member::{MANIFEST_NAME, MemberName};
use openstream_domain::document::{DeckDocument, ProfileDocument};
use openstream_domain::error::DomainError;
use openstream_domain::ids::WorkspaceId;
use openstream_domain::profile::Profile;
use openstream_domain::switching::SwitchBoard;

/// Highest bundle-manifest version this build writes and reads
/// (`major` half of the fail-closed readability rule).
pub const BUNDLE_MANIFEST_MAJOR: u32 = 1;

/// Additive-evolution counter of the bundle-manifest schema (`minor` half).
pub const BUNDLE_MANIFEST_MINOR: u32 = 0;

/// The decoded contents of one validated bundle.
#[derive(Debug, Clone)]
pub struct ParsedBundle {
    /// Manifest schema version carried by the bundle.
    pub manifest_version: ManifestVersion,
    /// Decks in canonical id order.
    pub decks: Vec<DeckDocument>,
    /// Profiles in canonical id order.
    pub profiles: Vec<ProfileDocument>,
}

/// Builds a deterministic `.openstream` bundle from a whole-workspace
/// snapshot.
///
/// Every document is validated (save-time rules), cross-document semantics
/// are checked ([`validate_workspace`]), and members are emitted in
/// canonical order — manifest first, then decks by id, then profiles by
/// id — with SHA-256 digests recorded per member. The same input therefore
/// always produces byte-identical output on this build; see
/// `PORTABILITY_BUNDLES.md` §8 for the round-trip rule across builds.
///
/// # Errors
/// [`BundleError`] for any invalid document, semantic break, or size cap.
pub fn build_bundle(
    decks: &[DeckDocument],
    profiles: &[ProfileDocument],
) -> Result<Vec<u8>, BundleError> {
    validate_workspace(decks, profiles)?;

    let mut entries: Vec<ManifestEntry> = Vec::new();
    let mut members: Vec<(String, Vec<u8>)> = Vec::new();

    for document in decks {
        let name = MemberName::Deck(document.deck.id).as_str();
        record_document(&mut members, &mut entries, name, document)?;
    }
    for document in profiles {
        let name = MemberName::Profile(document.profile.id).as_str();
        record_document(&mut members, &mut entries, name, document)?;
    }

    // +1 reserves the manifest's own frame slot under the member cap.
    if members.len() + 1 > MAX_MEMBER_COUNT {
        return Err(BundleError::TooLarge {
            what: "member count",
            limit: MAX_MEMBER_COUNT,
        });
    }
    let manifest_raw = manifest::write_manifest(
        ManifestVersion {
            major: BUNDLE_MANIFEST_MAJOR,
            minor: BUNDLE_MANIFEST_MINOR,
        },
        entries,
    )?;
    members.insert(0, (MANIFEST_NAME.to_string(), manifest_raw));
    Ok(frame::write_frame(&members))
}

/// Parses and fully validates a serialized `.openstream` bundle.
///
/// Order of defenses: file cap → magic → container version → member caps →
/// closed-vocabulary names → decompression ratio guard → exact-length
/// verification → trailing-byte rejection → manifest decode → manifest/
/// member bijection with per-member hash verification → domain decoding of
/// every document → workspace semantic validation. Nothing is returned on
/// any failure, so a [`ParsedBundle`] is safe to restore atomically.
///
/// # Errors
/// [`BundleError`] describing the first defense that rejected.
pub fn parse_bundle(bytes: &[u8]) -> Result<ParsedBundle, BundleError> {
    let framed = frame::read_frame(bytes)?;

    let mut manifest_count = 0usize;
    let mut manifest_at = 0usize;
    for (index, member) in framed.iter().enumerate() {
        if MemberName::parse(&member.name)?.is_manifest() {
            manifest_count += 1;
            manifest_at = index;
        }
    }
    match (manifest_count, manifest_at) {
        (0, _) => {
            return Err(BundleError::MalformedFrame {
                reason: "manifest member missing",
            });
        }
        (1, 0) => {}
        (1, _) => {
            return Err(BundleError::MalformedFrame {
                reason: "manifest must be the first member",
            });
        }
        (_, _) => return Err(BundleError::DuplicateMember),
    }
    let parsed_manifest = manifest::parse_manifest(&framed[manifest_at].raw)?;

    // Bijection: exactly one framed payload per manifest entry, no extras,
    // each matching its recorded digest. Both sides arrive canonically
    // sorted by name, so one aligned walk proves the correspondence.
    if framed.len() != parsed_manifest.entries.len() + 1 {
        return Err(inconsistent("entry count does not match members"));
    }
    let mut payloads: Vec<(MemberName, &[u8])> = Vec::with_capacity(framed.len());
    for member in &framed {
        match MemberName::parse(&member.name)? {
            MemberName::Manifest => {}
            typed => payloads.push((typed, &member.raw)),
        }
    }
    payloads.sort_by_key(|left| left.0.as_str());
    for ((name, raw), entry) in payloads.iter().zip(&parsed_manifest.entries) {
        if name.as_str() != entry.name {
            return Err(inconsistent("entries do not match members"));
        }
        if manifest::sha256_hex(raw) != entry.sha256_hex {
            return Err(BundleError::HashMismatch {
                name: entry.name.clone(),
            });
        }
    }

    let mut decks = Vec::new();
    let mut profiles = Vec::new();
    for (name, raw) in payloads {
        match name {
            MemberName::Deck(id) => {
                let json = std::str::from_utf8(raw).map_err(|_| DomainError::Encoding {
                    detail: "member is not utf-8".into(),
                })?;
                let document = DeckDocument::from_json_str(json).map_err(BundleError::Document)?;
                if document.deck.id != id {
                    return Err(inconsistent("document id disagrees with member name"));
                }
                decks.push(document);
            }
            MemberName::Profile(id) => {
                let json = std::str::from_utf8(raw).map_err(|_| DomainError::Encoding {
                    detail: "member is not utf-8".into(),
                })?;
                let document =
                    ProfileDocument::from_json_str(json).map_err(BundleError::Document)?;
                if document.profile.id != id {
                    return Err(inconsistent("document id disagrees with member name"));
                }
                profiles.push(document);
            }
            MemberName::Manifest => unreachable!("manifests are filtered above"),
        }
    }

    validate_workspace(&decks, &profiles)?;
    Ok(ParsedBundle {
        manifest_version: parsed_manifest.version,
        decks,
        profiles,
    })
}

/// Cross-document semantic validation shared by builder and parser:
/// uniform workspace ownership, deck-reference closure, and switch-rule
/// conflict freedom (the same board-construction rule authoring applies).
/// An empty snapshot passes: restoring it reproduces an empty workspace,
/// which is what was exported.
///
/// # Errors
/// [`BundleError::WorkspaceMismatch`], [`BundleError::MissingDeckReference`],
/// [`BundleError::ConflictingSwitchRules`], or [`BundleError::Document`].
pub fn validate_workspace(
    decks: &[DeckDocument],
    profiles: &[ProfileDocument],
) -> Result<(), BundleError> {
    let owners = decks
        .iter()
        .map(|d| d.deck.workspace_id)
        .chain(profiles.iter().map(|p| p.profile.workspace_id));
    let mut first_owner: Option<WorkspaceId> = None;
    for owner in owners {
        match first_owner {
            Some(existing) if existing != owner => return Err(BundleError::WorkspaceMismatch),
            Some(_) => {}
            None => first_owner = Some(owner),
        }
    }
    let mut deck_ids: Vec<_> = decks.iter().map(|d| d.deck.id).collect();
    deck_ids.sort_unstable();
    for document in profiles {
        for deck_id in &document.profile.deck_ids {
            if deck_ids.binary_search(deck_id).is_err() {
                return Err(BundleError::MissingDeckReference);
            }
        }
    }
    let owned_profiles: Vec<&Profile> = profiles.iter().map(|p| &p.profile).collect();
    SwitchBoard::from_profiles(owned_profiles).map_err(|error| match error {
        DomainError::ConflictingSwitchRule { kind } => BundleError::ConflictingSwitchRules(kind),
        other => BundleError::Document(other),
    })?;
    Ok(())
}

fn record_document<T: serde::Serialize>(
    members: &mut Vec<(String, Vec<u8>)>,
    entries: &mut Vec<ManifestEntry>,
    name: String,
    document: &T,
) -> Result<(), BundleError> {
    let raw = serde_json::to_vec(document).map_err(|error| DomainError::Encoding {
        detail: error.to_string(),
    })?;
    if raw.len() > MAX_MEMBER_UNCOMPRESSED_BYTES {
        return Err(BundleError::TooLarge {
            what: "member raw size",
            limit: MAX_MEMBER_UNCOMPRESSED_BYTES,
        });
    }
    entries.push(ManifestEntry {
        name: name.clone(),
        sha256_hex: manifest::sha256_hex(&raw),
    });
    members.push((name, raw));
    Ok(())
}

fn inconsistent(reason: &'static str) -> BundleError {
    BundleError::ManifestInconsistent { reason }
}
