# Review and release gates

## Pull-request requirements

- Declared issue and dependencies matching `docs/product/ROADMAP_GRAPH.tsv`.
- Exact base/head SHAs; CI must check out and assert the head SHA.
- Acceptance criteria traced to reproducible evidence.
- Format, lint, unit, integration, dependency/license, secrets, policy, and applicable platform/accessibility/security checks.
- DCO sign-off on every commit unless a separately approved, exact-SHA bootstrap policy says otherwise.
- Distinct stable context IDs for implementer, verifier, reviewer, and evaluator.
- No unrelated change, implicit privilege, false-success state, or undocumented crash window.
- No unresolved conversation.
- Four owner-relayed evidence comments named by ID in the PR body; the trusted default-branch gate must publish `governance/review-evidence` for the exact head.

## Main protection target

- Pull request required; squash merge and linear history.
- Dismiss stale approvals; require conversations resolved and required checks.
- Require an eligible non-author human approval when a second maintainer exists; do not configure a sole-author CODEOWNERS deadlock.
- Block force pushes and branch deletion.
- Actions default read-only; grant per job; pin third-party Actions by commit SHA.
- Autonomous integration is permitted only under standing owner authority after all exact-head checks and evidence gates pass; hard stops remain human-controlled.
- Releases require protected environments, signed tags, checksums, SBOM, provenance, and human authorization.

Repository-setting enforcement is a maintainer action and must be verified separately from documentation and CI.
