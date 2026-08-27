#!/usr/bin/env bash
# Smoke test: verify installer artifacts exist and are non-empty.
# Run after CI packaging job completes.
#
# Usage: bash tests/packaging/test-installer-output.sh [artifacts-dir]

set -euo pipefail

ARTIFACTS_DIR="${1:-target/release/bundle}"
ERRORS=0

step() { printf '\n==> %s\n' "$1"; }

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  ERRORS=$((ERRORS + 1))
}

step "Checking Windows artifacts (NSIS)"
if find "$ARTIFACTS_DIR" -name "*.exe" -size +1k 2>/dev/null | head -1 | grep -q .; then
  echo "  PASS: Windows NSIS installer found"
else
  fail "No Windows NSIS installer found in $ARTIFACTS_DIR"
fi

step "Checking macOS artifacts (DMG)"
if find "$ARTIFACTS_DIR" -name "*.dmg" -size +1k 2>/dev/null | head -1 | grep -q .; then
  echo "  PASS: macOS DMG found"
else
  fail "No macOS DMG found in $ARTIFACTS_DIR"
fi

step "Checking Linux artifacts (DEB)"
if find "$ARTIFACTS_DIR" -name "*.deb" -size +1k 2>/dev/null | head -1 | grep -q .; then
  echo "  PASS: Linux DEB found"
else
  fail "No Linux DEB found in $ARTIFACTS_DIR"
fi

step "Checking Linux artifacts (RPM)"
if find "$ARTIFACTS_DIR" -name "*.rpm" -size +1k 2>/dev/null | head -1 | grep -q .; then
  echo "  PASS: Linux RPM found"
else
  fail "No Linux RPM found in $ARTIFACTS_DIR"
fi

step "Checking Linux artifacts (AppImage)"
if find "$ARTIFACTS_DIR" -name "*.AppImage" -size +1k 2>/dev/null | head -1 | grep -q .; then
  echo "  PASS: Linux AppImage found"
else
  fail "No Linux AppImage found in $ARTIFACTS_DIR"
fi

step "Checking checksum files"
CHECKSUM_COUNT=$(find "$ARTIFACTS_DIR" -name "*.sha256" 2>/dev/null | wc -l)
if [ "$CHECKSUM_COUNT" -gt 0 ]; then
  echo "  PASS: $CHECKSUM_COUNT checksum file(s) found"
else
  fail "No checksum files found"
fi

echo ""
if [ $ERRORS -eq 0 ]; then
  echo "All installer output tests passed."
  exit 0
else
  echo "FAILED with $ERRORS error(s)."
  exit 1
fi
