# Incident: PR #87 merged with dependency #25 unsatisfied (M2 #26)

**Status:** Repair in progress; awaiting independent gates and human merge.
**Date of incident:** 2026-08-27 (PR #87 source head `e25c19f`; squash merge commit on `main` = `6155457ee66659e2dc596f95b79624789cf33e33`).
**Reporter:** OpenStream implementer + operator (Diego Rhoger).

## PR #87 exact provenance (for the record)

| field             | value                                      |
|-------------------|--------------------------------------------|
| PR number         | #87                                        |
| PR source head    | `e25c19f2978915a9a8c6bb6602fc61b64ddaf430` (NOT the commit on `main`) |
| PR base           | `4b57929465d306b2c1d21b6db121243d39865132` |
| Squash merge commit on `main` | `6155457ee66659e2dc596f95b79624789cf33e33` (the head of `main` after merge) |
| Merged at         | 2026-08-27T20:41:28Z                        |
| Title             | "M2 issue26: simulator fixtures + discovery fixtures (F1-F8)" |

The squash merge commit `6155457` is the one whose checks/PR-body/dependencies
are evaluated; `e25c19f` is the source branch tip, not the commit on `main`.

## Summary

PR #87 (source head `e25c19f`, squash-merged to `main` as `6155457`) was merged
while its declared dependency `M2 #25` (pairing / identity vectors) was still
OPEN. The merge bypassed three required CI gates:

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

## Dependency graph evidence (dependency breach UNRESOLVED)

`docs/product/ROADMAP_GRAPH.tsv` row for issue #26:

| issue | milestone | dependencies | public_scope |
|------:|:---------:|:------------:|:-------------|
| 26    | M2        | 24, 25       | simulators-discovery-fixtures |

Issue #24 is closed; issue #25 is **OPEN**. Therefore the merge of PR #87
was a `dependencies merged: yes #24, no #25` event, and the body must
reflect that truthfully. This repair PR does **NOT** resolve the #25
dependency breach and does **NOT** close issue #26.

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
- Requires each Linux format (`deb`, `rpm`, `appimage`) to be present
  and non-empty; missing any one fails closed (no longer "fail only
  when all three are absent").
- Re-derives every `checksums.sha256` aggregate from the installer
  files themselves via `sha256sum ... | tee -a checksums.sha256`, so
  the aggregate hashes INSTALLER artifacts and not their sidecar
  checksum files.
- Strengthens the smoke test to require and verify one of each
  platform format (EXE, DMG, DEB, RPM, AppImage) plus a matching
  per-artifact `.sha256` sidecar; missing any expected format or
  checksum fails closed.
- Excludes `checksums.sha256` and `checksums-manifest.txt` from the
  combined manifest so it lists only installer hashes.
- Fixes the smoke-test verify loop: `sha256sum --check` is now called
  with the basename after `cd` (the previous form used the full
  relative path that no longer existed once the loop was in the
  artifact directory).
- Adds a durable in-tree record of the dependency breach.

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
- The four independent gates (verifier, reviewer, security,
  evaluator) have NOT been dispatched yet. They will be dispatched
  only after the operator authorizes dispatch, and only at the
  exact head that is currently green on every workflow. Only after
  all four return APPROVE **and** a human explicitly authorizes the
  merge will the merge occur.
- The follow-up #25 PR and the #26 revalidation PR are sequenced and
  may not be merged out of order. M2 #30 (security suite) and M3+
  work is blocked on #26's valid restoration.

## Live-state reconciliation (as of 2026-09-01)

| Issue | Live state | How it got there | This repair's effect |
|------:|:----------:|:-----------------|:---------------------|
| #25 (pairing / identity vectors) | OPEN | never merged | no change; this repair does not address #25 |
| #26 (simulator fixtures) | CLOSED | CLOSED at PR #87's original merge (2026-08-27) at squash commit `6155457`; NOT closed by this repair | this repair does not re-close #26 and does not claim revalidation |
| #26 revalidation | blocked on #25 | sequence rule from the user-authorized repair plan | not started; this repair does not unblock it |

The CLOSED state of #26 is from the original PR #87 merge that violated
the #25 dependency, not from this repair. The dependency breach
(#25 OPEN at the time of PR #87's merge) is preserved by the truthful
`Dependencies merged: yes #24, no #25` declaration in the PR body.
This repair cannot be revalidated against a still-OPEN #25; that is
why revalidation is sequenced strictly after #25 lands.

## Repair PR revisions (branch `m1/pr87-repair`, draft)

| Head      | Files changed                                                                                                                                                | Status |
|----------:|:-------------------------------------------------------------------------------------------------------------------------------------------------------------|:-------|
| b8f2abf   | rustfmt cleanup (4 Rust files) + `.github/workflows/package.yml` (minor): `crates/openstream-discovery/tests/discovery_fixtures.rs`, `crates/openstream-engine/src/fixtures/simulator_fixtures.rs`, `crates/openstream-engine/src/lib.rs`, `crates/openstream-engine/tests/simulator_fixtures.rs` | superseded |
| f3015cb   | `.github/workflows/package.yml` (target-qualified bundle paths, fail-closed checksum gen, remove invalid glob); `docs/incidents/PR87-merge-breach.md` (initial incident record) | superseded |
| 01deb91   | `.github/workflows/package.yml` (smoke-test verify loop uses basename after `cd`); `docs/incidents/PR87-merge-breach.md` (revision history table)              | superseded |
| a914d36   | `.github/workflows/package.yml` (aggregate `checksums.sha256` derives from installer files via `tee -a`; Linux fail-closed per format; smoke test requires each platform format); `docs/incidents/PR87-merge-breach.md` (corrected PR #87 provenance and new revision row) | Superseded; clean-context adversarial review `OSTR-PR96-ADV-A914D-20260901-BR7` returned `REQUEST_CHANGES / BLOCKED` on three findings. |
| d97068a   | `.github/workflows/package.yml` (self-verifiable combined manifest); `.github/workflows/governance.yml` and `AGENTS.md` (shape-only operator-token experiment); incident reconciliation | Superseded for review purposes. Four clean-context reviews returned `REQUEST_CHANGES`: the token shape did not authenticate independence. Packaging and incident findings were repaired. |
| (review candidate) | Replace the rejected operator-token claim with truthful orchestrator-provided clean-context provenance, exact-head verdict binding, and durable evidence requirements. Grant autonomous integration after all required clean-context reviewers approve; preserve human hard stops. | Awaiting exact-head CI and fresh clean-context review. |

The current candidate head on `m1/pr87-repair` is the only valid repair
until independent gates approve. Any push invalidates prior evidence
(including the `OSTR-PR96-ADV-A914D-20260901-BR7` review above) and
requires a fresh exact-head review.

The 4 Rust files in `b8f2abf` were authorized rustfmt-only repairs to
make `cargo fmt --all -- --check` pass on PR #87's source tree; no
semantic changes.
