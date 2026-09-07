import assert from 'node:assert/strict';
import test from 'node:test';
import { ROLES, validateEvidence } from './check-review-evidence.mjs';

const head = 'a'.repeat(40);
const trustedActor = 'repository-owner';
const contexts = Object.fromEntries(ROLES.map((role) => [role, `OSTR-CONTEXT-${role}-${role.toLowerCase()}-1`]));
const body = ROLES.map((role) => `AGENT_${role}: ${contexts[role]}\nGATE_${role}_VERDICT: APPROVE@${head}`).join('\n');

function comment(role, overrides = {}) {
  const record = { role, context: contexts[role], head, verdict: 'APPROVE', summary: `Independent ${role} review found the exact head acceptable.`, commands: ['git diff --check base..head'], results: ['exit 0'], ...overrides };
  return { id: role, user: { login: trustedActor }, body: `<!-- openstream-review-evidence:v1\n${JSON.stringify(record)}\n-->` };
}

const validate = (comments) => validateEvidence({ body, comments, expectedHead: head, trustedActor });

test('accepts one complete exact-head record per role', () => assert.deepEqual(validate(ROLES.map((role) => comment(role))), { ok: true, problems: [] }));
test('rejects missing role evidence', () => assert.match(validate(ROLES.slice(1).map((role) => comment(role))).problems.join('\n'), /VERIFIER requires exactly one/));
test('rejects stale-head evidence', () => assert.match(validate(ROLES.map((role) => comment(role, role === 'REVIEWER' ? { head: 'b'.repeat(40) } : {}))).problems.join('\n'), /REVIEWER evidence is not bound/));
test('rejects non-approve evidence', () => assert.match(validate(ROLES.map((role) => comment(role, role === 'SECURITY' ? { verdict: 'REPAIR' } : {}))).problems.join('\n'), /SECURITY evidence verdict/));
test('rejects duplicate role evidence even when one record is stale', () => assert.match(validate([...ROLES.map((role) => comment(role)), comment('EVALUATOR', { head: 'b'.repeat(40) })]).problems.join('\n'), /EVALUATOR requires exactly one evidence record; found 2/));
test('rejects incomplete commands and results', () => {
  const problems = validate(ROLES.map((role) => comment(role, role === 'VERIFIER' ? { commands: [null], results: [' '] } : {}))).problems.join('\n');
  assert.match(problems, /commands must contain/);
  assert.match(problems, /results must contain/);
});
test('ignores evidence markers from untrusted commenters', () => {
  const untrusted = comment('VERIFIER');
  untrusted.user.login = 'untrusted-user';
  assert.match(validate([untrusted, ...ROLES.slice(1).map((role) => comment(role))]).problems.join('\n'), /VERIFIER requires exactly one evidence record; found 0/);
});
