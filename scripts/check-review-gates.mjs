#!/usr/bin/env node
// Review-gate validation for OpenStream pull requests (issue #6).
//
// Machine-checks the parts of `docs/engineering/REVIEW_GATES.md` that a CI
// step can verify offline, complementing `.github/workflows/governance.yml`
// (which already enforces AGENT_* provenance and DCO):
//   - the declared issue exists as a row in docs/product/ROADMAP_GRAPH.tsv;
//   - every dependency listed in the PR body matches that row's declared
//     dependencies (the TSV is the source of truth);
//   - Base SHA / Expected head SHA fields are present and well-formed, and
//     the expected head equals the exact checked-out HEAD.
//
// Body sources:
//   --refresh            fetch the LIVE pull-request body via `gh api`
//                        (env: PR_NUMBER, GH_REPO, GH_TOKEN). A push always
//                        precedes its own head-SHA declaration in the body,
//                        so CI retries within a bounded window (default
//                        120s, REVIEW_GATE_REFRESH_WINDOW_MS to override)
//                        for the body to catch up, then FAILS CLOSED.
//   --body-file <path>   static body from disk (local parity).
//   PR_BODY env          static body (kept for compatibility).
//
// Fails closed in every unresolved case.

import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import process from 'node:process';

const TSV_PATH = 'docs/product/ROADMAP_GRAPH.tsv';

function parseArgs(argv) {
  const args = { bodyFile: null, refresh: false };
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === '--body-file') {
      args.bodyFile = argv[i + 1] ?? fail('usage: check-review-gates.mjs [--refresh] [--body-file <path>]');
      i += 1;
    } else if (argv[i] === '--refresh') {
      args.refresh = true;
    }
  }
  return args;
}

function fetchLivePrBody() {
  const prNumber = process.env.PR_NUMBER;
  const repo = process.env.GH_REPO;
  if (!prNumber || !repo) {
    throw new Error('--refresh needs PR_NUMBER and GH_REPO in the environment');
  }
  const raw = execFileSync('gh', ['api', `repos/${repo}/pulls/${prNumber}`], {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  return JSON.parse(raw).body ?? '';
}

function readTsvRows(root) {
  const text = readFileSync(`${root}/${TSV_PATH}`, 'utf8');
  const lines = text.split(/\r?\n/).filter((line) => line.trim().length > 0);
  const rows = new Map();
  for (const line of lines.slice(1)) {
    const [issueColumn, , depsColumn] = line.split('\t');
    const issue = Number.parseInt(issueColumn, 10);
    if (!Number.isInteger(issue)) continue;
    const deps =
      depsColumn && depsColumn.trim() !== '-'
        ? depsColumn
            .split(',')
            .map((dep) => Number.parseInt(dep.trim(), 10))
            .filter((dep) => Number.isInteger(dep))
        : [];
    rows.set(issue, deps);
  }
  return rows;
}

function field(body, name) {
  const match = body.match(new RegExp(`^[-*]?[ \\t]*${name}:[ \\t]*(.+?)[ \\t]*$`, 'mi'));
  return match ? match[1] : null;
}

function validate(body, head, tsvRows) {
  const problems = [];

  const issueField = field(body, 'Issue');
  const issueNumber = issueField == null ? NaN : Number.parseInt(issueField.replace('#', '').trim(), 10);
  if (!Number.isInteger(issueNumber) || issueNumber < 1) {
    problems.push(`PR body must declare "Issue: #<n>" (got "${issueField ?? 'missing'}")`);
  } else if (!tsvRows.has(issueNumber)) {
    problems.push(`declared issue #${issueNumber} has no row in ${TSV_PATH}`);
  }

  const mergedField = field(body, 'Dependencies merged');
  if (mergedField == null) {
    problems.push('PR body must declare "Dependencies merged: yes/no" with listed issues');
  } else {
    if (!/^(yes|no)\b/i.test(mergedField.trim())) {
      problems.push(`"Dependencies merged" must start with yes/no (got "${mergedField}")`);
    }
    const listed = [...mergedField.matchAll(/#(\d+)/g)].map((m) => Number.parseInt(m[1], 10));
    const declaredDeps = Number.isInteger(issueNumber) ? (tsvRows.get(issueNumber) ?? []) : [];
    const sortedListed = [...new Set(listed)].sort((a, b) => a - b);
    const sortedDeclared = [...declaredDeps].sort((a, b) => a - b);
    if (
      Number.isInteger(issueNumber) &&
      tsvRows.has(issueNumber) &&
      JSON.stringify(sortedListed) !== JSON.stringify(sortedDeclared)
    ) {
      problems.push(
        `listed dependencies [${sortedListed}] do not match ${TSV_PATH} row #${issueNumber} ` +
          `dependencies [${sortedDeclared}]`,
      );
    }
  }

  const baseSha = field(body, 'Base SHA');
  if (baseSha == null || !/^[0-9a-f]{40}$/.test(baseSha.trim())) {
    problems.push('PR body must declare "Base SHA: <40-hex commit>"');
  }

  const expectedHead = field(body, 'Expected head SHA');
  if (expectedHead == null || !/^[0-9a-f]{40}$/.test(expectedHead.trim())) {
    problems.push('PR body must declare "Expected head SHA: <40-hex commit>"');
  } else if (expectedHead.trim() !== head) {
    problems.push(`Expected head SHA ${expectedHead.trim()} does not match checked-out HEAD ${head}`);
  }

  return { ok: problems.length === 0, problems };
}

async function main() {
  const args = parseArgs(process.argv.slice(2));

  let head;
  try {
    head = execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim();
  } catch {
    fail('not inside a git worktree');
  }
  const tsvRows = readTsvRows('.');
  const windowMs = Number.parseInt(process.env.REVIEW_GATE_REFRESH_WINDOW_MS ?? '120000', 10);
  const intervalMs = 5000;

  let result = { ok: false, problems: [] };
  if (args.refresh) {
    const deadline = Date.now() + windowMs;
    let attempt = 0;
    for (;;) {
      attempt += 1;
      let body;
      try {
        body = fetchLivePrBody();
      } catch (error) {
        console.error(`live PR body fetch failed (attempt ${attempt}): ${error.message}`);
        body = '';
      }
      result = validate(body ?? '', head, tsvRows);
      if (result.ok) break;
      if (Date.now() >= deadline) break;
      console.error(
        `attempt ${attempt}: review gates not satisfied yet; retrying within the ` +
          `${windowMs / 1000}s convergence window (a push always precedes its own head declaration)`,
      );
      await new Promise((resolve) => setTimeout(resolve, intervalMs));
    }
  } else {
    const body = args.bodyFile != null ? readFileSync(args.bodyFile, 'utf8') : process.env.PR_BODY;
    if (body == null || body.trim().length === 0) {
      fail('no PR body provided; pass --refresh, --body-file <path>, or set PR_BODY');
    }
    result = validate(body, head, tsvRows);
  }

  if (!result.ok) {
    for (const problem of result.problems) console.error(problem);
    console.error('review-gate validation FAILED');
    process.exit(1);
  }
  console.log('review-gate validation passed');
}

function fail(message) {
  console.error(message);
  process.exit(1);
}

main();
