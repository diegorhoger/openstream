# Incident: PR #87 merged with dependency #25 unsatisfied (M2 #26)

**Status:** Repaired by `m1/pr87-repair` (head: see git log of branch).
**Date of incident:** 2026-08-27 (PR #87 merge SHA `e25c19f`).
**Date of repair:** see latest commit on `m1/pr87-repair`.
**Reporter:** OpenStream implementer + operator (Diego Rhoger).

## Summary

PR #87 ("M2 issue26: simulator fixtures + discovery fixtures (F1-F8)") was
merged into `main` at head `e25c19f2978915a9a8c6bb6602fc61b64ddaf430` on
2026-08-27 while its declared dependency `M2 #25` (pairing / identity
vectors) was still OPEN. The merge bypassed three required CI gates:

1. **Governance / provenance** — PR body did not carry the AGENT_* fields
   required by `.github/workflows/governance.yml`.
2. **Quality** — `cargo fmt --all -- --check` failed; the review-gate
   script (`scripts/check-review-gates.mjs`) was not run.
3. **Packaging** — `.github/workflows/package.yml` ran the macOS and
   Linux checksum steps against the wrong directories
   (`target/release/bundle/...` instead of the target-qualified
   `target/<triple>/release/bundle/...`); the macOS `cd target/release/bundle/dmg`
   step exited with "No such file or directory" and the Linux
   `cd target/release/bundle` step did the same. PR #87 was merged
   despite a `package` CI run that ended in `failure`.

## Dependency graph evidence

`docs/product/ROADMAP_GRAPH.tsv` row for issue #26:

| issue | milestone | dependencies | public_scope |
|------:|:---------:|:------------:|:-------------|
| 26    | M2        | 24, 25       | simulators-discovery-fixtures |

Issue #24 is closed; issue #25 is OPEN at the time of this incident.
Therefore the merge of PR #87 was a `dependencies merged: yes #24, no #25`
event and the body must reflect that truthfully.

## What this repair PR does

`m1/pr87-repair` (this branch) makes the following *bounded* corrections
to restore repository + CI integrity:

- Fixes `.github/workflows/package.yml` so the macOS and Linux checksum
  steps target the Tauri-emitted paths
  (`target/aarch64-apple-darwin/release/bundle/dmg/` and
  `target/x86_64-unknown-linux-gnu/release/bundle/{deb,rpm,appimage}/`)
  rather than the un-target-qualified `target/release/bundle/...`.
- Removes the broken `for f in *.deb *.rpm *.AppImage 2>/dev/null; do`
  pattern (the `2>/dev/null` only applied to the last glob, and
  with `nullglob` disabled the literal globs were iterated as filenames)
  and replaces it with one `case` per format, anchored to
  `${PWD}/target/...` so the loop returns to the bundle root reliably.
- Adds `set -euo pipefail`, `shopt -s nullglob`, and an explicit
  `test -d "$root" || { echo FAIL...; exit 1; }` so a missing or
  empty bundle directory fails closed instead of being swallowed by
  `find ... 2>/dev/null || true`.
- Adds a per-format `checksums.sha256` manifest plus a smoke-test
  assertion that **at least one** platform artifact AND **at least
  one** per-artifact `.sha256` are produced.

## What this repair PR does NOT do

- It does **not** close issue #26. Issue #26 cannot be considered
  complete until M2 #25 (pairing / identity vectors) is merged with its
  own independent cryptographic + security + verifier + reviewer + evaluator
  gate, AND a follow-up revalidation PR demonstrates that #26's
  fixtures integrate correctly against the merged #25 surface.
- It does **not** retroactively unmerge PR #87. The merge is a fact
  in `main` history; this repair only restores the CI gates for
  future merges so the same failure mode cannot recur.
- It does **not** include any release, tag, signing, or deployment
  action. Production signing remains BLOCKED per `signing/signing.md`
  and `AGENTS.md`.
- It does **not** re-run any Codex review quota. Code review for the
  follow-up #25 PR must use a fresh independent reviewer (human or
  agent), not the previously-exhausted Codex slot.

## Hard stop preserved

- This PR is opened as a **draft** and will not be merged autonomously.
- After CI is green at the exact head, the four independent gates
  (verifier, reviewer, security, evaluator) will be dispatched at that
  exact head. Only after all four return APPROVE **and** a human
  explicitly authorizes the merge will the merge occur.
- The follow-up #25 PR and the #26 revalidation PR are sequenced and
  may not be merged out of order. M2 #30 (security suite) and M3+
  work is blocked on #26's valid restoration.

## Repair PR revisions

| Head      | Change | Status |
|----------:|:-------|:-------|
| b8f2abf   | initial repair (governance/format only); workflow paths still wrong | superseded |
| f3015cb   | fix macOS/Linux bundle paths to target-qualified dirs; fail-closed checksum generation; remove invalid `*.deb *.rpm *.AppImage 2>/dev/null` pattern; add per-format `checksums.sha256`; durable in-tree incident record | superseded |
| <next>    | fix smoke-test checksum verification loop: `sha256sum --check` was called with the full relative path AFTER `cd` to the checksum's directory, so the path resolved to a non-existent file and the first per-artifact check always returned exit 1 | candidate |

The current candidate head on `m1/pr87-repair` is the only valid repair
until independent gates approve. Any push invalidates prior evidence.

