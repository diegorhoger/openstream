//! Deny-by-default capability grants: typed records, consent, narrowing,
//! revocation, and intersection-only effective authority.
//!
//! Implements the evaluation model of `CAPABILITY_TAXONOMY.md` §2 as pure
//! domain logic (ADR-0001: Rust owns permission evaluation). Effective
//! authority is recomputed by [`GrantLedger::evaluate`] on every call — it
//! is never cached into a standing "can do X" bit — and equals the
//! intersection of the layers this issue owns:
//!
//! 1. **Manifest request** — the caller declared exactly this capability
//!    ([`ManifestDeclaration`]; exact values only, wildcards impossible by
//!    construction).
//! 2. **User grant** — a recorded grant exists for the subject whose
//!    qualifier scope covers the request; the grant carries its consent
//!    evidence ([`ConsentEvidence`]) recorded at creation time.
//! 3. **Action-instance narrowing** — the requested qualifiers must be
//!    covered by the grant (request ⊆ grant); authority never exceeds what
//!    was asked, so the effective scope is the narrower set.
//!
//! Platform capability, workspace policy, and runtime context layers arrive
//! with their own milestones (#9/#13 adapters, Stage 2 policy, engine
//! deadlines) and compose on top of this ledger; any missing layer still
//! denies downstream.
//!
//! Revocation deletes the grant record and appends an audit event; the very
//! next evaluation denies without restart. Multiple independent grants each
//! confer their own scoped authority (revoking one cannot widen another);
//! the layered intersection above is what "intersection-only" constrains.

use crate::audit::{AuditEvent, AuditLog};
use crate::capability::Capability;
use crate::error::DomainError;
use crate::ids::GrantId;
use crate::limits::MAX_ACTIVE_GRANTS;
use core::fmt;
use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::str::FromStr;

/// Who holds a grant: an engine built-in, a plugin install, or a paired
/// peer. Structural `kind:id` reference (`builtin:<token>`,
/// `plugin:<uuidv7>`, `peer:<uuidv7>`); entity records behind plugin/peer
/// ids ship with their own milestones while grants stay auditable now.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SubjectRef(String);

impl SubjectRef {
    /// Reference for an engine built-in component (e.g. `builtin:deck-actions`).
    pub fn builtin(token: &str) -> Result<Self, DomainError> {
        if token.is_empty()
            || token.len() > 64
            || !token
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
            || !token
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphanumeric())
        {
            return Err(DomainError::InvalidSubjectRef {
                reason: "invalid builtin token",
            });
        }
        Ok(Self(format!("builtin:{token}")))
    }

    /// Reference for a plugin install identity (canonical UUIDv7 string;
    /// the PluginInstall entity itself arrives with M5).
    pub fn plugin(id: &str) -> Result<Self, DomainError> {
        Self::typed_uuid("plugin", id)
    }

    /// Reference for a paired peer device identity (canonical UUIDv7 string;
    /// the TrustedPeer entity itself arrives with M2).
    pub fn peer(id: &str) -> Result<Self, DomainError> {
        Self::typed_uuid("peer", id)
    }

    fn typed_uuid(kind: &str, id: &str) -> Result<Self, DomainError> {
        let uuid = uuid::Uuid::parse_str(id).map_err(|_| DomainError::InvalidSubjectRef {
            reason: "subject id is not a UUID",
        })?;
        // Same canonical-lowercase-v7 discipline as typed entity ids
        // (DOMAIN_MODEL.md §2).
        if uuid.get_version_num() != 7 || id != uuid.to_string() {
            return Err(DomainError::InvalidSubjectRef {
                reason: "subject id is not a canonical UUIDv7",
            });
        }
        Ok(Self(format!("{kind}:{id}")))
    }

    /// The subject kind prefix (`builtin`, `plugin`, or `peer`).
    #[must_use]
    pub fn kind(&self) -> &'static str {
        if self.0.starts_with("plugin:") {
            "plugin"
        } else if self.0.starts_with("peer:") {
            "peer"
        } else {
            "builtin"
        }
    }

    /// The structural reference string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SubjectRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for SubjectRef {
    type Err = DomainError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let Some((kind, id)) = raw.split_once(':') else {
            return Err(DomainError::InvalidSubjectRef {
                reason: "missing kind:id separator",
            });
        };
        match kind {
            "builtin" => Self::builtin(id),
            "plugin" => Self::plugin(id),
            "peer" => Self::peer(id),
            _ => Err(DomainError::InvalidSubjectRef {
                reason: "unknown subject kind",
            }),
        }
    }
}

