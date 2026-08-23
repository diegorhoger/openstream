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
// Inputs: PR body via env PR_BODY or --body-file <path>. Fails closed.

import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import process from 'node:process';

const TSV_PATH = 'docs/product/ROADMAP_GRAPH.tsv';

function fail(message) {
  console.error(message);
  process.exit(1);
}

function parseArgs(argv) {
  const args = { bodyFile: null };
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === '--body-file') {
      args.bodyFile = argv[i + 1] ?? fail('usage: check-review-gates.mjs [--body-file <path>]');
      i += 1;
    }
  }
  return args;
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

function main() {
  const args = parseArgs(process.argv.slice(2));
  const body = args.bodyFile != null ? readFileSync(args.bodyFile, 'utf8') : process.env.PR_BODY;
  if (body == null || body.trim().length === 0) {
    fail('no PR body provided; set PR_BODY or pass --body-file <path>');
  }

  let head;
  try {
    head = execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim();
  } catch {
    fail('not inside a git worktree');
  }
  const tsvRows = readTsvRows('.');

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
      problems.push(
        `"Dependencies merged" must start with yes/no (got "${mergedField}")`,
      );
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

  if (problems.length > 0) {
    for (const problem of problems) console.error(problem);
    console.error('review-gate validation FAILED');
    process.exit(1);
  }
  console.log('review-gate validation passed');
}

main();
