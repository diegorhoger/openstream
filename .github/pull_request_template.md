## Outcome

Closes #

## Dependency gate

- Issue: #
- Dependencies merged: yes/no
- Base SHA:
- Expected head SHA:

## Scope

In:

Out:

## User-visible behavior

## Security, privacy, and permissions

- Risk: low/medium/high/critical
- Capability changes:
- Threat-model impact:

## Compatibility and migration

## Verification evidence

- [ ] Format/lint
- [ ] Unit tests
- [ ] Integration/E2E tests
- [ ] Platform matrix where applicable
- [ ] Accessibility evidence where applicable
- [ ] Security tests where applicable

Commands and exact-head results:

## Visual evidence

Screenshots/video, or N/A with reason.

## Documentation and rollback

## Reviewer checklist

- [ ] Acceptance criteria trace exactly to evidence
- [ ] No unrelated changes
- [ ] No new implicit privilege
- [ ] Required failures fail closed
- [ ] Tests exercise failure and crash-window paths
- [ ] No secrets or personal data
- [ ] Dependencies are pinned and reviewed

## Agent provenance

The values below are stable context IDs, not role names. They must identify distinct contexts.

AGENT_PLANNER: pending
AGENT_IMPLEMENTER: pending
AGENT_VERIFIER: pending
AGENT_REVIEWER: pending
AGENT_SECURITY: pending
AGENT_EVALUATOR: pending

GATE_VERIFIER_VERDICT: PENDING@<exact-head>
GATE_REVIEWER_VERDICT: PENDING@<exact-head>
GATE_SECURITY_VERDICT: PENDING@<exact-head>
GATE_EVALUATOR_VERDICT: PENDING@<exact-head>

EVIDENCE_VERIFIER_COMMENT: pending
EVIDENCE_REVIEWER_COMMENT: pending
EVIDENCE_SECURITY_COMMENT: pending
EVIDENCE_EVALUATOR_COMMENT: pending

The repository owner must relay exactly one `openstream-review-evidence:v1`
HTML-comment record for each required role, containing role, matching context,
exact head, verdict, summary, commands, structured results, and reviewer-report
SHA-256 digest. Record each relay comment's
numeric ID above; the trusted gate fetches only those four comments directly.
