import assert from 'node:assert/strict';
import test from 'node:test';
import { ROLES, normalizeEvidenceIds, validateEvidence } from './check-review-evidence.mjs';

const head = 'a'.repeat(40);
const trustedActor = 'repository-owner';
const contexts = Object.fromEntries(ROLES.map((role) => [role, `OSTR-CONTEXT-${role}-${role.toLowerCase()}-1`]));
const body = ROLES.map((role) => `AGENT_${role}: ${contexts[role]}\nGATE_${role}_VERDICT: APPROVE@${head}`).join('\n');

function comment(role, overrides = {}) {
  const hex = String(ROLES.indexOf(role) + 1).repeat(64);
  const record = { role, context: contexts[role], head, verdict: 'APPROVE', summary: `Independent ${role} review found the exact head acceptable.`, commands: [`git diff --check base..head # ${role}`], results: [{ exit_code: 0, output_digest: `sha256:${hex}`, assertion: `${role} diff verification completed successfully.` }], report_digest: `sha256:${hex}`, ...overrides };
  return { id: role, expected_role: role, user: { login: trustedActor }, body: `<!-- openstream-review-evidence:v1\n${JSON.stringify(record)}\n-->` };
}

const validate = (comments) => validateEvidence({ body, comments, expectedHead: head, trustedActor });

test('accepts one complete exact-head record per role', () => assert.deepEqual(validate(ROLES.map((role) => comment(role))), { ok: true, problems: [] }));
test('rejects missing role evidence', () => assert.match(validate(ROLES.slice(1).map((role) => comment(role))).problems.join('\n'), /VERIFIER requires exactly one/));
test('rejects stale-head evidence', () => assert.match(validate(ROLES.map((role) => comment(role, role === 'REVIEWER' ? { head: 'b'.repeat(40) } : {}))).problems.join('\n'), /REVIEWER evidence is not bound/));
test('rejects non-approve evidence', () => assert.match(validate(ROLES.map((role) => comment(role, role === 'SECURITY' ? { verdict: 'REPAIR' } : {}))).problems.join('\n'), /SECURITY evidence verdict/));
test('rejects multiple markers in one role relay comment', () => {
  const comments = ROLES.map((role) => comment(role));
  comments[3].body += comments[3].body;
  assert.match(validate(comments).problems.join('\n'), /EVALUATOR relay comment requires exactly one evidence marker; found 2/);
});
test('rejects incomplete commands and results', () => {
  const problems = validate(ROLES.map((role) => comment(role, role === 'VERIFIER' ? { commands: [null], results: [' '] } : {}))).problems.join('\n');
  assert.match(problems, /commands must contain/);
  assert.match(problems, /results must correspond/);
});
test('rejects placeholder evidence and malformed context identifiers', () => {
  const badBody = body.replace(contexts.VERIFIER, 'NOT-A-CONTEXT-VERIFIER');
  const comments = ROLES.map((role) => comment(role, role === 'VERIFIER' ? { context: 'NOT-A-CONTEXT-VERIFIER', summary: '1234567890123456789012345678901234567890', commands: ['x'], results: ['y'], report_digest: 'x' } : {}));
  const problems = validateEvidence({ body: badBody, comments, expectedHead: head, trustedActor }).problems.join('\n');
  assert.match(problems, /does not identify a VERIFIER clean context/);
  assert.match(problems, /commands must contain/);
  assert.match(problems, /results must correspond/);
  assert.match(problems, /summary must contain/);
});
test('ignores evidence markers from untrusted commenters', () => {
  const untrusted = comment('VERIFIER');
  untrusted.user.login = 'untrusted-user';
  assert.match(validate([untrusted, ...ROLES.slice(1).map((role) => comment(role))]).problems.join('\n'), /VERIFIER requires exactly one trusted owner relay comment/);
});
test('binds every named relay comment to its declared role', () => {
  const comments = ROLES.map((role) => comment(role));
  comments[0].expected_role = 'SECURITY';
  comments[2].expected_role = 'VERIFIER';
  const problems = validate(comments).problems.join('\n');
  assert.match(problems, /VERIFIER relay comment contains evidence for SECURITY/);
  assert.match(problems, /SECURITY relay comment contains evidence for VERIFIER/);
});
test('rejects no-op and cross-role duplicated evidence', () => {
  const comments = ROLES.map((role) => comment(role, { commands: ['bash -c echo-hello'], report_digest: `sha256:${'a'.repeat(64)}` }));
  const problems = validate(comments).problems.join('\n');
  assert.match(problems, /non-noop/);
  assert.match(problems, /report digest duplicates another role/);
});
test('rejects duplicate governance fields in the PR body', () => {
  const duplicateBody = `${body}\nAGENT_VERIFIER: OSTR-CONTEXT-VERIFIER-conflict`;
  assert.match(validateEvidence({ body: duplicateBody, comments: ROLES.map((role) => comment(role)), expectedHead: head, trustedActor }).problems.join('\n'), /AGENT_VERIFIER must appear exactly once; found 2/);
});
test('rejects duplicate review contexts across roles', () => {
  const duplicateContextBody = body.replace(contexts.REVIEWER, contexts.VERIFIER);
  const comments = ROLES.map((role) => comment(role, role === 'REVIEWER' ? { context: contexts.VERIFIER } : {}));
  assert.match(validateEvidence({ body: duplicateContextBody, comments, expectedHead: head, trustedActor }).problems.join('\n'), /review contexts must be pairwise distinct/);
});
test('normalizes comment IDs and rejects leading-zero aliases', () => {
  assert.deepEqual(normalizeEvidenceIds(['1', '2', '3', '4']), ['1', '2', '3', '4']);
  assert.throws(() => normalizeEvidenceIds(['1', '01', '001', '0001']), /canonical positive integers/);
});
test('rejects nonzero command results for an approval', () => {
  const comments = ROLES.map((role) => comment(role));
  comments[0] = comment('VERIFIER', { results: [{ exit_code: 1, output_digest: `sha256:${'1'.repeat(64)}`, assertion: 'VERIFIER command failed and cannot support approval.' }] });
  assert.match(validate(comments).problems.join('\n'), /results must correspond/);
});
