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

### AGENT_* provenance (clean-context reviews)

Verifier, Reviewer, Security, and Evaluator must be separate clean-context
review agents. They receive the specification, acceptance criteria, relevant
repository state, exact head, and required commands, but not the implementer's
conclusions. Each independently inspects the change and returns `APPROVE`,
`REPAIR`, or `HARD_STOP` with reproducible evidence.

The PR body records each review context as
`OSTR-CONTEXT-<ROLE>-<CONTEXT_ID>` and each verdict as
`GATE_<ROLE>_VERDICT: <RESULT>@<40-hex-head>`. Context identifiers must be
distinct. A durable PR comment must contain the complete verdict and evidence
for every required role. The machine check validates syntax, role separation,
and exact-head binding; it cannot prove context isolation. Context isolation is
provided by the orchestrator and must never be represented as cryptographic or
human independence.

The implementer may record returned context identifiers and verdicts but may
not author the review conclusions. Pending values fail closed. Every push
invalidates all verdicts; PR-body edits do not invalidate a verdict when the
recorded head remains the current PR head.

## Completion

Continue repair until acceptance criteria pass, required checks are green on the exact head, threads are resolved, and every required clean-context reviewer returns `APPROVE` with durable evidence. When the repository owner has granted standing autonomous integration authority, an approved exact head may be merged without an additional routine human confirmation. Human authority remains mandatory for the hard stops below.

## Hard stop

Stop on legal/licensing/trademark/pricing decisions; new arbitrary code/process authority; uncertain cryptography/auth/tenant/plugin/updater boundary; destructive migration; billing activation; production deployment; DNS/store/signing action; new personal-data transfer; unresolved high/critical security risk; missing permission/credential/device/evidence; or incompatible dependency license/advisory.

At a hard stop, preserve the branch and SHA, post evidence and 2–3 options, mark needs-decision, cancel downstream writes, and wait.