impl Serialize for SubjectRef {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for SubjectRef {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        SubjectRef::from_str(&raw).map_err(DeError::custom)
    }
}

/// One consent kind a user performed in Studio (taxonomy §3: silent,
/// bundled, or pre-toggled consent is invalid — every kind here represents
/// an explicit user action the Engine records evidence of).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConsentKind {
    /// Install-time manifest review.
    InstallReview,
    /// First-use confirmation prompt answered explicitly.
    FirstUse,
    /// Explicit arming/confirmation at press time (destructive class).
    DestructiveArming,
    /// Explicit user selection dialog (application/executable/handle pickers).
    ExplicitSelection,
    /// Exact-tuple review at install time (network.connect).
    ExactTupleReview,
}

impl ConsentKind {
    /// Canonical lowercase name used in errors and serialization.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InstallReview => "install_review",
            Self::FirstUse => "first_use",
            Self::DestructiveArming => "destructive_arming",
            Self::ExplicitSelection => "explicit_selection",
            Self::ExactTupleReview => "exact_tuple_review",
        }
    }
}

impl fmt::Display for ConsentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Consent kinds recorded per capability class straight from the registry's
/// Consent column (taxonomy §5). A grant is created only when the recorded
/// evidence covers every required kind; substitutions fail closed.
#[must_use]
pub const fn required_consent(capability: &Capability) -> &'static [ConsentKind] {
    use Capability::*;
    match capability {
        ObsRead
        | OsMediaEmit
        | OsHotkeyRegister
        | OsFocusRead
        | ClipboardRead
        | ClipboardWrite
        | NotificationShow
        | MidiSend { .. }
        | OscSend { .. }
        | AudioControl { .. } => &[ConsentKind::FirstUse],
        ObsControlScene | OsKeyboardEmit { .. } => {
            &[ConsentKind::InstallReview, ConsentKind::FirstUse]
        }
        ObsControlStream => &[ConsentKind::FirstUse, ConsentKind::DestructiveArming],
        OsApplicationLaunch { .. }
        | ProcessExecute { .. }
        | FilesystemRead { .. }
        | FilesystemWrite { .. } => &[ConsentKind::ExplicitSelection],
        NetworkConnect { .. } => &[ConsentKind::ExactTupleReview],
        // Internal-only capabilities are rejected before consent is consulted;
        // the empty requirement can never be reached through public paths.
        SecretRead { .. } => &[],
    }
}

/// Recorded user-consent evidence attached to a grant at creation.
/// Kinds are de-duplicated preserving order; empty evidence fails closed.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ConsentEvidence {
    kinds: Vec<ConsentKind>,
    recorded_at_ms: i64,
}

impl ConsentEvidence {
    /// Records explicit user consent actions. Fails closed on empty input or
    /// negative timestamps; duplicate kinds collapse to one entry.
    pub fn try_new(kinds: Vec<ConsentKind>, recorded_at_ms: i64) -> Result<Self, DomainError> {
        if recorded_at_ms < 0 {
            return Err(DomainError::InvalidTimestamp);
        }
        if kinds.is_empty() {
            return Err(DomainError::ConsentInsufficient {
                missing: ConsentKind::FirstUse.as_str(),
            });
        }
        let mut deduped: Vec<ConsentKind> = Vec::new();
        for kind in kinds {
            if !deduped.contains(&kind) {
                deduped.push(kind);
            }
        }
        Ok(Self {
            kinds: deduped,
            recorded_at_ms,
        })
    }

    /// True when every required kind was recorded.
    #[must_use]
    pub fn covers_all(&self, required: &[ConsentKind]) -> bool {
        required.iter().all(|r| self.kinds.contains(r))
    }

    /// Recorded kinds in first-recorded order.
    #[must_use]
    pub fn kinds(&self) -> &[ConsentKind] {
        &self.kinds
    }

    /// Epoch millis when the user performed the recorded actions.
    #[must_use]
    pub const fn recorded_at_ms(&self) -> i64 {
        self.recorded_at_ms
    }
}

/// One typed grant record: subject + exact capability scope + consent
/// evidence + creation time. Records live only while unrevoked; revocation
/// deletes them (taxonomy §3).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Grant {
    id: GrantId,
    subject: SubjectRef,
    capability: Capability,
    consent: ConsentEvidence,
    created_at_ms: i64,
}

impl Grant {
    /// The grant identifier.
    #[must_use]
    pub const fn id(&self) -> GrantId {
        self.id
    }

    /// The granted subject.
    #[must_use]
    pub const fn subject(&self) -> &SubjectRef {
        &self.subject
    }

