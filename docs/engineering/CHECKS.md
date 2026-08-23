# Checks and local parity

Every gate enforced by CI has a documented local equivalent. The authoritative
CI definition is `.github/workflows/quality.yml`; the one-shot local runner is
`scripts/local-parity.sh` (bash; on Windows use Git Bash). Policy gates
(provenance, DCO, repository contract) stay in
`.github/workflows/governance.yml`.

All third-party Actions are pinned to full commit SHAs. The Rust toolchain is
pinned by `rust-toolchain.toml` (1.98.0); pnpm is pinned by the UI
`packageManager` field (10.x); Node is 24.

## Gate reference

| Gate | CI job / step | Exact command |
| --- | --- | --- |
| Exact-head checkout + assertion | every `quality` job | checkout `github.event.pull_request.head.sha`, then `git rev-parse HEAD` compared against it |
| Rust format | `rust` | `cargo fmt --all -- --check` |
| Rust lint | `rust` | `cargo clippy --workspace --all-targets -- -D warnings` |
| Rust unit/integration tests | `rust` | `cargo test --workspace` |
| UI install | `ui` | `pnpm install --frozen-lockfile` (in `apps/desktop/ui`) |
| UI typecheck | `ui` | `pnpm typecheck` |
| UI tests (unit, contrast/a11y, CSS parity, i18n) | `ui` | `pnpm test` |
| UI build | `ui` | `pnpm build` |
| Codegen dirty check | `contracts` | `node scripts/check-codegen.mjs tools/codegen.json` (+ `--self-test` proves its failure modes) |
| Review-gate validation | `contracts` | `node scripts/check-review-gates.mjs` (PR body from the event) |
| Dependency licenses and advisories | `cargo-deny` | `cargo deny check` with committed `deny.toml` (`[graph] all-features = true`; CI passes the equivalent flag in the action's global position) |
| Secret scan | `secrets` | gitleaks via SHA-pinned action over full history |
| Artifact build (unsigned) | `artifacts` | `cargo build --workspace`; `pnpm build`; outputs uploaded as workflow artifacts |

## Codegen dirty check

`tools/codegen.json` declares generated artifact paths. Today no generator is
implemented (OSCP protobuf codegen lands in M2 per ADR-0003/ADR-0005), so the
single entry is `pending`: any hand-authored file under
`crates/openstream-protocol/src/gen/` fails CI. When a generator arrives,
declare it `active` with its deterministic `command`; CI then runs the command
and requires `git status --porcelain` to stay empty for the declared paths —
stale or untracked output fails the build. `--self-test` exercises both
failure modes in CI so the failure paths themselves stay proven.

## Review-gate validation

`scripts/check-review-gates.mjs` machine-checks the offline-verifiable parts
of `docs/engineering/REVIEW_GATES.md`: the declared issue exists as a row in
`docs/product/ROADMAP_GRAPH.tsv`, listed dependencies match that row exactly,
`Base SHA` / `Expected head SHA` are well-formed, and the expected head equals
the exact checked-out HEAD. Provenance (`AGENT_*`) and DCO enforcement remain
in `governance.yml`.

In CI the step reads the live pull-request body through the API
(`--refresh`, job permission `pull-requests: read`) with a bounded 120s
convergence window: a push always precedes the body edit that declares its
head SHA, so CI waits briefly for that declaration and fails closed if it
never arrives.

To run locally: `node scripts/check-review-gates.mjs --body-file <path>`
(or set `PR_BODY_FILE=<path>` for the parity script).

## Local parity

```bash
bash scripts/local-parity.sh
# optional: PR_BODY_FILE=pr-body.md bash scripts/local-parity.sh
```

The script runs the identical commands above in CI order. It never downloads
executables: if `cargo-deny` or `gitleaks` binaries are not installed locally,
those two steps print a skip note and remain CI-enforced.

## Known gaps (explicit, not silent)

- Full desktop bundling (`tauri build` producing .msi/.deb/AppImage/dmg) is a
  release-milestone activity gated by protected environments, signing keys,
  and human authorization (REVIEW_GATES.md). The current artifact check builds
  the Rust workspace binary and the signed-nothing UI bundle only.
- Linux CI installs GTK/WebKitGTK development headers because compiling
  `apps/desktop/src-tauri` (tauri/wry) needs them even for clippy/tests.
