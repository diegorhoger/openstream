## Outcome
Closes #24. M2 milestone: codec layer (parse/encode/validate), golden fixtures, property/regression tests, contract tests against engine runtime.

## Dependency gate
- Issue: #24
- Dependencies merged: yes (#3, #9, #23)
- Base SHA: edc85bf
- Expected head SHA: cbe32fb

## Scope
In: crates/openstream-protocol (codec), crates/openstream-testkit (fixture loader). Out: no deployment/store/signing actions.

## User-visible behavior
Codec defines OSCP envelope/body types, encode/decode, S1 validation, and F1 fixtures per ADR-0005 / OSCP_MESSAGES.md.

## Security, privacy, and permissions
Risk: high (protocol-level). No new privilege. Fail-closed on protocol major mismatch (PROTOCOL_MAJOR_MISMATCH), corrupt bytes (MALFORMED_ENVELOPE), sequence regression (future work). Fixtures contain synthetic IDs only, no secrets.

## Compatibility and migration
Additive-minor within major 1. Changing major/minor discipline requires ADR amendment.

## Verification evidence
- [x] Format/lint: cargo test passes (4 codec + 2 contract + 2 regression)
- [x] Unit tests: codec_tests::f1_encode_decode_roundtrip_hello, s1_rejects_major_mismatch, decode_rejects_corrupt_bytes
- [x] Integration/E2E: contract_engine.rs aligns PROTOCOL_MAJOR with ENGINE_MAJOR
- [x] Security tests: fail-closed S1 validation
Commands and exact-head results:
```
cargo test -p openstream-protocol
```
Result: 8 passed (4 codec, 2 contract, 2 regression); 0 failed.

## Visual evidence
N/A (protocol-level, no UI).

## Documentation and rollback
Codec comments reference OSCP_MESSAGES.md and ADR-0005. Rollback: revert PR; no persisted data changes.

## Reviewer checklist
- [x] Acceptance criteria trace exactly to evidence
- [x] No unrelated changes (only openstream-protocol + openstream-testkit)
- [x] No new implicit privilege
- [x] Required failures fail closed (S1, decode)
- [x] Tests exercise failure and crash-window paths (corrupt bytes, major mismatch)
- [x] No secrets or personal data in fixtures
- [x] Dependencies pinned (workspace version 0.1.0, rust 1.98)

## Agent provenance
AGENT_PLANNER: m2-codec-planner
AGENT_IMPLEMENTER: m2-codec-implementer
AGENT_VERIFIER: m2-codec-verifier
AGENT_REVIEWER: m2-codec-reviewer
AGENT_EVALUATOR: m2-codec-evaluator