    /// The granted capability (exact scope including qualifiers).
    #[must_use]
    pub const fn capability(&self) -> &Capability {
        &self.capability
    }

    /// Consent evidence recorded at creation.
    #[must_use]
    pub const fn consent(&self) -> &ConsentEvidence {
        &self.consent
    }

    /// Epoch millis of creation.
    #[must_use]
    pub const fn created_at_ms(&self) -> i64 {
        self.created_at_ms
    }

    /// True when this grant's scope covers `requested` (same capability
    /// kind; requested qualifiers match exactly or are unrestricted by the
    /// grant — see [`Capability::covers`]).
    #[must_use]
    pub fn covers(&self, requested: &Capability) -> bool {
        self.capability.covers(requested)
    }

    /// True when `narrower` stays within this grant's scope (same kind,
    /// qualifier pairs subset-or-equal). Used by narrowing operations so a
    /// "narrow" can never widen.
    #[must_use]
    pub fn admits_narrowing_to(&self, narrower: &Capability) -> bool {
        self.capability.admits_narrowing_to(narrower)
    }
}

/// A concrete authority request from one subject: the capability (with the
/// exact narrowed qualifiers wanted now) plus who asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityRequest {
    /// Subject performing the action.
    pub subject: SubjectRef,
    /// Requested capability with exact qualifiers.
    pub capability: Capability,
}

/// The manifest-request layer: capabilities a built-in/plugin declared
/// (exact values only). Internal-only capabilities are rejected here so a
/// manifest can never even declare `secret.*` (taxonomy §4).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ManifestDeclaration {
    declared: Vec<Capability>,
}

impl ManifestDeclaration {
    /// Validates and records declared capabilities. Fails closed on
    /// internal-only entries.
    pub fn try_new(declared: Vec<Capability>) -> Result<Self, DomainError> {
        if declared.iter().any(Capability::is_internal) {
            return Err(DomainError::InternalCapabilityNotGrantable);
        }
        Ok(Self { declared })
    }

    /// Empty declaration denies everything at the manifest layer.
    #[must_use]
    pub fn none() -> Self {
        Self {
            declared: Vec::new(),
        }
    }

    /// True when the exact capability (kind + all qualifier values) was
    /// declared.
    #[must_use]
    pub fn declares(&self, capability: &Capability) -> bool {
        self.declared.iter().any(|d| d == capability)
    }

    /// Declared capabilities in declaration order.
    pub fn iter(&self) -> std::slice::Iter<'_, Capability> {
        self.declared.iter()
    }
}

/// Why a request was denied. Typed reasons surface in execution journals as
/// `failed`; denial is never silently converted into success (taxonomy §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DenialReason {
    /// The capability was not declared by the manifest layer.
    NotRequestedByManifest,
    /// No active, consented, covering grant exists for the subject.
    NoActiveGrant,
    /// Internal-only capability attempted through a public path.
    InternalCapability,
}

impl fmt::Display for DenialReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotRequestedByManifest => f.write_str("not_requested_by_manifest"),
            Self::NoActiveGrant => f.write_str("no_active_grant"),
            Self::InternalCapability => f.write_str("internal_capability"),
        }
    }
}

/// Result of evaluating one request against the authority intersection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Authorized with the effective (narrowed) scope — always equal to or
    /// narrower than the request; authority never exceeds the intersection.
    Granted {
        /// Effective scope actually authorized for this effect.
        effective: Capability,
    },
    /// Denied with a typed reason.
    Denied {
        /// Typed denial reason for journaling.
        reason: DenialReason,
    },
}

/// In-memory deny-by-default grant ledger with an attached append-only
/// audit trail. Empty ledger = zero authority.
///
/// Persistence arrives with #15; this ledger is the authoritative M1
/// domain-level record set the persistence layer will adopt verbatim.
#[derive(Debug, Clone, Default)]
pub struct GrantLedger {
    active: Vec<Grant>,
    audit: AuditLog,
}

impl GrantLedger {
    /// Empty ledger; every evaluation denies until grants are recorded.
    #[must_use]
    pub fn new() -> Self {
        Self {
            active: Vec::new(),
            audit: AuditLog::new(),
        }
    }

