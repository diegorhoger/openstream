#!/usr/bin/env node

import process from 'node:process';

export const ROLES = ['VERIFIER', 'REVIEWER', 'SECURITY', 'EVALUATOR'];
const MARKER = /<!--\s*openstream-review-evidence:v1\s*([\s\S]*?)-->/g;
const CONTEXT_SUFFIX = '[A-Za-z0-9_-]+';

export function normalizeEvidenceIds(values) {
  if (values.some((id) => !/^[1-9]\d*$/.test(id ?? ''))) throw new Error('evidence comment IDs must be canonical positive integers without leading zeroes');
  const normalized = values.map((id) => BigInt(id).toString());
  if (new Set(normalized).size !== normalized.length) throw new Error('evidence comment IDs must be numerically distinct');
  return normalized;
}

function bodyValues(body, name) {
  return [...body.matchAll(new RegExp(`^[-*]?[ \\t]*${name}:[ \\t]*(.+?)[ \\t]*$`, 'gmi'))].map((match) => match[1].trim());
}

export function parseEvidence(comments, trustedActor) {
  const records = [];
  for (const comment of comments) {
    if (comment.user?.login !== trustedActor) continue;
    for (const match of (comment.body ?? '').matchAll(MARKER)) {
      try {
        records.push({ ...JSON.parse(match[1]), comment_id: comment.id, expected_role: comment.expected_role });
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
  const reportDigests = new Set();
  const getField = (name) => {
    const values = bodyValues(body, name);
    if (values.length !== 1) problems.push(`${name} must appear exactly once; found ${values.length}`);
    return values.length === 1 ? values[0] : null;
  };

  for (const role of ROLES) {
    const context = getField(`AGENT_${role}`);
    const verdict = getField(`GATE_${role}_VERDICT`);
    if (!context) {
      problems.push(`missing AGENT_${role}`);
      continue;
    }
    if (!new RegExp(`^OSTR-CONTEXT-${role}-${CONTEXT_SUFFIX}$`).test(context)) {
      problems.push(`AGENT_${role} does not identify a ${role} clean context`);
    }
    contexts.add(context);
    if (verdict !== `APPROVE@${expectedHead}`) {
      problems.push(`GATE_${role}_VERDICT must be APPROVE@${expectedHead}`);
    }
    const roleComments = comments.filter((comment) => comment.expected_role === role);
    if (roleComments.length !== 1 || roleComments[0].user?.login !== trustedActor) {
      problems.push(`${role} requires exactly one trusted owner relay comment`);
      continue;
    }
    const roleRecords = records.filter((record) => record.expected_role === role);
    if (roleRecords.length !== 1) {
      problems.push(`${role} relay comment requires exactly one evidence marker; found ${roleRecords.length}`);
      continue;
    }
    const record = roleRecords[0];
    if (record.role !== role) problems.push(`${role} relay comment contains evidence for ${record.role ?? 'no role'}`);
    if (record.context !== context) problems.push(`${role} evidence context does not match AGENT_${role}`);
    if (record.head !== expectedHead) problems.push(`${role} evidence is not bound to ${expectedHead}`);
    if (record.verdict !== 'APPROVE') problems.push(`${role} evidence verdict must be APPROVE`);
    const commandsValid = Array.isArray(record.commands) && record.commands.length >= 1 && record.commands.length <= 64
      && record.commands.every((item) => typeof item === 'string' && item.trim().length >= 10 && item.length <= 2000
        && /^(?:\.?\.?\/)?[A-Za-z0-9_.-]+(?:\s+\S.*)$/.test(item.trim()) && !/^(?:echo|printf|true|false|bash\s+-c|sh\s+-c)\b/i.test(item.trim()));
    const resultsValid = Array.isArray(record.results) && record.results.length === record.commands?.length
      && record.results.every((item) => item && item.exit_code === 0
        && /^sha256:[0-9a-f]{64}$/.test(item.output_digest)
        && typeof item.assertion === 'string' && item.assertion.trim().length >= 20 && item.assertion.length <= 2000);
    if (!commandsValid) problems.push(`${role} evidence commands must contain 1-64 executable, non-noop strings of 10-2000 characters`);
    if (!resultsValid) problems.push(`${role} evidence results must correspond to commands with integer exit_code, SHA-256 output_digest, and a substantive assertion`);
    if (typeof record.summary !== 'string' || record.summary.trim().length < 40 || record.summary.length > 8000 || !record.summary.toUpperCase().includes(role)) problems.push(`${role} evidence summary must contain 40-8000 characters and identify the role`);
    if (!/^sha256:[0-9a-f]{64}$/.test(record.report_digest ?? '')) problems.push(`${role} evidence requires a SHA-256 digest of the complete reviewer report`);
    else if (reportDigests.has(record.report_digest)) problems.push(`${role} evidence report digest duplicates another role`);
    else reportDigests.add(record.report_digest);
  }

  if (contexts.size !== ROLES.length) problems.push('review contexts must be pairwise distinct');
  if (records.some((record) => record.invalid_json)) problems.push('evidence marker contains invalid JSON');
  return { ok: problems.length === 0, problems };
}

async function fetchEvidenceComments({ repo, prNumber, token, body, trustedActor }) {
  const comments = [];
  const idFields = ROLES.map((role) => bodyValues(body, `EVIDENCE_${role}_COMMENT`));
  if (idFields.some((values) => values.length !== 1)) throw new Error('each EVIDENCE_<ROLE>_COMMENT field must appear exactly once');
  const ids = normalizeEvidenceIds(idFields.map(([id]) => id));
  for (let index = 0; index < ids.length; index += 1) {
    const id = ids[index];
    const role = ROLES[index];
    const url = `https://api.github.com/repos/${repo}/issues/comments/${id}`;
    const response = await fetch(url, { headers: { Accept: 'application/vnd.github+json', Authorization: `Bearer ${token}`, 'X-GitHub-Api-Version': '2022-11-28' } });
    if (!response.ok) throw new Error(`GitHub evidence comment ${id} request failed: ${response.status}`);
    const comment = await response.json();
    if (BigInt(comment.id).toString() !== id) throw new Error(`GitHub returned comment ${comment.id} for requested evidence comment ${id}`);
    if (comment.issue_url !== `https://api.github.com/repos/${repo}/issues/${prNumber}`) throw new Error(`evidence comment ${id} does not belong to PR #${prNumber}`);
    if (comment.user?.login !== trustedActor) throw new Error(`evidence comment ${id} is not authored by the trusted owner`);
    comments.push({ ...comment, expected_role: role });
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
  const result = validateEvidence({ body, comments: await fetchEvidenceComments({ repo, prNumber, token, body, trustedActor }), expectedHead, trustedActor });
  if (!result.ok) {
    for (const problem of result.problems) console.error(problem);
    process.exit(1);
  }
  console.log(`durable review evidence validated for ${ROLES.length} roles at ${expectedHead}`);
}

if (import.meta.url === `file://${process.argv[1]}`) main().catch((error) => { console.error(error.message); process.exit(1); });
