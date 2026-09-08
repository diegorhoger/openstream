# Contributing

Thank you for helping build OpenStream.

1. Search existing issues and claim one bounded issue before material changes.
2. Branch from current `main`; use one issue per branch and pull request.
3. Keep production authority in Rust unless a documented platform/UI boundary requires otherwise.
4. Add deterministic success, failure, crash-window, and denial tests as applicable.
5. Never add telemetry by default, secrets, proprietary assets, undocumented permissions, or hosted Cloud implementation to this public repository.
6. Add an ADR for a public protocol, irreversible architecture, security boundary, source boundary, dependency policy, or migration decision.
7. Run repository checks and include exact commands, results, and SHA in the PR. The gate-by-gate reference and local-parity runner live in `docs/engineering/CHECKS.md` and `scripts/local-parity.sh`.
8. Sign off every commit under the Developer Certificate of Origin with `git commit -s`.
9. Use distinct planner, implementer, verifier, reviewer, security, and evaluator context IDs in the PR provenance block. Security review is mandatory; boundary-sensitive changes require expanded threat-model evidence.

The initial unsigned foundation commit predates DCO enforcement and remains an unresolved bootstrap gate. No exception is implied until the maintainer selects and executes Decision C from PR #62.

Security vulnerabilities must be reported privately as described in `SECURITY.md`, not in public issues.

---

# Governance model (added 2026-09)

This section records the current exact-head review model and the four
hard stops in `AGENTS.md`. It is a developer-facing summary; the
authoritative source is `AGENTS.md` itself.

## Roles

Every pull request must distinguish six roles in its provenance block
and the exact-head review process:

- **Planner** — chose the issue and the work breakdown.
- **Implementer** — authored the code, tests, and docs. Single identity
  per PR. The implementer is the author of the PR body.
- **Verifier** — confirmed commit identity, DCO trailer, base/expected
  head SHAs, declared dependencies match the TSV row, no `Closes #N`
  tokens where forbidden, the local review-gate script passes.
- **Reviewer** — independently inspected the change for scope, semantic
  correctness, and contract quality. Read the diff, ran the tests,
  exercised the change end-to-end.
- **Evaluator** — confirmed acceptance criteria satisfaction and
  evidence integrity, including CI status at exact head.
- **Security** — mandatory for every change; networking, authentication,
  secrets, OS permissions, remote control, plugins, billing, updates,
  signing, privacy, or tenant isolation require expanded threat-model evidence.

## Exact-head review process

Every push to a pull request branch invalidates every prior verdict
on that branch. A clean-context review at the new exact head SHA
is required before merge. The implementer may not self-approve.

The CI workflows (`governance`, `quality`, `package` in
`.github/workflows/`) gate the PR on:

- The PR body containing four `OSTR-CONTEXT-<ROLE>-<CONTEXT_ID>`
  fields and four `GATE_<ROLE>_VERDICT: <RESULT>@<40-hex-head>` lines.
- The role encoded in the `OSTR-CONTEXT-*` value matching the field it
  appears in.
- The verdict line equal to `APPROVE@<head>` (or, for a held verdict,
  `REQUEST_CHANGES@<head>` / `NEEDS_HUMAN@<head>`).
- The exact head SHA in the body matching the current branch head.

The contract enforces shape + exact-head binding. It does **not**
prove reviewer independence; that property is provided by the
orchestrator and is not represented as cryptographic or human
independence. Real reviewer independence requires a separate machine
context (separate process, separate credentials, separate memory)
that the implementer does not control.

## Standing autonomous-integration authority

When the repository owner has granted standing autonomous-integration
authority, an approved exact head may be merged without an additional
routine human confirmation. The human retains hard-stop authority
over the items listed in `AGENTS.md` "Hard stop" and may at any time
override the autonomous-integration authority by leaving a
`HARD_STOP` verdict on the PR or closing the PR without merge.

## Hard stops (from `AGENTS.md`)

The implementer must stop and request operator authorization before
any of the following:

- Legal, licensing, trademark, or pricing decisions.
- New arbitrary code/process authority.
- Uncertain cryptography, authentication, tenant, plugin, or updater
  boundary.
- Destructive migration.
- Billing activation.
- Production deployment.
- DNS, store, or signing action.
- New personal-data transfer.
- Unresolved high or critical security risk.
- Missing permission, credential, device, or evidence.
- Incompatible dependency license or advisory.

A failed test, lint error, compiler error, merge conflict, incomplete
implementation, architectural defect, reviewer rejection, CI failure,
or ordinary engineering uncertainty is **not** a hard stop. Such
issues are investigated, repaired, verified, and the work continues.

## What the implementer must never do

- Self-approve its own work. A `REPAIR` verdict is not the same as
  approving a fix; the next push invalidates the verdict.
- Weaken tests or acceptance criteria merely to obtain approval.
- Hide failures or substitute mocks, stubs, placeholders, TODOs, or
  workarounds for production functionality.
- Treat the four clean-context reviewers as a ceremonial rubber
  stamp. The contract enforces shape; the reviewer context enforces
  substance.
- Move tags, create releases, or sign artifacts without operator
  authorization.

## Process

1. Create a branch from `main` for one bounded issue.
2. Implement, test, lint, run the local review-gate script.
3. Push the branch; CI runs `governance`, `quality`, `package`.
4. Update the PR body with the four `OSTR-CONTEXT-*` context IDs, the
   `Issue:`, `Dependencies merged:`, `Base SHA:`, `Expected head SHA:`
   fields, and the four `GATE_<ROLE>_VERDICT: PENDING@<head>` lines.
5. Dispatch four clean-context reviewers (verifier, reviewer,
   security where applicable, evaluator) and record their
   `APPROVE@<head>` (or other) returns in the body.
6. Governance workflow runs against the body; passes when shape and
   exact-head binding match.
7. Merge under the operator's standing autonomous-integration
   authority once CI is green and four `APPROVE@<head>` lines are
   recorded.
8. Every push invalidates every prior verdict; the four reviews must
   be re-dispatched at the new head.

## Source of truth

The authoritative contract lives in `.github/workflows/governance.yml`
(the contract) and `scripts/check-review-gates.mjs` (the local
review-gate script). The shape contract is a `bash` block in
`governance.yml`; the local script is Node.js. Both must agree on
the required field names, the role regex, and the exact-head
binding format.

If the body shape, the contract, and the local script disagree, the
implementer fixes the disagreement in a single bounded PR with a
test that exercises every field, before advancing any further.