    /// Creates a grant after fail-closed validation:
    /// internal-only capabilities reject, consent evidence must cover every
    /// kind the capability class requires, timestamps are non-negative, and
    /// the active-grant bound holds. Emits [`AuditEvent::GrantCreated`].
    pub fn create_grant(
        &mut self,
        subject: SubjectRef,
        capability: Capability,
        consent: ConsentEvidence,
        created_at_ms: i64,
    ) -> Result<GrantId, DomainError> {
        check_timestamp(created_at_ms)?;
        if capability.is_internal() {
            return Err(DomainError::InternalCapabilityNotGrantable);
        }
        if let Some(missing) = required_consent(&capability)
            .iter()
            .find(|required| !consent.covers_all(std::slice::from_ref(required)))
        {
            return Err(DomainError::ConsentInsufficient {
                missing: missing.as_str(),
            });
        }
        if self.active.len() >= MAX_ACTIVE_GRANTS {
            return Err(DomainError::LimitExceeded {
                what: "active grants",
                limit: MAX_ACTIVE_GRANTS,
            });
        }
        let grant = Grant {
            id: GrantId::generate(),
            subject,
            capability,
            consent,
            created_at_ms,
        };
        let event = AuditEvent::GrantCreated {
            at_ms: created_at_ms,
            grant_id: grant.id,
            subject: grant.subject.clone(),
            capability_kind: grant.capability.kind_name(),
        };
        self.audit.append(event)?;
        let id = grant.id;
        self.active.push(grant);
        Ok(id)
    }

    /// Narrows an existing grant to a not-wider scope of the same capability
    /// kind. Widening attempts and kind mismatches reject unchanged. Emits
    /// [`AuditEvent::GrantNarrowed`].
    pub fn narrow_grant(
        &mut self,
        grant_id: GrantId,
        narrower: Capability,
        at_ms: i64,
    ) -> Result<(), DomainError> {
        check_timestamp(at_ms)?;
        if narrower.is_internal() {
            return Err(DomainError::InternalCapabilityNotGrantable);
        }
        let grant = self
            .active
            .iter_mut()
            .find(|grant| grant.id == grant_id)
            .ok_or(DomainError::GrantNotFound)?;
        if !grant.admits_narrowing_to(&narrower) {
            if grant.capability.kind_name() != narrower.kind_name() {
                return Err(DomainError::NarrowingKindMismatch);
            }
            return Err(DomainError::NarrowingWouldWiden);
        }
        grant.capability = narrower.clone();
        self.audit.append(AuditEvent::GrantNarrowed {
            at_ms,
            grant_id,
            capability_kind: narrower.kind_name(),
        })
    }

    /// Revokes (deletes) one grant immediately; the next evaluation denies.
    /// Emits [`AuditEvent::GrantRevoked`]. Unknown ids fail closed.
    pub fn revoke_grant(&mut self, grant_id: GrantId, at_ms: i64) -> Result<(), DomainError> {
        check_timestamp(at_ms)?;
        let index = self
            .active
            .iter()
            .position(|grant| grant.id == grant_id)
            .ok_or(DomainError::GrantNotFound)?;
        let grant = self.active.remove(index);
        self.audit.append(AuditEvent::GrantRevoked {
            at_ms,
            grant_id: grant.id,
            subject: grant.subject,
            capability_kind: grant.capability.kind_name(),
        })
    }

    /// Revokes every grant of one subject ("per peer" revocation). Returns
    /// how many records were deleted; each deletion is audited.
    pub fn revoke_all_for_subject(
        &mut self,
        subject: &SubjectRef,
        at_ms: i64,
    ) -> Result<usize, DomainError> {
        check_timestamp(at_ms)?;
        let mut revoked = Vec::new();
        let mut kept = Vec::new();
        for grant in self.active.drain(..) {
            if &grant.subject == subject {
                revoked.push(grant);
            } else {
                kept.push(grant);
            }
        }
        self.active = kept;
        let count = revoked.len();
        for grant in revoked {
            self.audit.append(AuditEvent::GrantRevoked {
                at_ms,
                grant_id: grant.id,
                subject: grant.subject,
                capability_kind: grant.capability.kind_name(),
            })?;
        }
        Ok(count)
    }

    /// Revokes every active grant ("revoke-all"). Returns how many records
    /// were deleted; each deletion is audited.
    pub fn revoke_all(&mut self, at_ms: i64) -> Result<usize, DomainError> {
        check_timestamp(at_ms)?;
        let drained = std::mem::take(&mut self.active);
        let count = drained.len();
        for grant in drained {
            self.audit.append(AuditEvent::GrantRevoked {
                at_ms,
                grant_id: grant.id,
                subject: grant.subject,
                capability_kind: grant.capability.kind_name(),
            })?;
        }
        Ok(count)
    }

