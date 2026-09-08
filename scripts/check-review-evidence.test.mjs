import assert from 'node:assert/strict';
import test from 'node:test';
import { ROLES, validateEvidence } from './check-review-evidence.mjs';

const head = 'a'.repeat(40);
const trustedActor = 'repository-owner';
const contexts = Object.fromEntries(ROLES.map((role) => [role, `OSTR-CONTEXT-${role}-${role.toLowerCase()}-1`]));
const body = ROLES.map((role) => `AGENT_${role}: ${contexts[role]}\nGATE_${role}_VERDICT: APPROVE@${head}`).join('\n');

function comment(role, overrides = {}) {
  const record = { role, context: contexts[role], head, verdict: 'APPROVE', summary: `Independent ${role} review found the exact head acceptable.`, commands: [`git diff --check base..head # ${role}`], results: [`exit 0; ${role} verification PASS`], ...overrides };
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
  assert.match(problems, /results must contain/);
});
test('rejects placeholder evidence and malformed context identifiers', () => {
  const badBody = body.replace(contexts.VERIFIER, 'NOT-A-CONTEXT-VERIFIER');
  const comments = ROLES.map((role) => comment(role, role === 'VERIFIER' ? { context: 'NOT-A-CONTEXT-VERIFIER', summary: '1234567890123456789012345678901234567890', commands: ['x'], results: ['y'] } : {}));
  const problems = validateEvidence({ body: badBody, comments, expectedHead: head, trustedActor }).problems.join('\n');
  assert.match(problems, /does not identify a VERIFIER clean context/);
  assert.match(problems, /commands must contain/);
  assert.match(problems, /results must contain/);
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
  const comments = ROLES.map((role) => comment(role, { commands: ['echo hello'], results: ['exit 0'] }));
  const problems = validate(comments).problems.join('\n');
  assert.match(problems, /no-op placeholders/);
  assert.match(problems, /duplicates another role/);
});
