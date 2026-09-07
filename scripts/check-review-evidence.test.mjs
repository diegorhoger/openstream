import assert from 'node:assert/strict';
import test from 'node:test';
import { ROLES, validateEvidence } from './check-review-evidence.mjs';

const head = 'a'.repeat(40);
const contexts = Object.fromEntries(ROLES.map((role) => [role, `OSTR-CONTEXT-${role}-${role.toLowerCase()}-1`]));
const body = ROLES.map((role) => `AGENT_${role}: ${contexts[role]}\nGATE_${role}_VERDICT: APPROVE@${head}`).join('\n');

function comment(role, overrides = {}) {
  const record = { role, context: contexts[role], head, verdict: 'APPROVE', summary: `Independent ${role} review found the exact head acceptable.`, commands: ['git diff --check base..head'], results: ['exit 0'], ...overrides };
  return { id: role, body: `<!-- openstream-review-evidence:v1\n${JSON.stringify(record)}\n-->` };
}

test('accepts one complete exact-head record per role', () => assert.deepEqual(validateEvidence({ body, comments: ROLES.map((role) => comment(role)), expectedHead: head }), { ok: true, problems: [] }));
test('rejects missing role evidence', () => assert.match(validateEvidence({ body, comments: ROLES.slice(1).map((role) => comment(role)), expectedHead: head }).problems.join('\n'), /VERIFIER requires exactly one/));
test('rejects stale-head evidence', () => assert.match(validateEvidence({ body, comments: ROLES.map((role) => comment(role, role === 'REVIEWER' ? { head: 'b'.repeat(40) } : {})), expectedHead: head }).problems.join('\n'), /REVIEWER requires exactly one/));
test('rejects non-approve evidence', () => assert.match(validateEvidence({ body, comments: ROLES.map((role) => comment(role, role === 'SECURITY' ? { verdict: 'REPAIR' } : {})), expectedHead: head }).problems.join('\n'), /SECURITY requires exactly one/));
test('rejects duplicate matching evidence', () => assert.match(validateEvidence({ body, comments: [...ROLES.map((role) => comment(role)), comment('EVALUATOR')], expectedHead: head }).problems.join('\n'), /EVALUATOR requires exactly one.*found 2/));
test('rejects incomplete commands and results', () => {
  const problems = validateEvidence({ body, comments: ROLES.map((role) => comment(role, role === 'VERIFIER' ? { commands: [], results: [] } : {})), expectedHead: head }).problems.join('\n');
  assert.match(problems, /at least one command/);
  assert.match(problems, /at least one result/);
});