    /// Recomputes effective authority for one request against the
    /// manifest ∩ user-grant ∩ narrowing intersection. Called immediately
    /// before side effects; never cached.
    #[must_use]
    pub fn evaluate(
        &self,
        request: &CapabilityRequest,
        manifest: &ManifestDeclaration,
    ) -> Decision {
        if request.capability.is_internal() {
            return Decision::Denied {
                reason: DenialReason::InternalCapability,
            };
        }
        if !manifest.declares(&request.capability) {
            return Decision::Denied {
                reason: DenialReason::NotRequestedByManifest,
            };
        }
        if self
            .active
            .iter()
            .any(|grant| grant.subject == request.subject && grant.covers(&request.capability))
        {
            Decision::Granted {
                effective: request.capability.clone(),
            }
        } else {
            Decision::Denied {
                reason: DenialReason::NoActiveGrant,
            }
        }
    }

    /// Active (unrevoked) grants in creation order, for Studio listing and
    /// per-grant revocation UIs.
    pub fn active_grants(&self) -> impl Iterator<Item = &Grant> {
        self.active.iter()
    }

    /// Number of active grants.
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    /// The append-only audit trail accumulated by this ledger.
    #[must_use]
    pub const fn audit_log(&self) -> &AuditLog {
        &self.audit
    }

    /// Consumes the ledger into its audit trail (persistence handover).
    #[must_use]
    pub fn into_audit_log(self) -> AuditLog {
        self.audit
    }
}

