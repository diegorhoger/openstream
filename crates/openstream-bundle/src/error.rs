//! Typed bundle errors (`PORTABILITY_BUNDLES.md` §7).
//!
//! Every failure is a typed, matchable value and fails closed; nothing fails
//! silently. Error payloads carry only structural facts (reason codes,
//! limits, grammar-valid member names). Rejected hostile input is never
//! echoed back, and no variant ever carries secret material — bundles
//! cannot contain any (see `crate::lib` docs), so there is nothing to leak.

use core::fmt;

/// Typed validation or integrity failure for one `.openstream` bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BundleError {
    /// The input exceeded a declared size cap before any content was
    /// interpreted. `what` names the bound structurally.
    TooLarge {
        /// Which cap was hit (bundle file, member count, ...).
        what: &'static str,
        /// The enforced maximum in bytes or entries.
        limit: usize,
    },
    /// The container does not start with the OpenStream bundle magic.
    InvalidMagic,
    /// The container framing version is not the one this build reads.
    /// Fail closed: foreign formats are never "best-effort" interpreted.
    UnsupportedContainerVersion {
        /// Framing version found in the header.
        found: u32,
        /// Framing version this build reads.
        supported: u32,
    },
    /// The binary frame is truncated, self-inconsistent, or carries bytes
    /// after the last declared member. The reason is structural only;
    /// hostile content is never echoed.
    MalformedFrame {
        /// Structural reason code (short name, short lengths, trailing
        /// bytes, decompression failure, length lies).
        reason: &'static str,
    },
    /// A member name is outside the closed vocabulary
    /// (`manifest.json`, `deck/<uuid>.json`, `profile/<uuid>.json`).
    /// This single gate makes path traversal structurally impossible:
    /// no other separator, parent component, drive letter, or absolute
    /// form can pass.
    IllegalMemberName {
        /// Structural reason code, never the rejected text.
        reason: &'static str,
    },
    /// Two framed members share one name.
    DuplicateMember,
    /// A deflated member declares an expansion beyond
    /// [`crate::limits::MAX_DECOMPRESSION_RATIO`] relative to its stored
    /// size. Refused before decompression starts.
    CompressionRatioExceeded {
        /// The enforced maximum ratio.
        max_ratio: u64,
    },
    /// The manifest member is not exactly the v1 manifest JSON schema
    /// (bad JSON, unknown member, wrong types). Carries the structural
    /// decoder message only.
    ManifestDecode {
        /// Structural decoder message (no user content).
        detail: String,
    },
    /// The manifest declares a bundle schema version this build must not
    /// interpret (foreign major, or minor newer than supported). Fail
    /// closed, mirroring the domain-model rule (DOMAIN_MODEL.md §1).
    UnsupportedManifestVersion {
        /// Version found in the manifest.
        found: ManifestVersion,
        /// Highest version this build can read.
        supported: ManifestVersion,
    },
    /// The manifest parsed but contradicts the frame it travels with:
    /// counts wrong, entries unsorted/duplicated, missing or extra members,
    /// entry/member bijection violated. Reason is structural only.
    ManifestInconsistent {
        /// Structural reason code.
        reason: &'static str,
    },
    /// A member's recomputed SHA-256 digest differs from the hash recorded
    /// in the manifest. The name is echoed only because it already passed
    /// the closed-vocabulary grammar.
    HashMismatch {
        /// Grammar-valid member name whose digest did not match.
        name: String,
    },
    /// An enclosed deck/profile document failed domain decoding or
    /// save-time validation (unknown schema version, unknown fields,
    /// broken invariants). The wrapped [`openstream_domain::error::DomainError`]
    /// follows the taxonomy redaction rules on its own.
    Document(openstream_domain::error::DomainError),
    /// Documents inside one bundle disagree about their owning workspace.
    WorkspaceMismatch,
    /// A profile references a deck that is not part of the same bundle.
    /// Restore replaces the whole workspace snapshot, so dangling
    /// references would be unresolvable afterwards and fail closed here.
    MissingDeckReference,
    /// Cross-profile switch-trigger conflict among bundled profiles; the
    /// restored board would run degraded, so import refuses instead
    /// (same rule as authoring time in issue #19).
    ConflictingSwitchRules(
        /// Structural mechanism token of the collided trigger class.
        &'static str,
    ),
    /// Filesystem IO failed during the durable file helpers. Mapped to a
    /// bare token without OS messages so absolute paths never enter logs
    /// (same posture as `openstream-persistence`'s typed IO refusals).
    IoFailed {
        /// Structural operation that failed (read, write, rename, sync).
        stage: &'static str,
    },
}

