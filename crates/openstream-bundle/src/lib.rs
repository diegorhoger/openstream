//! `openstream-bundle` — versioned `.openstream` import/export bundles
//! (issue #20, `PORTABILITY_BUNDLES.md`).
//!
//! A bundle is a portable whole-workspace snapshot: versioned manifest,
//! per-member SHA-256 content hashes, and the exact deterministic document
//! JSON produced by `openstream-domain`. The crate owns three guarantees:
//!
//! 1. **Fail-closed import.** Framing magic and container version, member
//!    count/name/size caps, closed-vocabulary member names (the structural
//!    path-traversal defense — there is no free-form path surface at all),
//!    decompression ratio guards (asset-bomb defense), manifest/member
//!    bijection with hash verification, domain schema decoding, and
//!    workspace semantics all reject before anything is returned. Malformed
//!    or hostile input never yields partial content.
//! 2. **Exact round-trip.** Building is deterministic: canonical member
//!    order, stored (uncompressed) members only, compact declaration-order
//!    JSON. Export → import → export is byte-identical on one build;
//!    across builds the documented rule is semantic identity of decoded
//!    documents (`PORTABILITY_BUNDLES.md` §8).
//! 3. **No secret material, ever.** Bundles carry `DeckDocument` /
//!    `ProfileDocument` values only. Those domain types have no
//!    secret-bearing fields; [`openstream_domain::secret::SecretValue`]
//!    cannot serialize at all (TM-LOG-01), so vault-backed secrets are
//!    structurally absent from every byte a bundle can contain — proven by
//!    raw-byte scan tests in this crate's integration suite.
//!
//! Restore itself is the documented atomic procedure over
//! `openstream_persistence::sqlite::WorkspaceStore::rewrite_all`: parse and
//! validate fully first, then replace the store inside its single
//! transaction. Any earlier failure leaves the previous workspace state
//! untouched; the proof test lives beside the implementation contract in
//! `PORTABILITY_BUNDLES.md` §10.

mod bundle;
mod error;
mod file;
mod frame;
mod manifest;
mod member;

/// v1 size and shape limits enforced on every bundle.
pub mod limits;

pub use bundle::{
    BUNDLE_MANIFEST_MAJOR, BUNDLE_MANIFEST_MINOR, ParsedBundle, build_bundle, parse_bundle,
    validate_workspace,
};
pub use error::{BundleError, ManifestVersion};
pub use file::{read_bundle_file, write_bundle_file};

#[cfg(test)]
mod tests {
    use super::limits::BUNDLE_FORMAT_VERSION;
    use super::{error::ManifestVersion, limits::MAGIC};

    #[test]
    fn manifest_version_support_is_one_zero() {
        assert_eq!(
            ManifestVersion::supported(),
            ManifestVersion { major: 1, minor: 0 }
        );
        assert!(ManifestVersion { major: 1, minor: 0 }.is_readable());
        assert!(!ManifestVersion { major: 1, minor: 1 }.is_readable());
        assert!(!ManifestVersion { major: 2, minor: 0 }.is_readable());
        assert!(!ManifestVersion { major: 0, minor: 9 }.is_readable());
    }

    #[test]
    fn container_version_is_pinned() {
        assert_eq!(BUNDLE_FORMAT_VERSION, 1);
        assert_eq!(&MAGIC, b"OSTRBNDL");
    }
}
