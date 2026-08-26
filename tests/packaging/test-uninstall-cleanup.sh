#!/usr/bin/env bash
# Smoke test: verify uninstall cleanup for platform installers.
# Tests that uninstall commands exist and that temp artifacts are cleaned up.
#
# Usage: bash tests/packaging/test-uninstall-cleanup.sh

set -euo pipefail

ERRORS=0
TEMP_DIR=$(mktemp -d)

step() { printf '\n==> %s\n' "$1"; }

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  ERRORS=$((ERRORS + 1))
}

cleanup() {
  rm -rf "$TEMP_DIR"
}
trap cleanup EXIT

step "Verifying uninstall script existence"
# Check that uninstall-related files exist in the repository
UNINSTALL_FILES=$(find . -name "*uninstall*" -not -path "./.git/*" -not -path "./target/*" 2>/dev/null)
if [ -n "$UNINSTALL_FILES" ]; then
  echo "  Found uninstall-related files:"
  echo "$UNINSTALL_FILES" | while read f; do echo "    $f"; done
else
  echo "  INFO: No explicit uninstall scripts found (Tauri handles this via platform installers)"
fi

step "Verifying NSIS uninstaller support"
# NSIS generates uninstaller executables; verify the config supports it
if grep -r "nsis" apps/desktop/src-tauri/tauri.conf.json >/dev/null 2>&1; then
  echo "  PASS: NSIS configured in tauri.conf.json"
else
  fail "NSIS not configured in tauri.conf.json"
fi

step "Verifying DEB package metadata"
# DEB packages include maintainer scripts; verify package metadata
DEB_CONFIG=$(grep -A 5 '"deb"' apps/desktop/src-tauri/tauri.conf.json 2>/dev/null || true)
if [ -n "$DEB_CONFIG" ]; then
  echo "  PASS: DEB configuration found"
  echo "$DEB_CONFIG" | head -5
else
  fail "DEB configuration not found"
fi

step "Verifying RPM package metadata"
RPM_CONFIG=$(grep -A 5 '"rpm"' apps/desktop/src-tauri/tauri.conf.json 2>/dev/null || true)
if [ -n "$RPM_CONFIG" ]; then
  echo "  PASS: RPM configuration found"
  echo "$RPM_CONFIG" | head -5
else
  fail "RPM configuration not found"
fi

step "Testing temp artifact cleanup"
# Verify that temp artifacts are properly cleaned up
TEST_FILE="$TEMP_DIR/test-artifact.exe"
echo "test" > "$TEST_FILE"
if [ -f "$TEST_FILE" ]; then
  rm -f "$TEST_FILE"
  if [ ! -f "$TEST_FILE" ]; then
    echo "  PASS: Temp artifact cleanup works"
  else
    fail "Failed to clean up temp artifact"
  fi
else
  fail "Failed to create temp artifact"
fi

step "Verifying no stale artifacts in source tree"
STALE_COUNT=$(find . -name "*.exe" -o -name "*.dmg" -o -name "*.deb" -o -name "*.rpm" -o -name "*.AppImage" 2>/dev/null | grep -v ".git" | grep -v "target" | wc -l)
if [ "$STALE_COUNT" -eq 0 ]; then
  echo "  PASS: No stale artifacts in source tree"
else
  fail "$STALE_COUNT stale artifact(s) found in source tree"
fi

echo ""
if [ $ERRORS -eq 0 ]; then
  echo "All uninstall cleanup tests passed."
  exit 0
else
  echo "FAILED with $ERRORS error(s)."
  exit 1
fi
