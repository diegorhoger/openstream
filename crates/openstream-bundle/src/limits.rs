//! v1 size and shape limits enforced by this crate (`PORTABILITY_BUNDLES.md` §6).
//!
//! These constants ARE the v1 bundle contract (`bundle format 1`, manifest
//! `schema_version 1.0`). Per the domain-model versioning policy
//! (DOMAIN_MODEL.md §1, ADR-0005), *tightening* any value here rejects
//! previously valid bundles and therefore requires a bundle major bump with
//! migration documentation; *loosening* is an additive minor. Every bound is
//! enforced BEFORE the corresponding allocation or decompression happens, so
//! a hostile bundle can never make this crate allocate past its caps.

/// Magic bytes every `.openstream` container starts with.
pub const MAGIC: [u8; 8] = *b"OSTRBNDL";

/// Container framing version this build writes. Reading fails closed on any
/// other value (foreign versions are never "best-effort" interpreted).
pub const BUNDLE_FORMAT_VERSION: u32 = 1;

/// Maximum accepted size of one serialized bundle in bytes (the compressed
/// input as handed to [`crate::parse_bundle`]).
pub const MAX_BUNDLE_FILE_BYTES: usize = 64 * 1024 * 1024;

/// Maximum number of framed members inside one bundle (manifest included).
pub const MAX_MEMBER_COUNT: usize = 2048;

/// Maximum byte length of one member name. The longest legal name
/// (`profile/<36 hex-hyphen chars>.json`) is 45 bytes; the cap leaves slack
/// for future closed-vocabulary kinds without admitting free-form paths.
pub const MAX_MEMBER_NAME_BYTES: usize = 128;

/// Maximum uncompressed size of one member in bytes. The largest legal
/// domain document (a deck at every v1 bound) stays far below this cap;
/// exceeding it fails closed rather than allocating.
pub const MAX_MEMBER_UNCOMPRESSED_BYTES: usize = 64 * 1024 * 1024;

/// Maximum summed uncompressed size of all members of one bundle.
pub const MAX_BUNDLE_UNCOMPRESSED_BYTES: usize = 128 * 1024 * 1024;

/// Maximum allowed `raw_len / stored_len` ratio for a deflated member.
/// Checked before decompression starts; legal JSON documents compress far
/// below this ratio, while classic zip-bomb constructions exceed it by
/// orders of magnitude.
pub const MAX_DECOMPRESSION_RATIO: u64 = 100;

/// Stored-form size under which the ratio check is skipped. Below this many
/// compressed bytes even the maximum realizable deflate expansion stays a
/// few megabytes, so the absolute caps already bound every allocation; the
/// exemption exists so legitimately tiny members are never rejected by
/// small-denominator arithmetic.
pub const RATIO_GUARD_FLOOR_BYTES: u64 = 4 * 1024;

#[cfg(test)]
mod tests {
    use super::{
        MAX_BUNDLE_FILE_BYTES, MAX_DECOMPRESSION_RATIO, MAX_MEMBER_COUNT, MAX_MEMBER_NAME_BYTES,
        MAX_MEMBER_UNCOMPRESSED_BYTES,
    };

    #[test]
    fn caps_are_ordered_and_bounded() {
        const {
            assert!(MAX_MEMBER_UNCOMPRESSED_BYTES <= MAX_BUNDLE_FILE_BYTES * 2);
            assert!(MAX_MEMBER_NAME_BYTES >= 45);
            assert!(MAX_DECOMPRESSION_RATIO > 1);
        }
    }

    #[test]
    fn member_count_cap_leaves_room_for_legal_workspaces() {
        // A workspace holds at most its decks plus profiles plus the manifest;
        // the store's own limits keep real snapshots far below the cap.
        const {
            assert!(MAX_MEMBER_COUNT >= 3);
        }
    }
}
