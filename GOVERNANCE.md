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
8. Post one machine-readable durable exact-head evidence record for every required review role.
9. Merge only after exact-head CI and every required clean-context review approve. Standing owner authority may replace routine human confirmation; hard stops still require human authority.
10. Update dependents and continue immediately.

No agent self-approves. No failed dependency, unresolved thread, missing DCO, missing durable review evidence, or failing exact-head check may be bypassed.

## Enforceable evidence

- CI checks out and asserts the exact PR head.
- The machine-readable roadmap graph is validated for complete ordered issue coverage and backward-only dependencies.
- PR bodies carry distinct stable context IDs for implementer, verifier, reviewer, and evaluator.
- Repository-owner PR comments relay exactly one machine-readable evidence record per required role, bound to the body context and exact head, with commands and results. The PR body names four distinct comment IDs; the trusted default-branch gate fetches only those comments. Owner comment mutation and PR lifecycle/body changes republish a SHA-bound evidence status.
- Every PR commit is checked for a DCO `Signed-off-by` trailer.
- Product paths reject Python without treating absent directories as success-by-error.
- Hosted service implementation is rejected from the public repository.

The PR that first installs the trusted default-branch evidence workflow cannot
be validated by that not-yet-installed workflow. It requires a one-time
documented bootstrap exception and clean-context approval. After integration,
no PR-head copy of the validator is authoritative.

## Merge and CODEOWNERS boundary

The current repository has one eligible owner. Requiring that same CODEOWNER to approve a PR they authored would deadlock GitHub review. CODEOWNERS is therefore advisory until a second eligible non-author maintainer is configured. Expected-head confirmation, zero unresolved threads, required checks, and durable evidence remain mandatory. Standing owner authority permits orchestrator integration without routine human confirmation; hard stops still require human authority. A future branch-protection change requiring CODEOWNER approval is itself a human governance decision.

## Decisions

Irreversible architecture, public protocol, permissions, privacy, licensing, billing, signing, source boundary, or migration decisions require ADRs. Accepted ADRs can be superseded only by another ADR with compatibility and reversal evidence.

The foundation PR is the only exception to the normal one-issue/small-PR budget. Its unsigned bootstrap commit has no automatic DCO exception; PR #62 Decision C remains required before merge.
