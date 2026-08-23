#!/usr/bin/env node
// Codegen dirty check for OpenStream (issue #6).
//
// Reads a manifest (default tools/codegen.json) declaring generated artifact
// paths and, when implemented, the deterministic command that regenerates
// them. Semantics, fail-closed:
//   - active generator  : run its command from the repo root, then require
//                         `git status --porcelain` to be empty for every
//                         declared generated path (drift or untracked output
//                         fails the build).
//   - pending generator : the declared generated path must NOT exist yet.
//                         A hand-authored "generated" file without a real
//                         generator is rejected.
// `--self-test` exercises both failure modes against throwaway fixtures so
// the failure paths themselves stay proven in CI.

import { execFileSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync, mkdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import process from 'node:process';

const DEFAULT_MANIFEST = 'tools/codegen.json';

function git(root, args) {
  return execFileSync('git', ['-C', root, ...args], {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  });
}

function loadManifest(manifestPath) {
  const raw = JSON.parse(readFileSync(manifestPath, 'utf8'));
  const problems = [];
  if (raw.version !== 1) {
    problems.push(`unsupported manifest version: ${String(raw.version)}`);
  }
  if (!Array.isArray(raw.generators)) {
    problems.push('manifest must contain a "generators" array');
    return { manifest: raw, problems };
  }
  const seen = new Set();
  raw.generators.forEach((entry, index) => {
    const where = `generators[${index}]`;
    if (!entry || typeof entry !== 'object') {
      problems.push(`${where} must be an object`);
      return;
    }
    if (typeof entry.id !== 'string' || entry.id.length === 0) {
      problems.push(`${where} needs a non-empty string id`);
    } else if (seen.has(entry.id)) {
      problems.push(`${where} duplicates id "${entry.id}"`);
    }
    if (seen.has(entry.id)) return;
    seen.add(entry.id);
    if (!['active', 'pending'].includes(entry.status)) {
      problems.push(`${where} status must be "active" or "pending"`);
    }
    if (
      !Array.isArray(entry.generated) ||
      entry.generated.length === 0 ||
      !entry.generated.every((p) => typeof p === 'string' && p.length > 0)
    ) {
      problems.push(`${where} needs a non-empty "generated" path array`);
    }
    if (entry.status === 'active' && (typeof entry.command !== 'string' || entry.command.length === 0)) {
      problems.push(`${where} active generator needs a non-empty "command"`);
    }
    if (entry.status === 'pending' && entry.command != null) {
      problems.push(`${where} pending generator must omit "command" until implemented`);
    }
  });
  return { manifest: raw, problems };
}

function dirtyPaths(root, generatedPaths) {
  const dirty = [];
  for (const declared of generatedPaths) {
    const rel = String(declared).replaceAll('\\', '/').replace(/\/+$/, '');
    const out = git(root, ['status', '--porcelain', '--', rel]);
    if (out.trim().length > 0) {
      for (const line of out.split('\n')) {
        if (line.trim().length > 0) dirty.push(line.trim());
      }
    }
  }
  return dirty;
}

function runGenerator(root, entry) {
  try {
    execFileSync(entry.command, {
      cwd: entry.workdir ? join(root, entry.workdir) : root,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
      shell: true,
    });
    return [];
  } catch (error) {
    return [
      `generator "${entry.id}" failed (exit ${error.status ?? 'unknown'}): ${error.message}`,
    ];
  }
}

export function runCheck(rootDir, manifestPath) {
  const { manifest, problems } = loadManifest(join(rootDir, manifestPath));
  if (problems.length > 0) return { ok: false, problems };

  for (const entry of manifest.generators) {
    if (entry.status === 'active') {
      problems.push(...runGenerator(rootDir, entry));
      const dirty = dirtyPaths(rootDir, entry.generated);
      if (dirty.length > 0) {
        problems.push(
          `generator "${entry.id}": declared generated paths are dirty; ` +
            'regenerate and commit the exact output:\n  ' +
            dirty.join('\n  '),
        );
      }
    } else {
      for (const declared of entry.generated) {
        if (existsSync(join(rootDir, declared))) {
          problems.push(
            `generator "${entry.id}" is pending but generated path "${declared}" ` +
              'exists on disk; hand-authored generated artifacts are forbidden',
          );
        }
      }
    }
  }
  return { ok: problems.length === 0, problems };
}

function selfTest() {
  const cases = [];

  const makeFixture = () => {
    const root = mkdtempSync(join(tmpdir(), 'openstream-codegen-'));
    const gitOpts = { stdio: 'ignore' };
    const gitArgs = (args) => ['-c', 'user.email=harness@example.invalid', '-c', 'user.name=harness', ...args];
    execFileSync('git', ['-C', root, 'init', '-q', '-b', 'main'], gitOpts);
    mkdirSync(join(root, 'src'), { recursive: true });
    return {
      root,
      commit(label) {
        execFileSync('git', ['-C', root, 'add', '-A'], gitOpts);
        execFileSync('git', ['-C', root, ...gitArgs(['commit', '-q', '-m', label])], gitOpts);
      },
    };
  };

  // Case 1: active generator, regenerated output matches committed state -> pass.
  {
    const fixture = makeFixture();
    const root = fixture.root;
    writeFileSync(join(root, 'src', 'out.txt'), 'v1\n');
    fixture.commit('baseline');
    writeFileSync(
      join(root, 'codegen.json'),
      JSON.stringify({
        version: 1,
        generators: [
          {
            id: 'fixture',
            status: 'active',
            command: `node -e "require('node:fs').writeFileSync('src/out.txt','v1\\n')"`,
            generated: ['src/out.txt'],
          },
        ],
      }),
    );
    cases.push({ name: 'active clean passes', root, expectOk: true });
  }

  // Case 2: active generator, drifted output -> fail.
  {
    const fixture = makeFixture();
    const root = fixture.root;
    writeFileSync(join(root, 'src', 'out.txt'), 'stale\n');
    fixture.commit('baseline');
    writeFileSync(
      join(root, 'codegen.json'),
      JSON.stringify({
        version: 1,
        generators: [
          {
            id: 'fixture',
            status: 'active',
            command: `node -e "require('node:fs').writeFileSync('src/out.txt','fresh\\n')"`,
            generated: ['src/out.txt'],
          },
        ],
      }),
    );
    cases.push({ name: 'active dirty fails', root, expectOk: false });
  }

  // Case 3: failing generator command -> fail.
  {
    const fixture = makeFixture();
    const root = fixture.root;
    writeFileSync(
      join(root, 'codegen.json'),
      JSON.stringify({
        version: 1,
        generators: [
          {
            id: 'fixture',
            status: 'active',
            command: `node -e "process.exit(3)"`,
            generated: ['src/out.txt'],
          },
        ],
      }),
    );
    cases.push({ name: 'failing generator fails', root, expectOk: false });
  }

  // Case 4: pending generator, artifact absent -> pass.
  {
    const fixture = makeFixture();
    const root = fixture.root;
    writeFileSync(
      join(root, 'codegen.json'),
      JSON.stringify({
        version: 1,
        generators: [{ id: 'future', status: 'pending', generated: ['src/gen/'] }],
      }),
    );
    cases.push({ name: 'pending absent passes', root, expectOk: true });
  }

  // Case 5: pending generator, hand-authored artifact -> fail.
  {
    const fixture = makeFixture();
    const root = fixture.root;
    mkdirSync(join(root, 'src', 'gen'), { recursive: true });
    writeFileSync(join(root, 'src', 'gen', 'proto.rs'), '// fake\n');
    writeFileSync(
      join(root, 'codegen.json'),
      JSON.stringify({
        version: 1,
        generators: [{ id: 'future', status: 'pending', generated: ['src/gen/'] }],
      }),
    );
    cases.push({ name: 'pending present fails', root, expectOk: false });
  }

  let failures = 0;
  for (const testCase of cases) {
    const result = runCheck(testCase.root, 'codegen.json');
    const passed = result.ok === testCase.expectOk;
    console.log(
      `${passed ? 'ok' : 'FAIL'} self-test/${testCase.name}` +
        (passed ? '' : ` :: expected ok=${testCase.expectOk}, got ${JSON.stringify(result.problems)}`),
    );
    if (!passed) failures += 1;
    rmSync(testCase.root, { recursive: true, force: true });
  }

  if (failures > 0) {
    console.error(`self-test: ${failures} case(s) failed`);
    process.exit(1);
  }
  console.log('self-test: all cases passed');
}

const invokedDirectly =
  process.argv[1] != null && fileURLToPath(import.meta.url) === process.argv[1];

if (invokedDirectly) {
  if (process.argv.includes('--self-test')) {
    selfTest();
    process.exit(0);
  }
  const manifestArg =
    process.argv.find((arg, index) => index >= 2 && arg.endsWith('.json')) ?? DEFAULT_MANIFEST;
  const repoRoot = process.cwd();
  const result = runCheck(repoRoot, manifestArg);
  for (const problem of result.problems) console.error(problem);
  if (!result.ok) {
    console.error(`codegen check FAILED (${manifestArg})`);
    process.exit(1);
  }
  console.log(`codegen check passed (${manifestArg})`);
}