/// Explicit bundle-manifest schema version (`major.minor`), decoded
/// fail-closed like the domain model's [`openstream_domain::version::SchemaVersion`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManifestVersion {
    /// Bundle-manifest major; foreign majors reject.
    pub major: u32,
    /// Additive-evolution counter inside one major.
    pub minor: u32,
}

impl ManifestVersion {
    /// Highest manifest version this build can read.
    #[must_use]
    pub const fn supported() -> Self {
        Self {
            major: crate::BUNDLE_MANIFEST_MAJOR,
            minor: crate::BUNDLE_MANIFEST_MINOR,
        }
    }

    /// Fail-closed readability check: foreign majors and minors newer than
    /// supported reject.
    #[must_use]
    pub const fn is_readable(&self) -> bool {
        self.major == crate::BUNDLE_MANIFEST_MAJOR
            && (self.minor as u64) <= (crate::BUNDLE_MANIFEST_MINOR as u64)
    }
}

impl fmt::Display for BundleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { what, limit } => {
                write!(f, "bundle rejected: {what} exceeds limit {limit}")
            }
            Self::InvalidMagic => f.write_str("bundle rejected: invalid magic"),
            Self::UnsupportedContainerVersion { found, supported } => {
                write!(
                    f,
                    "bundle rejected: container version {found} unsupported (supported {supported})"
                )
            }
            Self::MalformedFrame { reason } => {
                write!(f, "bundle rejected: malformed frame ({reason})")
            }
            Self::IllegalMemberName { reason } => {
                write!(f, "bundle rejected: illegal member name ({reason})")
            }
            Self::DuplicateMember => f.write_str("bundle rejected: duplicate member"),
            Self::CompressionRatioExceeded { max_ratio } => {
                write!(f, "bundle rejected: decompression ratio above {max_ratio}")
            }
            Self::ManifestDecode { detail } => {
                write!(f, "bundle rejected: manifest decode ({detail})")
            }
            Self::UnsupportedManifestVersion { found, supported } => write!(
                f,
                "bundle rejected: manifest version {}.{} unsupported (supported {}.{})",
                found.major, found.minor, supported.major, supported.minor
            ),
            Self::ManifestInconsistent { reason } => {
                write!(f, "bundle rejected: inconsistent manifest ({reason})")
            }
            Self::HashMismatch { name } => write!(f, "bundle rejected: hash mismatch for {name}"),
            Self::Document(error) => write!(f, "bundle rejected by document validation: {error}"),
            Self::WorkspaceMismatch => {
                f.write_str("bundle rejected: documents span multiple workspaces")
            }
            Self::MissingDeckReference => {
                f.write_str("bundle rejected: profile references a deck outside the bundle")
            }
            Self::ConflictingSwitchRules(kind) => {
                write!(
                    f,
                    "bundle rejected: conflicting {kind} switch rules across profiles"
                )
            }
            Self::IoFailed { stage } => write!(f, "bundle file operation failed at stage {stage}"),
        }
    }
}

impl std::error::Error for BundleError {}

impl From<openstream_domain::error::DomainError> for BundleError {
    fn from(error: openstream_domain::error::DomainError) -> Self {
        Self::Document(error)
    }
}

/// Maps an IO result into the path-free [`BundleError::IoFailed`].
pub(crate) fn io_failed<R>(
    stage: &'static str,
    result: std::io::Result<R>,
) -> Result<R, BundleError> {
    result.map_err(|_| BundleError::IoFailed { stage })
}
