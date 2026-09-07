#!/usr/bin/env node

import process from 'node:process';

export const ROLES = ['VERIFIER', 'REVIEWER', 'SECURITY', 'EVALUATOR'];
const MARKER = /<!--\s*openstream-review-evidence:v1\s*([\s\S]*?)-->/g;

function bodyField(body, name) {
  const match = body.match(new RegExp(`^[-*]?[ \\t]*${name}:[ \\t]*(.+?)[ \\t]*$`, 'mi'));
  return match?.[1]?.trim() ?? null;
}

export function parseEvidence(comments, trustedActor) {
  const records = [];
  for (const comment of comments) {
    if (comment.user?.login !== trustedActor) continue;
    for (const match of (comment.body ?? '').matchAll(MARKER)) {
      try {
        records.push({ ...JSON.parse(match[1]), comment_id: comment.id });
      } catch {
        records.push({ invalid_json: true, comment_id: comment.id });
      }
    }
  }
  return records;
}

export function validateEvidence({ body, comments, expectedHead, trustedActor }) {
  const problems = [];
  const records = parseEvidence(comments, trustedActor);
  const contexts = new Set();

  for (const role of ROLES) {
    const context = bodyField(body, `AGENT_${role}`);
    const verdict = bodyField(body, `GATE_${role}_VERDICT`);
    if (!context) {
      problems.push(`missing AGENT_${role}`);
      continue;
    }
    contexts.add(context);
    if (verdict !== `APPROVE@${expectedHead}`) {
      problems.push(`GATE_${role}_VERDICT must be APPROVE@${expectedHead}`);
    }
    const roleRecords = records.filter((record) => record.role === role);
    if (roleRecords.length !== 1) {
      problems.push(`${role} requires exactly one evidence record; found ${roleRecords.length}`);
      continue;
    }
    const record = roleRecords[0];
    if (record.context !== context) problems.push(`${role} evidence context does not match AGENT_${role}`);
    if (record.head !== expectedHead) problems.push(`${role} evidence is not bound to ${expectedHead}`);
    if (record.verdict !== 'APPROVE') problems.push(`${role} evidence verdict must be APPROVE`);
    const validList = (value) => Array.isArray(value) && value.length >= 1 && value.length <= 64
      && value.every((item) => typeof item === 'string' && item.trim().length > 0 && item.length <= 2000);
    if (!validList(record.commands)) problems.push(`${role} evidence commands must contain 1-64 nonblank strings of at most 2000 characters`);
    if (!validList(record.results)) problems.push(`${role} evidence results must contain 1-64 nonblank strings of at most 2000 characters`);
    if (typeof record.summary !== 'string' || record.summary.trim().length < 20 || record.summary.length > 8000) problems.push(`${role} evidence summary must contain 20-8000 characters`);
  }

  if (contexts.size !== ROLES.length) problems.push('review contexts must be pairwise distinct');
  if (records.some((record) => record.invalid_json)) problems.push('evidence marker contains invalid JSON');
  return { ok: problems.length === 0, problems };
}

async function fetchAllIssueComments({ repo, prNumber, token }) {
  const comments = [];
  let url = `https://api.github.com/repos/${repo}/issues/${prNumber}/comments?per_page=100`;
  while (url) {
    const response = await fetch(url, { headers: { Accept: 'application/vnd.github+json', Authorization: `Bearer ${token}`, 'X-GitHub-Api-Version': '2022-11-28' } });
    if (!response.ok) throw new Error(`GitHub comments request failed: ${response.status}`);
    comments.push(...(await response.json()));
    url = response.headers.get('link')?.split(',').find((part) => part.includes('rel="next"'))?.match(/<([^>]+)>/)?.[1] ?? '';
  }
  return comments;
}

async function main() {
  const body = process.env.PR_BODY;
  const expectedHead = process.env.EXPECTED_HEAD;
  const repo = process.env.GH_REPO;
  const prNumber = process.env.PR_NUMBER;
  const token = process.env.GH_TOKEN;
  const trustedActor = process.env.TRUSTED_EVIDENCE_ACTOR;
  if (!body || !expectedHead || !repo || !prNumber || !token || !trustedActor) throw new Error('PR_BODY, EXPECTED_HEAD, GH_REPO, PR_NUMBER, GH_TOKEN, and TRUSTED_EVIDENCE_ACTOR are required');
  const result = validateEvidence({ body, comments: await fetchAllIssueComments({ repo, prNumber, token }), expectedHead, trustedActor });
  if (!result.ok) {
    for (const problem of result.problems) console.error(problem);
    process.exit(1);
  }
  console.log(`durable review evidence validated for ${ROLES.length} roles at ${expectedHead}`);
}

if (import.meta.url === `file://${process.argv[1]}`) main().catch((error) => { console.error(error.message); process.exit(1); });
