# Graph engineering system

## Roles

| Role | Authority | Prohibited |
|---|---|---|
| Orchestrator | Read queue, validate dependencies, assign roles, enforce gates | Implement, approve, merge |
| Planner | Map criteria to files/tests/risks/rollback | Production changes |
| Implementer | Own one issue branch and remediate findings | Final verification/self-approval |
| Verifier | Reproduce, test criteria/failure paths, publish evidence | Rewrite criteria to fit implementation |
| Reviewer | Exact-head correctness/scope/architecture review | Approve own implementation |
| Security | Threat/capability/privacy/abuse review | Waive high/critical findings |
| Evaluator | Validate exact SHA and post review gate | Merge |
| Release | Artifacts/SBOM/provenance/notes/rollback | Use production credentials without human gate |

## Queue algorithm

1. Fetch `main`, open PRs, issues, checks, reviews, and comments.
2. Resume the oldest open PR first.
3. Otherwise choose the lowest-numbered open issue whose declared dependencies are merged.
4. Never silently skip a blocked older issue; publish the blocker and evaluate whether independent read-only work can proceed.
5. Keep implementation/merge order serial. Planning, verification, review, and security can run in parallel.

## State machine

`queued -> planned -> implementing -> verifying -> reviewing -> remediating -> review_gate -> integrating -> completed`

Any new commit moves verification/review/review_gate back to `verifying`. A hard stop moves work to `needs_decision` and blocks downstream mutation.

## Durable issue status comment

```text
OPENSTREAM_AGENT_STATUS v1
ISSUE: #
BRANCH:
HEAD:
STATE:
RISK:
DEPENDENCIES:
PLANNER:
IMPLEMENTER:
VERIFIER:
REVIEWER:
SECURITY:
EVIDENCE:
BLOCKERS:
NEXT:
```

## Review evidence gate

The PR body names distinct clean contexts and exact-head verdicts for
VERIFIER, REVIEWER, SECURITY, and EVALUATOR, plus four canonical positive
integer `EVIDENCE_<ROLE>_COMMENT` IDs. Each owner-relayed comment contains one
`openstream-review-evidence:v1` JSON record with its role, context, head,
`APPROVE` verdict, substantive summary, commands, structured zero-exit results
and SHA-256 output/report digests. The trusted default-branch workflow fetches
those comments directly and publishes `governance/review-evidence` for the
exact head.

The gate is invalid when base/head or named evidence changes, a check fails, a
new unresolved thread exists, dependency state changes, risk changes, or the PR
gains migration/permission/public-API/billing/signing/deployment impact.
Standing owner authority permits integration without routine human approval;
the hard stops below still require human authority.

## Hard stops

See `AGENTS.md`. Orchestrator preserves exact state, posts evidence and explicit options, marks needs-decision, cancels downstream writes, and waits. No workaround may silently weaken the boundary.
