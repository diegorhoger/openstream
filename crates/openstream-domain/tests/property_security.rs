//! Property-based security tests (proptest) for issue #8:
//! capability parsing round-trips and fail-closed rejection, deny-by-default
//! evaluation, narrowing/revocation invariants, internal-only `secret.*`
//! handling, and redaction — no qualifier value ever appears in audit
//! evidence serialization or Debug output.

mod common;

use common::uuid7_from;
use openstream_domain::audit::{AuditEvent, AuditLog, ExecutionState};
use openstream_domain::capability::Capability;
use openstream_domain::error::DomainError;
use openstream_domain::grant::{
    CapabilityRequest, ConsentEvidence, ConsentKind, Decision, DenialReason, GrantLedger,
    ManifestDeclaration, SubjectRef, required_consent,
};
use openstream_domain::ids::{ExecutionId, GrantId};
use openstream_domain::secret::{SECRET_REDACTED, SecretRef, SecretValue};
use proptest::prelude::*;
use std::str::FromStr as _;

fn subject(seed: u128) -> SubjectRef {
    SubjectRef::peer(&uuid7_from(seed)).expect("counter-derived ids are canonical UUIDv7")
}

/// A container that would normally serialize a secret value alongside other
/// data; serialization must fail as a whole.
#[derive(serde::Serialize)]
struct RedactionProbe<'a> {
    note: &'a str,
    value: &'a SecretValue,
}

fn consent(kinds: &[ConsentKind]) -> ConsentEvidence {
    ConsentEvidence::try_new(kinds.to_vec(), 1).expect("valid consent evidence")
}

/// Arbitrary qualifier values that stay inside the structural grammar
/// (no surrounding whitespace, bounded length).
fn arb_value() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9._~/\\\\:@#-]|[a-zA-Z0-9._~/\\\\:@#-][a-zA-Z0-9 ._~/\\\\:@#-]{0,38}[a-zA-Z0-9._~/\\\\:@#-]"
}

