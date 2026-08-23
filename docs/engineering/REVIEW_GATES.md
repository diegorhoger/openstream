# Review and release gates

## Pull-request requirements

- Declared issue and merged dependencies.
- Base/head exact SHAs.
- Acceptance criteria traced to evidence.
- Format, lint, unit, integration, dependency/license, secrets, policy, and applicable platform/accessibility/security checks.
- No unrelated changes or implicit privilege.
- Failure paths and rollback documented.
- No unresolved conversations.
- Independent reviewer and evaluator.

## Main protection target

- Pull request required; squash merge and linear history.
- Dismiss stale approvals; require conversations resolved and CODEOWNERS review.
- Required checks cannot be bypassed.
- Block force pushes and deletion.
- Actions default read-only; grant per job; pin third-party Actions by commit SHA.
- Auto-merge disabled.
- Releases require protected environments, signed tags, checksums, SBOM, provenance, and human authorization.

Repository-setting enforcement is a maintainer action and must be verified separately from this document.
