//! Typed outcomes, failure policies, and failure reasons.
//!
//! Terminal states are exactly the seven authoritative journal states of
//! `PROTOCOL.md` (via `openstream_domain::audit::ExecutionState`); this
//! module adds the typed *reasons* a `failed` terminal carries and the
//! graph-level failure policy vocabulary of `TECHNICAL_SPEC` §5. Reason
//! tokens align with the v1 error registry (`OSCP_MESSAGES.md` §9) where a
//! registry key exists; engine-scoped situations without a registry row use
//! documented engine tokens and map onto wire codes at the M2 codec
//! boundary.

use core::fmt;

/// Graph-level failure policy (`DOMAIN_MODEL.md` §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FailurePolicy {
    /// First failure aborts the execution; pending work is cancelled.
    #[default]
    Stop,
    /// Failures are recorded but remaining reachable work still runs; the
    /// terminal state is `failed` when any node failed.
    Continue,
    /// On first failure, succeeded effects are compensated in reverse
    /// completion order before the execution fails. Valid only where every
    /// action's adapter declares safe compensation (validated at build).
    Compensate,
}

/// Typed reason carried by a `failed` terminal state and by node-level
/// failures. Variants carry structural data only — adapter codes are
/// bounded identifier strings chosen by the adapter author, never free
/// text payloads.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FailureReason {
    /// Capability intersection denied the effect immediately before
    /// dispatch (taxonomy §2). Carries the typed domain denial reason.
    CapabilityDenied(openstream_domain::grant::DenialReason),
    /// Monotonic deadline elapsed (execution or node override).
    DeadlineExceeded,
    /// The adapter port refused dispatch outright.
    AdapterUnavailable,
    /// The adapter reported an operational failure with a bounded code.
    AdapterFailed {
        /// Adapter-chosen structural code (no free text).
        code: String,
    },
    /// Durable evidence could not be recorded before/around an effect;
    /// the runtime refuses to proceed without evidence.
    JournalWriteRefused,
    /// A variable transform hit a type error (for example `AddInt` on a
    /// non-integer variable) or overflowed the variable bounds.
    TransformFailed,
    /// A compensation effect itself failed while unwinding.
    CompensationFailed {
        /// The failing compensation's adapter code, if any.
        code: Option<String>,
    },
    /// Conservative catch-all for scheduler invariant violations; never
    /// converted into success.
    InternalError,
}

impl FailureReason {
    /// Registry-aligned token for journals and receipts. Tokens without an
    /// `OSCP_MESSAGES.md` §9 row are engine-scoped and documented here;
    /// wire mapping is M2 scope.
    #[must_use]
    pub fn token(&self) -> &str {
        match self {
            Self::CapabilityDenied(_) => "CAPABILITY_DENIED",
            Self::DeadlineExceeded => "DEADLINE_EXCEEDED",
            Self::AdapterUnavailable => "ADAPTER_UNAVAILABLE",
            Self::AdapterFailed { .. } => "ADAPTER_FAILED",
            Self::JournalWriteRefused => "JOURNAL_WRITE_REFUSED",
            Self::TransformFailed => "TRANSFORM_FAILED",
            Self::CompensationFailed { .. } => "COMPENSATION_FAILED",
            Self::InternalError => "INTERNAL_ERROR",
        }
    }
}

impl fmt::Display for FailureReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CapabilityDenied(reason) => write!(f, "CAPABILITY_DENIED({reason})"),
            Self::DeadlineExceeded => f.write_str("DEADLINE_EXCEEDED"),
            Self::AdapterUnavailable => f.write_str("ADAPTER_UNAVAILABLE"),
            Self::AdapterFailed { code } => write!(f, "ADAPTER_FAILED({code})"),
            Self::JournalWriteRefused => f.write_str("JOURNAL_WRITE_REFUSED"),
            Self::TransformFailed => f.write_str("TRANSFORM_FAILED"),
            Self::CompensationFailed { code } => match code {
                Some(code) => write!(f, "COMPENSATION_FAILED({code})"),
                None => f.write_str("COMPENSATION_FAILED"),
            },
            Self::InternalError => f.write_str("INTERNAL_ERROR"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FailurePolicy, FailureReason};
    use openstream_domain::grant::DenialReason;

    #[test]
    fn policy_default_is_stop() {
        assert_eq!(FailurePolicy::default(), FailurePolicy::Stop);
    }

    #[test]
    fn reason_tokens_align_with_registry_rows() {
        assert_eq!(
            FailureReason::CapabilityDenied(DenialReason::NoActiveGrant).token(),
            "CAPABILITY_DENIED"
        );
        assert_eq!(FailureReason::DeadlineExceeded.token(), "DEADLINE_EXCEEDED");
        assert_eq!(
            FailureReason::AdapterUnavailable.token(),
            "ADAPTER_UNAVAILABLE"
        );
        assert_eq!(
            FailureReason::AdapterFailed {
                code: "scene-missing".into()
            }
            .token(),
            "ADAPTER_FAILED"
        );
        assert_eq!(FailureReason::InternalError.token(), "INTERNAL_ERROR");
        assert_eq!(
            FailureReason::JournalWriteRefused.token(),
            "JOURNAL_WRITE_REFUSED"
        );
        let denied = FailureReason::CapabilityDenied(DenialReason::NotRequestedByManifest);
        assert!(denied.to_string().starts_with("CAPABILITY_DENIED("));
        // Adapter codes surface structurally; no payload text beyond them.
        assert_eq!(
            FailureReason::AdapterFailed {
                code: "boom".into()
            }
            .to_string(),
            "ADAPTER_FAILED(boom)"
        );
    }
}