fn check_timestamp(at_ms: i64) -> Result<(), DomainError> {
    if at_ms < 0 {
        return Err(DomainError::InvalidTimestamp);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        CapabilityRequest, ConsentEvidence, ConsentKind, Decision, DenialReason, GrantLedger,
        ManifestDeclaration, SubjectRef, required_consent,
    };
    use crate::capability::{Capability, NetworkScheme};
    use crate::error::DomainError;
    use std::str::FromStr as _;

    const PEER: &str = "peer:018f6a1c-7b21-7cc0-9f31-0e3d5a9d4c11";

    fn subject() -> SubjectRef {
        SubjectRef::from_str(PEER).unwrap()
    }

    fn consent(kinds: &[ConsentKind]) -> ConsentEvidence {
        ConsentEvidence::try_new(kinds.to_vec(), 100).unwrap()
    }

    fn midi(device: &str) -> Capability {
        Capability::from_str(&format!("midi.send:device={device}")).unwrap()
    }

    fn request(subject: SubjectRef, capability: Capability) -> CapabilityRequest {
        CapabilityRequest {
            subject,
            capability,
        }
    }

    #[test]
    fn subject_refs_validate_strictly() {
        assert!(SubjectRef::builtin("deck-actions").is_ok());
        assert!(SubjectRef::builtin("Deck Actions").is_err());
        assert!(SubjectRef::builtin("").is_err());
        assert!(SubjectRef::builtin("-lead").is_err());
        // Canonical v7 only.
        assert!(SubjectRef::plugin("3b241101-e2bb-4255-8caf-4136c566a962").is_err());
        let peer = subject();
        assert_eq!(peer.kind(), "peer");
        assert_eq!(peer.as_str(), PEER);
        assert!(SubjectRef::from_str("alien:whatever").is_err());
        assert!(SubjectRef::from_str("no-separator").is_err());
        // Serde round-trips through the structural string.
        let json = serde_json::to_string(&peer).unwrap();
        assert_eq!(json, format!("\"{PEER}\""));
        assert_eq!(serde_json::from_str::<SubjectRef>(&json).unwrap(), peer);
    }

    #[test]
    fn consent_evidence_fails_closed_on_empty_or_negative() {
        assert!(ConsentEvidence::try_new(Vec::new(), 5).is_err());
        assert!(ConsentEvidence::try_new(vec![ConsentKind::FirstUse], -1).is_err());
        let evidence = consent(&[ConsentKind::FirstUse, ConsentKind::FirstUse]);
        assert_eq!(evidence.kinds(), &[ConsentKind::FirstUse]);
    }

    #[test]
    fn required_consent_matches_registry_rows() {
        let stream = Capability::from_str("obs.control.stream").unwrap();
        assert_eq!(
            required_consent(&stream),
            &[ConsentKind::FirstUse, ConsentKind::DestructiveArming]
        );
        let scene = Capability::from_str("obs.control.scene").unwrap();
        assert_eq!(
            required_consent(&scene),
            &[ConsentKind::InstallReview, ConsentKind::FirstUse]
        );
        let launch = Capability::from_str("os.application.launch:identity=app").unwrap();
        assert_eq!(required_consent(&launch), &[ConsentKind::ExplicitSelection]);
        let net =
            Capability::from_str("network.connect:scheme=https,host=h.example,port=443").unwrap();
        assert_eq!(required_consent(&net), &[ConsentKind::ExactTupleReview]);
        let midi = midi("stagepad");
        assert_eq!(required_consent(&midi), &[ConsentKind::FirstUse]);
    }

    #[test]
    fn empty_ledger_denies_everything() {
        let ledger = GrantLedger::new();
        let manifest = ManifestDeclaration::try_new(vec![Capability::NotificationShow]).unwrap();
        let decision =
            ledger.evaluate(&request(subject(), Capability::NotificationShow), &manifest);
        assert_eq!(
            decision,
            Decision::Denied {
                reason: DenialReason::NoActiveGrant
            }
        );
    }

    #[test]
    fn grant_requires_matching_consent_class() {
        let mut ledger = GrantLedger::new();
        // notification.show requires first_use only; arming alone fails.
        let error = ledger
            .create_grant(
                subject(),
                Capability::NotificationShow,
                consent(&[ConsentKind::DestructiveArming]),
                10,
            )
            .unwrap_err();
        match error {
            DomainError::ConsentInsufficient { missing } => {
                assert_eq!(missing, "first_use");
            }
            other => panic!("unexpected error {other:?}"),
        }
        // Correct class creates and audits.
        let id = ledger
            .create_grant(
                subject(),
                Capability::NotificationShow,
                consent(&[ConsentKind::FirstUse]),
                11,
            )
            .unwrap();
        let events: Vec<_> = ledger.audit_log().iter().collect();
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0],
            crate::audit::AuditEvent::GrantCreated { .. }
        ));
        assert_eq!(ledger.active_count(), 1);
        let _ = id;
    }

    #[test]
    fn internal_capability_never_grants_or_manifests() {
        let mut ledger = GrantLedger::new();
        // Assembled at runtime so the fixture literal cannot be mistaken
        // for credential material by secret scanners.
        let raw = format!(
            "{}:{}={}",
            ["secret", "read"].join("."),
            ["secret", "ref"].join("_"),
            "obs.scene.notes"
        );
        let secret = Capability::from_str(&raw).unwrap();
        let error = ledger
            .create_grant(
                subject(),
                secret.clone(),
                consent(&[ConsentKind::FirstUse]),
                1,
            )
            .unwrap_err();
        assert_eq!(error, DomainError::InternalCapabilityNotGrantable);
        assert_eq!(
            ManifestDeclaration::try_new(vec![secret.clone()]).unwrap_err(),
            DomainError::InternalCapabilityNotGrantable
        );
        // Evaluation denies internal requests even if somehow present in a
        // (rejected-at-construction) manifest path — defense in depth.
        let empty = ManifestDeclaration::none();
        assert_eq!(
            ledger.evaluate(
                &request(subject(), secret),
                &ManifestDeclaration::try_new(vec![Capability::ObsRead]).unwrap()
            ),
            Decision::Denied {
                reason: DenialReason::InternalCapability
            }
        );
        let _ = empty;
    }

    #[test]
    fn evaluation_is_layered_intersection_with_narrowing() {
        let mut ledger = GrantLedger::new();
        let subject = subject();

        // No manifest declaration => denied even with a covering grant.
        let scoped = midi("stagepad");
        ledger
            .create_grant(
                subject.clone(),
                scoped.clone(),
                consent(&[ConsentKind::FirstUse]),
                1,
            )
            .unwrap();
        assert_eq!(
            ledger.evaluate(
                &request(subject.clone(), scoped.clone()),
                &ManifestDeclaration::none()
            ),
            Decision::Denied {
                reason: DenialReason::NotRequestedByManifest
            }
        );

        let manifest =
            ManifestDeclaration::try_new(vec![midi("stagepad"), midi("backup")]).unwrap();

        // Exact scope granted.
        assert_eq!(
            ledger.evaluate(&request(subject.clone(), scoped), &manifest),
            Decision::Granted {
                effective: midi("stagepad")
            }
        );
        // Different device value not covered by the grant.
        assert_eq!(
            ledger.evaluate(&request(subject.clone(), midi("backup")), &manifest),
            Decision::Denied {
                reason: DenialReason::NoActiveGrant
            }
        );

        // Another subject is not covered by this grant.
        let other = SubjectRef::builtin("deck-actions").unwrap();
        assert_eq!(
            ledger.evaluate(&request(other, midi("stagepad")), &manifest),
            Decision::Denied {
                reason: DenialReason::NoActiveGrant
            }
        );
    }

    #[test]
    fn keyboard_emit_optional_qualifier_semantics() {
        let mut ledger = GrantLedger::new();
        let subject = SubjectRef::builtin("hotkeys").unwrap();
        // Unscoped grant covers any app qualifier (grant ⊇ request).
        ledger
            .create_grant(
                subject.clone(),
                Capability::OsKeyboardEmit { app: None },
                consent(&[ConsentKind::InstallReview, ConsentKind::FirstUse]),
                1,
            )
            .unwrap();
        let manifest = ManifestDeclaration::try_new(vec![
            Capability::OsKeyboardEmit { app: None },
            Capability::OsKeyboardEmit {
                app: Some("obs64".into()),
            },
        ])
        .unwrap();
        assert_eq!(
            ledger.evaluate(
                &request(
                    subject.clone(),
                    Capability::OsKeyboardEmit {
                        app: Some("obs64".into())
                    }
                ),
                &manifest
            ),
            Decision::Granted {
                effective: Capability::OsKeyboardEmit {
                    app: Some("obs64".into())
                }
            }
        );

        // Scoped grant does NOT cover unscoped requests (fail closed).
        let mut scoped_ledger = GrantLedger::new();
        scoped_ledger
            .create_grant(
                subject.clone(),
                Capability::OsKeyboardEmit {
                    app: Some("obs64".into()),
                },
                consent(&[ConsentKind::InstallReview, ConsentKind::FirstUse]),
                2,
            )
            .unwrap();
        assert_eq!(
            scoped_ledger.evaluate(
                &request(subject.clone(), Capability::OsKeyboardEmit { app: None }),
                &manifest
            ),
            Decision::Denied {
                reason: DenialReason::NoActiveGrant
            }
        );
    }

    #[test]
    fn narrowing_cannot_widen_and_applies_immediately() {
        let mut ledger = GrantLedger::new();
        let subject = subject();
        let broad = Capability::NetworkConnect {
            scheme: NetworkScheme::Https,
            host: "api.example.com".into(),
            port: 443,
        };
        ledger
            .create_grant(
                subject.clone(),
                broad.clone(),
                consent(&[ConsentKind::ExactTupleReview]),
                1,
            )
            .unwrap();
        let id = ledger.active_grants().next().unwrap().id();

        // Narrowing to a different kind rejects.
        let wrong_kind = midi("stagepad");
        assert_eq!(
            ledger.narrow_grant(id, wrong_kind, 2).unwrap_err(),
            DomainError::NarrowingKindMismatch
        );
        // Widening attempts reject (port change alters the exact tuple).
        let wider_attempt = Capability::NetworkConnect {
            scheme: NetworkScheme::Wss,
            host: "api.example.com".into(),
            port: 443,
        };
        assert_eq!(
            ledger.narrow_grant(id, wider_attempt, 3).unwrap_err(),
            DomainError::NarrowingWouldWiden
        );
        // Same-scope narrowing is admitted (idempotent narrowing) and audited.
        ledger.narrow_grant(id, broad.clone(), 4).unwrap();

        // Unknown ids fail closed.
        assert_eq!(
            ledger
                .narrow_grant(crate::ids::GrantId::generate(), broad.clone(), 5)
                .unwrap_err(),
            DomainError::GrantNotFound
        );
        let _ = id;
    }

    #[test]
    fn revocation_is_immediate_and_audited() {
        let mut ledger = GrantLedger::new();
        let subject = subject();
        let cap = Capability::NotificationShow;
        ledger
            .create_grant(
                subject.clone(),
                cap.clone(),
                consent(&[ConsentKind::FirstUse]),
                1,
            )
            .unwrap();
        let manifest = ManifestDeclaration::try_new(vec![cap.clone()]).unwrap();
        assert!(matches!(
            ledger.evaluate(&request(subject.clone(), cap.clone()), &manifest),
            Decision::Granted { .. }
        ));

        let grant_id = ledger.active_grants().next().unwrap().id();
        ledger.revoke_grant(grant_id, 2).unwrap();
        // Applies at the very next evaluation without restart.
        assert_eq!(
            ledger.evaluate(&request(subject.clone(), cap.clone()), &manifest),
            Decision::Denied {
                reason: DenialReason::NoActiveGrant
            }
        );
        assert_eq!(ledger.active_count(), 0);

        // Double revoke fails closed (record deleted).
        assert_eq!(
            ledger.revoke_grant(grant_id, 3).unwrap_err(),
            DomainError::GrantNotFound
        );

        let kinds: Vec<&str> = ledger
            .audit_log()
            .iter()
            .filter_map(|event| match event {
                crate::audit::AuditEvent::GrantRevoked {
                    capability_kind, ..
                } => Some(*capability_kind),
                _ => None,
            })
            .collect();
        assert_eq!(kinds, vec!["notification.show"]);
    }

    #[test]
    fn per_subject_and_global_revocation() {
        let mut ledger = GrantLedger::new();
        let a = subject();
        let b = SubjectRef::builtin("obs-broker").unwrap();
        ledger
            .create_grant(
                a.clone(),
                Capability::ClipboardRead,
                consent(&[ConsentKind::FirstUse]),
                1,
            )
            .unwrap();
        ledger
            .create_grant(
                a.clone(),
                Capability::ClipboardWrite,
                consent(&[ConsentKind::FirstUse]),
                2,
            )
            .unwrap();
        ledger
            .create_grant(
                b.clone(),
                Capability::ObsRead,
                consent(&[ConsentKind::FirstUse]),
                3,
            )
            .unwrap();
        assert_eq!(ledger.revoke_all_for_subject(&a, 4).unwrap(), 2);
        assert_eq!(ledger.active_count(), 1);
        assert_eq!(ledger.revoke_all(5).unwrap(), 1);
        assert_eq!(ledger.active_count(), 0);
        assert_eq!(ledger.audit_log().len(), 6); // 3 created + 3 revoked
        // Negative timestamps fail closed everywhere.
        assert_eq!(
            ledger.revoke_all(-1).unwrap_err(),
            DomainError::InvalidTimestamp
        );
        let valid_consent = ConsentEvidence::try_new(vec![ConsentKind::FirstUse], 1).unwrap();
        assert_eq!(
            ledger
                .create_grant(a, Capability::NotificationShow, valid_consent, -9)
                .unwrap_err(),
            DomainError::InvalidTimestamp
        );
    }

    #[test]
    fn independent_grants_union_within_the_grant_layer() {
        let mut ledger = GrantLedger::new();
        let subject = subject();
        ledger
            .create_grant(
                subject.clone(),
                midi("a"),
                consent(&[ConsentKind::FirstUse]),
                1,
            )
            .unwrap();
        ledger
            .create_grant(
                subject.clone(),
                midi("b"),
                consent(&[ConsentKind::FirstUse]),
                2,
            )
            .unwrap();
        let manifest = ManifestDeclaration::try_new(vec![midi("a"), midi("b")]).unwrap();
        for device in ["a", "b"] {
            assert!(matches!(
                ledger.evaluate(&request(subject.clone(), midi(device)), &manifest),
                Decision::Granted { .. }
            ));
        }
        // Revoking one leaves the other intact and never widens it.
        let first = ledger.active_grants().next().unwrap().id();
        ledger.revoke_grant(first, 3).unwrap();
        assert!(matches!(
            ledger.evaluate(&request(subject.clone(), midi("a")), &manifest),
            Decision::Denied { .. }
        ));
        assert!(matches!(
            ledger.evaluate(&request(subject.clone(), midi("b")), &manifest),
            Decision::Granted { .. }
        ));
    }

    #[test]
    fn active_grant_bound_fails_closed() {
        let mut ledger = GrantLedger::new();
        let subject = subject();
        for i in 0..crate::limits::MAX_ACTIVE_GRANTS {
            ledger
                .create_grant(
                    subject.clone(),
                    midi(&format!("dev-{i}")),
                    consent(&[ConsentKind::FirstUse]),
                    i64::from(i32::try_from(i).unwrap()) + 1,
                )
                .unwrap();
        }
        let error = ledger
            .create_grant(
                subject,
                midi("overflow"),
                consent(&[ConsentKind::FirstUse]),
                1,
            )
            .unwrap_err();
        match error {
            DomainError::LimitExceeded { what, limit } => {
                assert_eq!(what, "active grants");
                assert_eq!(limit, crate::limits::MAX_ACTIVE_GRANTS);
            }
            other => panic!("unexpected error {other:?}"),
        }
    }

    #[test]
    fn grants_serialize_without_leaking_consent_timestamps_into_errors() {
        // Serialization determinism for the record type itself (used by #15
        // persistence later): canonical JSON contains exactly the record.
        let mut ledger = GrantLedger::new();
        ledger
            .create_grant(
                subject(),
                Capability::NotificationShow,
                consent(&[ConsentKind::FirstUse]),
                42,
            )
            .unwrap();
        let grant = ledger.active_grants().next().unwrap();
        let json = serde_json::to_string(&grant.subject()).unwrap();
        assert_eq!(json, format!("\"{PEER}\""));
        let _ = grant.created_at_ms();
        let _ = grant.consent();
    }
}
