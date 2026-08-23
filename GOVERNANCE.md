# Governance

OpenStream uses a serial merge lane with parallel analysis. GitHub is the durable source of truth.

## Work lifecycle

1. Resume the oldest open PR; otherwise select the lowest-numbered issue whose graph dependencies are merged.
2. Validate `docs/product/ROADMAP_GRAPH.tsv` and the issue acceptance contract.
3. Plan in the issue, create an issue branch, and open a draft PR.
4. Implement within scope.
5. Independently verify success, failure, denial, and applicable crash windows.
6. Review the exact-head diff; add Security/Release contexts when triggered.
7. Remediate; every push invalidates earlier verification and review.
8. Post the machine-readable exact-head review gate.
9. A human reviews and merges with expected-head protection.
10. Update dependents and continue immediately.

No agent self-approves or autonomously merges. No failed dependency, unresolved thread, missing DCO, or failing exact-head check may be bypassed.

## Enforceable evidence

- CI checks out and asserts the exact PR head.
- The machine-readable roadmap graph is validated for complete ordered issue coverage and backward-only dependencies.
- PR bodies carry distinct stable context IDs for implementer, verifier, reviewer, and evaluator.
- Every PR commit is checked for a DCO `Signed-off-by` trailer.
- Product paths reject Python without treating absent directories as success-by-error.
- Hosted service implementation is rejected from the public repository.

## Human and CODEOWNERS boundary

The current repository has one eligible owner. Requiring that same CODEOWNER to approve a PR they authored would deadlock GitHub review. CODEOWNERS is therefore advisory until a second eligible non-author maintainer is configured. Human merge, expected-head confirmation, zero unresolved threads, and required checks remain mandatory. A future branch-protection change requiring CODEOWNER approval is itself a human governance decision.

## Decisions

Irreversible architecture, public protocol, permissions, privacy, licensing, billing, signing, source boundary, or migration decisions require ADRs. Accepted ADRs can be superseded only by another ADR with compatibility and reversal evidence.

The foundation PR is the only exception to the normal one-issue/small-PR budget. Its unsigned bootstrap commit has no automatic DCO exception; PR #62 Decision C remains required before merge.
