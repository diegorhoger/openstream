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

## Completion

Continue repair until acceptance criteria pass, required checks are green on the exact head, threads are resolved, and an independent evaluator posts the machine-readable review gate. Merge is always human-controlled until a separately approved policy says otherwise.

## Hard stop

Stop on legal/licensing/trademark/pricing decisions; new arbitrary code/process authority; uncertain cryptography/auth/tenant/plugin/updater boundary; destructive migration; billing activation; production deployment; DNS/store/signing action; new personal-data transfer; unresolved high/critical security risk; missing permission/credential/device/evidence; or incompatible dependency license/advisory.

At a hard stop, preserve the branch and SHA, post evidence and 2–3 options, mark needs-decision, cancel downstream writes, and wait.
