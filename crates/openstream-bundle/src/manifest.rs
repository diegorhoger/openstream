//! The versioned bundle manifest (`PORTABILITY_BUNDLES.md` §5).
//!
//! `manifest.json` is the first member of every v1 bundle and pins the
//! content contract:
//!
//! ```json
//! {
//!   "schema_version": { "major": 1, "minor": 0 },
//!   "counts": { "decks": 1, "profiles": 1 },
//!   "entries": [
//!     { "name": "deck/<uuid>.json", "sha256": "<64 lowercase hex>" }
//!   ]
//! }
//! ```
//!
//! Decoding is fail closed on the same terms as the domain documents:
//! unknown members reject, foreign manifest majors and minors newer than
//! supported reject ([`BundleError::UnsupportedManifestVersion`]), and
//! hashes are lowercase hex SHA-256 digests of the exact member bytes.
//! Entries are canonically sorted by name; the builder writes them that way
//! and the reader refuses anything else, so manifests are byte-stable.

use crate::error::{BundleError, ManifestVersion};
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// One manifest entry: a member name plus the SHA-256 of its exact bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestEntry {
    /// Closed-vocabulary member name.
    pub name: String,
    /// Lowercase hex SHA-256 digest (64 characters).
    pub sha256_hex: String,
}

/// Decoded and structurally validated manifest.
#[derive(Debug, Clone)]
pub(crate) struct Manifest {
    /// Declared schema version.
    pub version: ManifestVersion,
    /// Validated entries in canonical (sorted, unique) order.
    pub entries: Vec<ManifestEntry>,
}

/// Raw serde shape; every deviation rejects during decode. The same shape
/// doubles as the writer form, keeping reader and writer byte-compatible.
#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    schema_version: RawVersion,
    counts: RawCounts,
    entries: Vec<RawEntry>,
}

#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct RawVersion {
    major: u32,
    minor: u32,
}

#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct RawCounts {
    decks: u32,
    profiles: u32,
}

#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct RawEntry {
    name: String,
    sha256: String,
}

/// Parses and validates the manifest member bytes. Structural checks here
/// cover version readability, canonical ordering/uniqueness, hash spelling,
/// count agreement with the deck/profile name grammar, and non-emptiness of
/// names. Cross-checks against the actual framed members happen in
/// [`crate::bundle::parse_bundle`], which owns the bijection proof.
pub(crate) fn parse_manifest(bytes: &[u8]) -> Result<Manifest, BundleError> {
    let raw: RawManifest =
        serde_json::from_slice(bytes).map_err(|error| BundleError::ManifestDecode {
            detail: error.to_string(),
        })?;
    let version = ManifestVersion {
        major: raw.schema_version.major,
        minor: raw.schema_version.minor,
    };
    if !version.is_readable() {
        return Err(BundleError::UnsupportedManifestVersion {
            found: version,
            supported: ManifestVersion::supported(),
        });
    }
    let mut previous: Option<&str> = None;
    let mut decks = 0_u64;
    let mut profiles = 0_u64;
    let mut entries = Vec::with_capacity(raw.entries.len());
    for entry in &raw.entries {
        let name = crate::member::MemberName::parse(&entry.name)?;
        if name.is_manifest() {
            return Err(inconsistent("manifest lists itself"));
        }
        if let Some(prev) = previous {
            if entry.name.as_str() <= prev {
                return Err(inconsistent("entries not strictly ascending"));
            }
        }
        previous = Some(entry.name.as_str());
        validate_hash_spelling(&entry.sha256)?;
        match name {
            crate::member::MemberName::Deck(_) => decks += 1,
            crate::member::MemberName::Profile(_) => profiles += 1,
            crate::member::MemberName::Manifest => unreachable!("rejected above"),
        }
        entries.push(ManifestEntry {
            name: entry.name.clone(),
            sha256_hex: entry.sha256.clone(),
        });
    }
    // Count fields must describe exactly what the entry list carries.
    if decks != u64::from(raw.counts.decks) || profiles != u64::from(raw.counts.profiles) {
        return Err(inconsistent("counts disagree with entries"));
    }
    Ok(Manifest { version, entries })
}

/// Serializes a validated manifest deterministically: entries sorted,
/// compact JSON, declaration-order fields.
pub(crate) fn write_manifest(
    version: ManifestVersion,
    mut entries: Vec<ManifestEntry>,
) -> Result<Vec<u8>, BundleError> {
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    for pair in entries.windows(2) {
        if pair[0].name == pair[1].name {
            return Err(inconsistent("duplicate entry name"));
        }
    }
    let decks = entries
        .iter()
        .filter(|e| e.name.starts_with("deck/"))
        .count();
    let profiles = entries
        .iter()
        .filter(|e| e.name.starts_with("profile/"))
        .count();
    let raw = RawManifest {
        schema_version: RawVersion {
            major: version.major,
            minor: version.minor,
        },
        counts: RawCounts {
            decks: u32::try_from(decks).map_err(|_| inconsistent("deck count overflow"))?,
            profiles: u32::try_from(profiles)
                .map_err(|_| inconsistent("profile count overflow"))?,
        },
        entries: entries
            .into_iter()
            .map(|entry| RawEntry {
                name: entry.name,
                sha256: entry.sha256_hex,
            })
            .collect(),
    };
    serde_json::to_vec(&raw).map_err(|error| BundleError::ManifestDecode {
        detail: error.to_string(),
    })
}

/// SHA-256 over `bytes`, lowercase hex encoded.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    hex_lower(&digest)
}

fn validate_hash_spelling(hex: &str) -> Result<(), BundleError> {
    if hex.len() == 64
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(inconsistent("hash must be 64 lowercase hex chars"))
    }
}

pub(crate) fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn inconsistent(reason: &'static str) -> BundleError {
    BundleError::ManifestInconsistent { reason }
}
