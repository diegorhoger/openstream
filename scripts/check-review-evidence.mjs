#!/usr/bin/env node

import process from 'node:process';

export const ROLES = ['VERIFIER', 'REVIEWER', 'SECURITY', 'EVALUATOR'];
const MARKER = /<!--\s*openstream-review-evidence:v1\s*([\s\S]*?)-->/g;

function bodyField(body, name) {
  const match = body.match(new RegExp(`^[-*]?[ \\t]*${name}:[ \\t]*(.+?)[ \\t]*$`, 'mi'));
  return match?.[1]?.trim() ?? null;
}

export function parseEvidence(comments) {
  const records = [];
  for (const comment of comments) {
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

export function validateEvidence({ body, comments, expectedHead }) {
  const problems = [];
  const records = parseEvidence(comments);
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
    const matches = records.filter(
      (record) => record.role === role && record.context === context && record.head === expectedHead && record.verdict === 'APPROVE',
    );
    if (matches.length !== 1) {
      problems.push(`${role} requires exactly one matching evidence record; found ${matches.length}`);
      continue;
    }
    const record = matches[0];
    if (!Array.isArray(record.commands) || record.commands.length === 0) problems.push(`${role} evidence must contain at least one command`);
    if (!Array.isArray(record.results) || record.results.length === 0) problems.push(`${role} evidence must contain at least one result`);
    if (typeof record.summary !== 'string' || record.summary.trim().length < 20) problems.push(`${role} evidence summary is missing or too short`);
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
  if (!body || !expectedHead || !repo || !prNumber || !token) throw new Error('PR_BODY, EXPECTED_HEAD, GH_REPO, PR_NUMBER, and GH_TOKEN are required');
  const result = validateEvidence({ body, comments: await fetchAllIssueComments({ repo, prNumber, token }), expectedHead });
  if (!result.ok) {
    for (const problem of result.problems) console.error(problem);
    process.exit(1);
  }
  console.log(`durable review evidence validated for ${ROLES.length} roles at ${expectedHead}`);
}

if (import.meta.url === `file://${process.argv[1]}`) main().catch((error) => { console.error(error.message); process.exit(1); });
