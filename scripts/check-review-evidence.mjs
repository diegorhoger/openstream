#!/usr/bin/env node

import process from 'node:process';

export const ROLES = ['VERIFIER', 'REVIEWER', 'SECURITY', 'EVALUATOR'];
const MARKER = /<!--\s*openstream-review-evidence:v1\s*([\s\S]*?)-->/g;
const CONTEXT_SUFFIX = '[A-Za-z0-9_-]+';

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
  const evidenceFingerprints = new Set();

  for (const role of ROLES) {
    const context = bodyField(body, `AGENT_${role}`);
    const verdict = bodyField(body, `GATE_${role}_VERDICT`);
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
    const validList = (value) => Array.isArray(value) && value.length >= 1 && value.length <= 64
      && value.every((item) => typeof item === 'string' && item.trim().length >= 5 && item.length <= 2000);
    if (!validList(record.commands)) problems.push(`${role} evidence commands must contain 1-64 nonblank strings of at most 2000 characters`);
    if (!validList(record.results)) problems.push(`${role} evidence results must contain 1-64 nonblank strings of at most 2000 characters`);
    if (validList(record.commands) && record.commands.some((command) => !/^(?:\.?\.?\/)?[A-Za-z0-9_.-]+(?:\s+\S.*)$/.test(command.trim()))) problems.push(`${role} evidence commands must describe executable invocations with arguments`);
    if (validList(record.commands) && record.commands.some((command) => /^(?:echo|printf|true|false)\b/i.test(command.trim()))) problems.push(`${role} evidence commands must not be no-op placeholders`);
    const outcome = /\b(?:pass(?:ed)?|fail(?:ed|ure)?|success|error|warning|approve|repair|hard_stop|exit(?: code)?\s*[=:]?\s*-?\d+|\d+\/\d+)\b/i;
    if (validList(record.results) && record.results.some((result) => !outcome.test(result))) problems.push(`${role} evidence results must contain a concrete outcome`);
    if (typeof record.summary !== 'string' || record.summary.trim().length < 40 || record.summary.length > 8000 || !record.summary.toUpperCase().includes(role)) problems.push(`${role} evidence summary must contain 40-8000 characters and identify the role`);
    if (validList(record.commands) && validList(record.results)) {
      const fingerprint = JSON.stringify([record.commands, record.results]);
      if (evidenceFingerprints.has(fingerprint)) problems.push(`${role} evidence duplicates another role's commands and results`);
      evidenceFingerprints.add(fingerprint);
    }
  }

  if (contexts.size !== ROLES.length) problems.push('review contexts must be pairwise distinct');
  if (records.some((record) => record.invalid_json)) problems.push('evidence marker contains invalid JSON');
  return { ok: problems.length === 0, problems };
}

async function fetchEvidenceComments({ repo, prNumber, token, body, trustedActor }) {
  const comments = [];
  const ids = ROLES.map((role) => bodyField(body, `EVIDENCE_${role}_COMMENT`));
  if (ids.some((id) => !/^\d+$/.test(id ?? '')) || new Set(ids).size !== ROLES.length) {
    throw new Error('four distinct numeric EVIDENCE_<ROLE>_COMMENT fields are required');
  }
  for (let index = 0; index < ids.length; index += 1) {
    const id = ids[index];
    const role = ROLES[index];
    const url = `https://api.github.com/repos/${repo}/issues/comments/${id}`;
    const response = await fetch(url, { headers: { Accept: 'application/vnd.github+json', Authorization: `Bearer ${token}`, 'X-GitHub-Api-Version': '2022-11-28' } });
    if (!response.ok) throw new Error(`GitHub evidence comment ${id} request failed: ${response.status}`);
    const comment = await response.json();
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