/// Weighted capability generator covering every qualifier shape without
/// relying on nested weighted-choice macros.
fn arb_capability() -> impl Strategy<Value = Capability> {
    // Host labels never start/end with '-' and dots separate whole labels,
    // so every generated host passes the structural validator.
    (
        0usize..15,
        arb_value(),
        proptest::option::of(arb_value()),
        "([a-z0-9]([a-z0-9-]{0,10}[a-z0-9])?\\.)[a-z0-9]([a-z0-9-]{0,10}[a-z0-9])?",
        1u16..=u16::MAX,
        0usize..4,
    )
        .prop_map(|(kind, value, app, host, port, scheme_idx)| {
            let schemes = ["http", "https", "ws", "wss"];
            let scheme = schemes[scheme_idx];
            match kind {
                0 => Capability::ObsRead,
                1 => Capability::NotificationShow,
                2 => Capability::ClipboardRead,
                3 => Capability::ClipboardWrite,
                4 => Capability::OsMediaEmit,
                5 => Capability::ObsControlScene,
                6 => Capability::ObsControlStream,
                7 => Capability::OsKeyboardEmit { app },
                8 => Capability::from_str(&format!(
                    "network.connect:scheme={scheme},host={host},port={port}"
                ))
                .expect("generator emits valid tuples"),
                9 => Capability::MidiSend {
                    device: value.clone(),
                },
                10 => Capability::OscSend {
                    endpoint: value.clone(),
                },
                11 => Capability::AudioControl {
                    device: value.clone(),
                },
                12 => Capability::FilesystemRead {
                    handle: value.clone(),
                },
                13 => Capability::FilesystemWrite {
                    handle: value.clone(),
                },
                _ => Capability::ProcessExecute { identity: value },
            }
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn capabilities_round_trip_through_canonical_string(cap in arb_capability()) {
        let canonical = cap.to_string();
        let reparsed_result = Capability::from_str(&canonical);
        let reparsed = reparsed_result.unwrap();
        prop_assert_eq!(&reparsed, &cap);
        let reserialized = reparsed.to_string();
        prop_assert_eq!(reserialized.as_str(), canonical.as_str());
        // Serde agrees with the canonical string form (JSON-escaped).
        let json = serde_json::to_string(&cap).unwrap();
        let expected_json = serde_json::to_string(&canonical).unwrap();
        prop_assert_eq!(json.as_str(), expected_json.as_str());
        let deserialized_result = serde_json::from_str::<Capability>(&json);
        prop_assert_eq!(deserialized_result.unwrap(), cap);
    }

    #[test]
    fn wildcards_reject_wherever_they_land(
        prefix in "[a-z.]{0,20}",
        filler_len in 0usize..40,
        wildcard in 0usize..2,
        tail in ".{0,20}",
    ) {
        let mut raw = String::with_capacity(prefix.len() + tail.len() + filler_len + 2);
        raw.push_str(&prefix);
        for _ in 0..filler_len {
            raw.push('x');
        }
        raw.push(if wildcard == 0 { '*' } else { '?' });
        raw.push_str(&tail);
        let result = Capability::from_str(&raw);
        let rejected_as_invalid = matches!(result, Err(DomainError::InvalidCapability { .. }));
        prop_assert!(rejected_as_invalid);
    }

    #[test]
    fn unknown_vocabulary_rejects(
        domain in "[a-zA-Z][a-zA-Z0-9]{0,10}",
        resource in "[a-zA-Z][a-zA-Z0-9]{0,10}",
    ) {
        // Any identifier outside the closed v1 vocabulary fails closed, even
        // when it accidentally resembles known grammar.
        let known = [
            "obs.read", "obs.control.scene", "obs.control.stream", "os.keyboard.emit",
            "os.media.emit", "os.application.launch", "process.execute", "clipboard.read",
            "clipboard.write", "filesystem.read", "filesystem.write", "network.connect",
            "midi.send", "osc.send", "audio.control", "notification.show", "secret.read",
        ];
        let candidate = format!("{domain}.{resource}");
        if !known.contains(&candidate.as_str()) {
            let result = Capability::from_str(&candidate);
            prop_assert!(result.is_err());
        }
    }

    #[test]
    fn secret_read_is_never_grantable_and_always_denied(
        secret_ref in "[a-z][a-z0-9_-]{0,20}\\.[a-z][a-z0-9_-]{0,20}",
        kinds in proptest::collection::vec(
            (0usize..5).prop_map(|i| match i {
                0 => ConsentKind::FirstUse,
                1 => ConsentKind::InstallReview,
                2 => ConsentKind::DestructiveArming,
                3 => ConsentKind::ExplicitSelection,
                _ => ConsentKind::ExactTupleReview,
            }),
            1..5,
        ),
    ) {
        let cap = Capability::from_str(&format!("secret.read:secret_ref={secret_ref}")).unwrap();
        prop_assert!(cap.is_internal());

        let mut ledger = GrantLedger::new();
        // Every possible consent combination still rejects grant creation.
        let evidence = ConsentEvidence::try_new(kinds, 5).unwrap();
        let grant_attempt = ledger.create_grant(subject(7), cap.clone(), evidence, 6);
        let grant_rejected =
            matches!(grant_attempt, Err(DomainError::InternalCapabilityNotGrantable));
        prop_assert!(grant_rejected);
        let manifest_attempt = ManifestDeclaration::try_new(vec![cap.clone()]);
        let manifest_rejected =
            matches!(manifest_attempt, Err(DomainError::InternalCapabilityNotGrantable));
        prop_assert!(manifest_rejected);
        let manifest = ManifestDeclaration::none();
        let request = CapabilityRequest { subject: subject(7), capability: cap };
        let decision = ledger.evaluate(&request, &manifest);
        let denied_internal =
            matches!(decision, Decision::Denied { reason: DenialReason::InternalCapability });
        prop_assert!(denied_internal);
    }

    #[test]
    fn deny_by_default_then_consent_scoped_authority(
        device in "[a-z0-9-]{1,20}",
        other_device in "[a-z0-9-]{1,20}",
    ) {
        prop_assume!(device != other_device);
        let mut ledger = GrantLedger::new();
        let requester = subject(11);
        let granted = Capability::from_str(&format!("midi.send:device={device}")).unwrap();
        let outside = Capability::from_str(&format!("midi.send:device={other_device}")).unwrap();
        let manifest =
            ManifestDeclaration::try_new(vec![granted.clone(), outside.clone()]).unwrap();

        // Deny by default.
        let default_request =
            CapabilityRequest { subject: requester.clone(), capability: granted.clone() };
        let default_decision = ledger.evaluate(&default_request, &manifest);
        let denied_by_default =
            matches!(default_decision, Decision::Denied { reason: DenialReason::NoActiveGrant });
        prop_assert!(denied_by_default);

        // Wrong consent class cannot create authority.
        let wrong_consent = ledger.create_grant(
            requester.clone(),
            granted.clone(),
            consent(&[ConsentKind::InstallReview]),
            1,
        );
        prop_assert!(wrong_consent.is_err());

        ledger
            .create_grant(requester.clone(), granted.clone(), consent(&[ConsentKind::FirstUse]), 2)
            .unwrap();

        let covered_request =
            CapabilityRequest { subject: requester.clone(), capability: granted.clone() };
        let covered_decision = ledger.evaluate(&covered_request, &manifest);
        let granted_ok = matches!(covered_decision, Decision::Granted { .. });
        prop_assert!(granted_ok);
        // Same kind but outside-grant qualifier value denies.
        let outside_request =
            CapabilityRequest { subject: requester.clone(), capability: outside.clone() };
        let outside_decision = ledger.evaluate(&outside_request, &manifest);
        let outside_denied =
            matches!(outside_decision, Decision::Denied { reason: DenialReason::NoActiveGrant });
        prop_assert!(outside_denied);
    }

    #[test]
    fn revocation_and_narrowing_apply_immediately(
        host in "[a-z][a-z0-9.-]{2,20}",
        port_a in 1u16..=u16::MAX,
        port_b in 1u16..=u16::MAX,
    ) {
        prop_assume!(port_a != port_b);
        let mut ledger = GrantLedger::new();
        let requester = subject(23);
        let broad = Capability::NetworkConnect {
            scheme: openstream_domain::capability::NetworkScheme::Https,
            host: host.clone(),
            port: port_a,
        };
        ledger
            .create_grant(requester.clone(), broad.clone(), consent(&[ConsentKind::ExactTupleReview]), 1)
            .unwrap();
        let grant_id = GrantId::from_str(&ledger.active_grants().next().unwrap().id().to_string())
            .unwrap();

        // Narrowing onto a different exact tuple rejects as widening; the
        // recorded authority never silently changes scope.
        let drifted = Capability::NetworkConnect {
            scheme: openstream_domain::capability::NetworkScheme::Https,
            host: host.clone(),
            port: port_b,
        };
        let narrow_result = ledger.narrow_grant(grant_id, drifted.clone(), 2);
        prop_assert!(narrow_result.is_err());

        // Revocation deletes the record; the next evaluation denies
        // immediately (the tuple stays manifest-declared, so the denial
        // reason isolates the missing grant).
        let revoked = ledger.revoke_grant(grant_id, 3);
        prop_assert!(revoked.is_ok());
        let post_revoke =
            CapabilityRequest { subject: requester.clone(), capability: drifted.clone() };
        let full_manifest =
            ManifestDeclaration::try_new(vec![broad.clone(), drifted.clone()]).unwrap();
        let post_decision = ledger.evaluate(&post_revoke, &full_manifest);
        let denied_after_revoke =
            matches!(post_decision, Decision::Denied { reason: DenialReason::NoActiveGrant });
        prop_assert!(denied_after_revoke);
        // Re-revoking a deleted record fails closed.
        let revoke_again = ledger.revoke_grant(grant_id, 4);
        let not_found = matches!(revoke_again, Err(DomainError::GrantNotFound));
        prop_assert!(not_found);
    }

    #[test]
    fn audit_evidence_redacts_qualifier_values(
        marker in "[A-Za-z0-9]{8,24}",
        device in "[a-z0-9-]{1,16}",
    ) {
        let mut ledger = GrantLedger::new();
        let mut extra_log = AuditLog::new();
        let requester = subject(31);
        let scoped_value = format!("{device}{marker}");
        let granted =
            Capability::from_str(&format!("midi.send:device={scoped_value}")).unwrap();

        ledger
            .create_grant(requester.clone(), granted.clone(), consent(&[ConsentKind::FirstUse]), 1)
            .unwrap();
        let grant_id = ledger.active_grants().next().unwrap().id();
        ledger.narrow_grant(grant_id, granted, 2).unwrap();
        ledger.revoke_grant(grant_id, 3).unwrap();
        let appended = extra_log.append(AuditEvent::ExecutionObserved {
            at_ms: 4,
            execution_id: ExecutionId::generate(),
            state: ExecutionState::OutcomeUnknown,
        });
        prop_assert!(appended.is_ok());

        // Serialize every event both ways and prove neither carries the
        // marker-bearing qualifier value.
        let events: Vec<AuditEvent> = ledger
            .audit_log()
            .iter()
            .chain(extra_log.iter())
            .cloned()
            .collect();
        prop_assert_eq!(events.len(), 4);
        let mut all_json = String::new();
        for event in &events {
            let json = serde_json::to_string(event).unwrap();
            prop_assert!(!json.contains(scoped_value.as_str()));
            let debug = format!("{event:?}");
            prop_assert!(!debug.contains(scoped_value.as_str()));
            all_json.push_str(&json);
        }
        // Sanity: events DID capture capability kind, lifecycle, and journal
        // vocabulary.
        prop_assert!(all_json.contains("midi.send"));
        prop_assert!(all_json.contains("outcome_unknown"));
    }

    #[test]
    fn consent_requirements_are_total(cap in arb_capability()) {
        let required = required_consent(&cap);
        prop_assert!(required.len() <= 2);
        // Every required kind name is a structural token.
        for kind in required.iter() {
            let name = kind.as_str();
            prop_assert!(!name.is_empty());
        }
    }

    #[test]
    fn secret_values_never_reach_debug_or_serialization(
        material in "[A-Za-z0-9+/=_-]{8,64}",
        context in "[a-z ]{0,20}",
    ) {
        let value = SecretValue::try_new(material.clone()).expect("in-range synthetic value");

        // Debug output shows only the redaction placeholder.
        let debug = format!("{value:?}");
        let expected_debug = format!("SecretValue({SECRET_REDACTED})");
        prop_assert_eq!(&debug, &expected_debug);
        prop_assert!(!debug.contains(material.as_str()));
        let nested_debug = format!("{:?}", vec![&value]);
        prop_assert!(!nested_debug.contains(material.as_str()));

        // Serialization of the bare value fails and leaks nothing through
        // the error text.
        let direct = serde_json::to_string(&value);
        prop_assert!(direct.is_err());
        prop_assert!(!direct.unwrap_err().to_string().contains(material.as_str()));

        // A container serialization fails as a whole; no partial JSON with
        // material can ever exist.
        let probe = RedactionProbe {
            note: context.as_str(),
            value: &value,
        };
        let container = serde_json::to_string(&probe);
        prop_assert!(container.is_err());

        // Deserialization always fails closed: there is no serialized form.
        let raw_json = format!("\"{material}\"");
        let rehydrated = serde_json::from_str::<SecretValue>(&raw_json);
        prop_assert!(rehydrated.is_err());
    }

    #[test]
    fn secret_references_round_trip_and_reject_fail_closed(
        segments in proptest::collection::vec(
            "[a-z][a-z0-9_-]{0,15}",
            1..=8,
        ),
    ) {
        let raw = segments.join(".");
        if raw.len() > openstream_domain::limits::MAX_SECRET_REF_BYTES {
            // Over-long references must reject with the structural reason.
            let rejected = SecretRef::try_new(&raw);
            let rejected_with_length_reason = matches!(
                rejected,
                Err(DomainError::InvalidSecretRef {
                    reason: "reference length out of range"
                })
            );
            prop_assert!(rejected_with_length_reason);
            return Ok(());
        }
        let parsed = SecretRef::try_new(&raw).expect("generator emits grammar-valid refs");
        let canonical = parsed.to_string();
        prop_assert_eq!(parsed.as_str(), raw.as_str());
        prop_assert_eq!(canonical.as_str(), raw.as_str());
        let json = serde_json::to_string(&parsed).unwrap();
        let expected_json = format!("\"{raw}\"");
        prop_assert_eq!(&json, &expected_json);
        let reparsed = serde_json::from_str::<SecretRef>(&json).unwrap();
        prop_assert_eq!(reparsed, parsed);

        // The internal-only capability built from this reference round-trips
        // too, proving the capability layer validates through the same
        // grammar.
        let capability = Capability::from_str(&format!("secret.read:secret_ref={raw}"))
            .expect("grammar-valid reference parses");
        let canonical_capability = format!("secret.read:secret_ref={raw}");
        prop_assert!(capability.is_internal());
        prop_assert_eq!(capability.to_string(), canonical_capability);
    }

    #[test]
    fn audit_evidence_never_carries_secret_material(
        secret_ref in "[a-z][a-z0-9_-]{1,15}\\.[a-z][a-z0-9_-]{1,15}",
        kinds in proptest::collection::vec(
            (0usize..5).prop_map(|i| match i {
                0 => ConsentKind::FirstUse,
                1 => ConsentKind::InstallReview,
                2 => ConsentKind::DestructiveArming,
                3 => ConsentKind::ExplicitSelection,
                _ => ConsentKind::ExactTupleReview,
            }),
            1..5,
        ),
    ) {
        // Even a full attempt to grant `secret.read` (always rejected) plus
        // its evaluation leaves an evidence trail that carries only
        // qualifier-free kind names — never the reference value.
        let capability =
            Capability::from_str(&format!("secret.read:secret_ref={secret_ref}")).unwrap();
        prop_assert!(capability.is_internal());
        let mut ledger = GrantLedger::new();
        let evidence = ConsentEvidence::try_new(kinds, 5).unwrap();
        prop_assert!(
            ledger
                .create_grant(subject(41), capability.clone(), evidence, 6)
                .is_err()
        );
        let decision = ledger.evaluate(
            &CapabilityRequest {
                subject: subject(41),
                capability,
            },
            &ManifestDeclaration::none(),
        );
        let denied_internal = matches!(decision, Decision::Denied { .. });
        prop_assert!(denied_internal);

        // A legitimate sibling grant plus an independently appended
        // execution observation form the whole retained evidence trail.
        ledger
            .create_grant(
                subject(41),
                Capability::NotificationShow,
                consent(&[ConsentKind::FirstUse]),
                7,
            )
            .unwrap();
        let mut extra = AuditLog::new();
        extra
            .append(AuditEvent::ExecutionObserved {
                at_ms: 8,
                execution_id: ExecutionId::generate(),
                state: ExecutionState::Accepted,
            })
            .unwrap();

        for event in ledger.audit_log().iter().chain(extra.iter()) {
            let json = serde_json::to_string(event).unwrap();
            let debug = format!("{event:?}");
            prop_assert!(!json.contains(secret_ref.as_str()));
            prop_assert!(!debug.contains(secret_ref.as_str()));
        }
    }
}
