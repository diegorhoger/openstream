# OpenStream agent contract

This repository is operated as a dependency-aware engineering graph.

## Source of truth

GitHub issues, pull requests, reviews, checks, and exact commit SHAs are authoritative. Resume the oldest open PR before selecting work. If none exists, select the lowest-numbered open issue whose dependencies are merged. Never silently skip an older blocked issue.

## Implementation rules

- One bounded issue per branch and PR, except the initial foundation PR.
- Rust owns production core, protocol, execution, permissions, persistence/sync rules, pairing/crypto, and shared native logic.
- Another language requires a documented platform boundary. No Python ships in the product.
- Preserve scope; update tests and documentation; record evidence in GitHub.
- Never fabricate evidence, weaken a test to get green CI, expose secrets, force-push shared history, or bypass branch protection.
- The implementer cannot perform the final independent verification or approve its own work.
- Every push invalidates previous verification and review gates.

## Required roles

Planner, Implementer, Verifier, Reviewer, and Evaluator are separate contexts. Add Security for networking, authentication, secrets, OS permissions, remote control, plugins, billing, updates, signing, privacy, or tenant isolation. Add Release for artifacts, versioning, signing, stores, or deployment.

### AGENT_* provenance (operator-issued)

The four gate roles — Verifier, Reviewer, Security, Evaluator — are external
endorsers, not the implementer. The PR body's `AGENT_VERIFIER`,
`AGENT_REVIEWER`, `AGENT_SECURITY`, `AGENT_EVALUATOR` fields are
**operator-issued** strings of the form

```
OSTR-GATE-<ROLE>-YYYYMMDD-XXXXXXX
```

where `<ROLE>` is `VERIFIER` | `REVIEWER` | `SECURITY` | `EVALUATOR`,
`YYYYMMDD` is the dispatch date, and `XXXXXXX` is a 7-character
operator-controlled tag. The string carries the role name in it
explicitly so a role/field mismatch fails closed in
`.github/workflows/governance.yml`.

`AGENT_IMPLEMENTER` is the implementer's self-declaration and is not
required to use the `OSTR-GATE-*` shape. It must be non-empty,
non-literal-`pending`, and pairwise distinct from the four gate values.

The implementer cannot synthesize an `OSTR-GATE-*` string for a gate
they did not actually dispatch: doing so would be fabricating external
endorsement, which is the failure mode the contract is designed to
close. If a gate value is missing or fabricated, the contract fails
closed; the operator must issue a real `OSTR-GATE-*` string (or
correct the field) before the PR can be mergeable.

## Completion

Continue repair until acceptance criteria pass, required checks are green on the exact head, threads are resolved, and an independent evaluator posts the machine-readable review gate. Merge is always human-controlled until a separately approved policy says otherwise.

## Hard stop

Stop on legal/licensing/trademark/pricing decisions; new arbitrary code/process authority; uncertain cryptography/auth/tenant/plugin/updater boundary; destructive migration; billing activation; production deployment; DNS/store/signing action; new personal-data transfer; unresolved high/critical security risk; missing permission/credential/device/evidence; or incompatible dependency license/advisory.

At a hard stop, preserve the branch and SHA, post evidence and 2–3 options, mark needs-decision, cancel downstream writes, and wait.
