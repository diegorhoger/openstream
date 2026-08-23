# Governance

OpenStream uses a serial merge lane with parallel analysis. GitHub is the only durable source of truth.

## Work lifecycle

1. Select the oldest open PR; otherwise the lowest-numbered unblocked issue.
2. Validate dependencies and acceptance criteria.
3. Plan in the issue, create an issue branch, and open a draft PR.
4. Implement within scope.
5. Independently verify success and failure paths.
6. Review exact-head diff; add Security/Release roles when triggered.
7. Remediate; every push restarts verification.
8. Post exact-head review gate.
9. Human reviews and merges with expected-head protection.
10. Close/update dependents and continue immediately.

No agent self-approves or autonomously merges. No unresolved thread or failed dependency may be bypassed.

## Decisions

Irreversible architecture, public protocol, permissions, privacy, licensing, billing, signing, or migration decisions require ADRs. Accepted ADRs can be superseded only by another ADR with compatibility and reversal evidence.

## Contributions

Material work is issue-first and must carry DCO sign-off. Governance changes require maintainer review. The initial foundation PR is the only exception to the normal one-issue/small-PR budget.
