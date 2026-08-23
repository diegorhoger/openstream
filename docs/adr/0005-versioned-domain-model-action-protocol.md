# ADR-0005: Versioned domain model and action protocol specification

Status: proposed  
Date: 2026-08-23  
Decision owners: @diegorhoger (maintainer)

## Context

Issue #3 requires M0 to specify typed decks, controls, action graphs, identities, envelopes, errors, execution states, compatibility, and migration rules before any runtime exists. Several decisions already exist and must not be contradicted:

- `docs/adr/0003-oscp-protobuf-websocket.md` fixes canonical Protobuf messages with boundary-specific secure transports.
- `docs/architecture/PROTOCOL.md` fixes OSCP v1 transports, the envelope field inventory, honest delivery/recovery semantics, and the seven authoritative execution states.
- `docs/architecture/TECHNICAL_SPEC.md` §4–§6 fixes the twenty core entities, UUIDv7 identity, engine limits, and sync merge rules.
- `docs/security/CAPABILITY_TAXONOMY.md` fixes the permission vocabulary, evaluation intersection, and lifecycle rules.

What remained unspecified at the precision issue #3 demands: typed document schemas and their version discipline, identity generation authority, a staged validation pipeline, version-negotiation mechanics, expiry/deadline ordering, admission-dedupe window bounds and idempotency rules, snapshot/patch recovery flows, a typed error registry, and a golden-fixtures obligation. Without these, M1/M2 implementers would improvise public-contract details after the fact — exactly the silent-breaking-change risk issue #3's hard stop forbids.

## Decision

Adopt two companion specifications and bind them to the existing decisions:

1. `docs/architecture/DOMAIN_MODEL.md` (v1) is the authoritative typed domain model: document schema versioning (`major.minor`, additive-minor discipline), UUIDv7 identity with generation-authority and non-reuse rules, the twenty core entities from TECHNICAL_SPEC §4, control kinds with interaction and visual states, action-graph shape and limits, a five-stage fail-closed validation pipeline, persistence/migration rules, and portability rules.
2. `docs/architecture/OSCP_MESSAGES.md` (v1) is the precise typed companion to `PROTOCOL.md`: protobuf message definitions as normative spec text (generated artifacts remain M2 scope per ADR-0003; no build artifacts ship in this repository yet), identifier conventions, Hello negotiation mechanics, the execution-state machine including `outcome_unknown` reconciliation rules, expiry/deadline admission ordering, admission dedupe and idempotency rules with bounded durable windows, snapshot/patch recovery flows, a v1 error-code registry, and the golden-fixture plan.
3. Contract parameters fixed here that were previously prose-bounded elsewhere:
   - Admission dedupe retention defaults to 24 hours within hard bounds [1 hour, 7 days], pruned oldest-first, with unresolved `outcome_unknown` entries exempt from pruning.
   - Sequence is per-sender monotonic non-decreasing within one session stream; regression fails closed; reconnect always opens a new session and recovers through snapshots/patches, never through sequence continuation.
   - Expired work is journaled as `expired` and never queued; deadline measurement switches to monotonic runtime clocks at admission.

These documents extend precision only; they change no OSCP v1 wire semantic fixed by `PROTOCOL.md`. Where wording appears to conflict, `PROTOCOL.md` plus this ADR govern and a reconciling PR must follow before dependent work proceeds.

## Alternatives considered

- **Inline the precision into `PROTOCOL.md`:** rejected; it is the stable summary contract, and burying schema, negotiation, recovery, and fixture detail there would make review and M2 conformance harder rather than easier.
- **Express schemas as JSON Schema:** rejected; ADR-0003 made Protobuf canonical for OSCP messages, and dual-canonical formats invite drift.
- **Leave dedupe-window bounds and negotiation mechanics to M2 implementation:** rejected; issue #3's acceptance criteria require the contract now, and post-hoc parameter choices would be silent breaking changes against already-paired clients.
- **ULIDs or UUIDv4 for durable entity IDs:** rejected; TECHNICAL_SPEC §4 already selects UUIDv7 (standard, time-ordered) and nothing here overrides it.

## Consequences

- M1 persistence migrations must realize the entity tables, `DedupeEntry` retention bounds, and soft deletion exactly as specified in `DOMAIN_MODEL.md`.
- M2 codegen must consume the protobuf sketches; once codegen activates, drift between generated code and sketches is a CI-blocking defect via stale-generation rejection (already contractual in `PROTOCOL.md`).
- Golden fixtures become release-retained conformance assets; adding a message, state, or error code without fixture vectors is invalid under this decision.
- Changing any parameter fixed in Decision item 3 now requires an ADR amendment, not a code change alone.
- Review surface grows by two long-lived specification documents, traded against removing implementation discretion from public-contract behavior.

## Security and privacy impact

Risk: high (issue-declared). This decision constrains future implementations only; it grants no privilege and ships no executable code.

- Fail-closed posture is preserved and made mechanically checkable: unknown capabilities, protocol-major mismatches, sequence regressions, expired commands, and non-forward-compatible unknown fields deny.
- Dedupe/expiry/recovery rules close the replay and delay-release windows analyzed as TM-RPL-01..04 in `docs/security/THREAT_MODEL.md`; `outcome_unknown` may be superseded only by explicit reconciliation evidence, never inferred success.
- Fixture rules forbid secrets and personal data; synthetic identifiers only; evidence obeys taxonomy redaction rules.
- Protocol/replay semantics touch networking and remote control, so independent Security-role review applies before merge per `AGENTS.md`.

## Compatibility and migration

- Additive-minor evolution within domain/protocol major 1; removals, type changes, or semantic reinterpretations require a protocol-major-style bump, an ADR, migration tests, and a documented rollback path.
- Portable import/export carries capability requests, never grants; imports referencing ungranted capabilities arrive disabled with a typed denial (`CAPABILITY_TAXONOMY.md` §7).
- Invalid merged graphs remain stored but disabled; operational presses never become sync operations (`TECHNICAL_SPEC.md` §6).
- Rollback of this decision itself: revert the documentation commits; nothing consumes these specifications yet.

## Reversal plan

Supersede via a new ADR demonstrating compatible golden fixtures, a migration proof for any persisted data, regenerated clients, and independent security review. Identifiers are never reused; deprecations keep their rows and records.

## Evidence

- Requirement source: GitHub issue #3 (outcome, acceptance criteria, hard stops).
- Consistency sources honored: `docs/architecture/PROTOCOL.md`, `docs/architecture/TECHNICAL_SPEC.md`, `docs/architecture/SECURITY.md`, `docs/security/CAPABILITY_TAXONOMY.md`, `docs/security/THREAT_MODEL.md`, `docs/product/PRD.md`, `docs/product/NON_GOALS.md`, `docs/adr/0001`–`0004`.
- Verification commands, consistency-grep results, and exact-head SHA are recorded in the implementing pull request; independent verification is a separate gate.
