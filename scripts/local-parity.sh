#!/usr/bin/env bash
# Local parity harness for OpenStream CI gates (issue #6).
#
# Runs the exact commands that `.github/workflows/quality.yml` enforces, in
# the same order. Gate-by-gate reference: docs/engineering/CHECKS.md.
#
# Prerequisites (already pinned by the repo):
#   - Rust 1.98.0 via rust-toolchain.toml (rustup installs it on demand)
#   - Node.js >= 24 and pnpm 10 (pnpm version comes from packageManager)
#   - git
# Optional binaries used when present (never downloaded by this script):
#   - cargo-deny   -> licenses/advisories gate locally
#   - gitleaks     -> secret scan locally
#
# The review-gate validation needs a pull-request body; pass one with
# PR_BODY_FILE=<path> to run it locally, otherwise it is skipped with a note
# because there is no PR object outside GitHub.

set -euo pipefail

root="$(git rev-parse --show-toplevel)"
cd "$root"

step() { printf '\n==> %s\n' "$1"; }

fail() {
  printf 'LOCAL PARITY FAILED: %s\n' "$1" >&2
  exit 1
}

step "rust: format check"
cargo fmt --all -- --check

step "rust: clippy"
cargo clippy --workspace --all-targets -- -D warnings

step "rust: tests"
cargo test --workspace

step "ui: install dependencies"
( cd apps/desktop/ui && pnpm install --frozen-lockfile )

step "ui: typecheck"
( cd apps/desktop/ui && pnpm typecheck )

step "ui: tests (unit, contrast/a11y, css parity, i18n)"
( cd apps/desktop/ui && pnpm test )

step "ui: build"
( cd apps/desktop/ui && pnpm build )

step "contracts: codegen check failure modes"
node scripts/check-codegen.mjs --self-test

step "contracts: codegen dirty check"
node scripts/check-codegen.mjs tools/codegen.json

if [[ -n "${PR_BODY_FILE:-}" ]]; then
  step "contracts: review-gate validation"
  node scripts/check-review-gates.mjs --body-file "$PR_BODY_FILE"
else
  step "contracts: review-gate validation SKIPPED (set PR_BODY_FILE to run it; enforced in CI)"
fi

step "supply-chain: cargo-deny (licenses / advisories)"
if command -v cargo-deny >/dev/null 2>&1; then
  # Feature activation comes from deny.toml [graph] all-features; the CLI
  # flag position differs across cargo-deny versions.
  cargo deny check
else
  echo "SKIPPED locally: cargo-deny not installed (CI runs it via a SHA-pinned action; see docs/engineering/CHECKS.md)"
fi

step "supply-chain: secret scan (gitleaks)"
if command -v gitleaks >/dev/null 2>&1; then
  gitleaks detect --redact
else
  echo "SKIPPED locally: gitleaks not installed (CI runs it via a SHA-pinned action; see docs/engineering/CHECKS.md)"
fi

step "artifacts: build Rust workspace binary (debug)"
cargo build --workspace

step "packaging: verify installer outputs (local bundle check)"
if [ -d "target/release/bundle" ]; then
  bash tests/packaging/test-installer-output.sh target/release/bundle
else
  echo "SKIPPED locally: no bundle directory (run 'cargo tauri build' first)"
fi

step "packaging: signing verification (local)"
if [ -d "target/release/bundle" ]; then
  bash tests/packaging/test-signing-verification.sh target/release/bundle
else
  echo "SKIPPED locally: no bundle directory (run 'cargo tauri build' first)"
fi

step "packaging: uninstall cleanup test"
bash tests/packaging/test-uninstall-cleanup.sh

printf '\nLocal parity complete. CI equivalents live in .github/workflows/quality.yml and .github/workflows/package.yml.\n'
